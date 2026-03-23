#!/usr/bin/env node
/**
 * cred-proxy — M27 Credential Opacity
 *
 * Reads credentials from GYRE_CRED_* environment variables at startup,
 * scrubs them from process.env, then listens on:
 *   - Unix socket:  /run/gyre/cred.sock       (primary proxy)
 *   - TCP loopback: 127.0.0.1:8765            (ANTHROPIC_BASE_URL compat)
 *   - TCP loopback: 127.0.0.1:8080            (GCE metadata emulator for Vertex AI)
 *
 * Routing (Unix socket + TCP 8765):
 *   /v1/*      → https://api.anthropic.com/v1/*  (injects x-api-key)
 *   POST /proxy → explicit proxy: {target_url, method, headers?, body?}
 *   CONNECT    → rejected
 *
 * GCE metadata emulator (TCP 8080):
 *   GET /computeMetadata/v1/instance/service-accounts/default/token
 *   → OAuth2 token exchange using GYRE_CRED_GCP_SA_JSON, cached until expiry
 *
 * Audit log format (stderr): {timestamp, method, url_prefix, status}
 * Credential values are NEVER logged.
 */

import http from 'http';
import https from 'https';
import crypto from 'crypto';
import fs from 'fs';

const SOCKET_PATH = '/run/gyre/cred.sock';
const PROXY_TCP_PORT = parseInt(process.env.GYRE_CRED_PROXY_PORT ?? '8765', 10);
const GCE_METADATA_PORT = parseInt(process.env.GYRE_CRED_GCE_PORT ?? '8080', 10);
const LOOPBACK = '127.0.0.1';
const GCP_SCOPES = ['https://www.googleapis.com/auth/cloud-platform'];

// ── Credential loading ────────────────────────────────────────────────────────

/** Load all GYRE_CRED_* env vars into a Map, then scrub them from process.env. */
function loadCredentials() {
  const creds = new Map();
  for (const [k, v] of Object.entries(process.env)) {
    if (k.startsWith('GYRE_CRED_')) {
      creds.set(k.slice('GYRE_CRED_'.length), v);
      delete process.env[k];
    }
  }
  return creds;
}

const credentials = loadCredentials();

const ANTHROPIC_API_KEY = credentials.get('ANTHROPIC_API_KEY') ?? '';
const GITLAB_TOKEN = credentials.get('GITLAB_TOKEN') ?? '';
const GITHUB_TOKEN = credentials.get('GITHUB_TOKEN') ?? '';
const GYRE_AUTH_TOKEN = process.env.GYRE_AUTH_TOKEN ?? '';
const GYRE_SERVER_URL = (process.env.GYRE_SERVER_URL ?? '').replace(/\/$/, '');

// Parse GCP service account JSON if present
let gcpServiceAccount = null;
const gcpSaJson = credentials.get('GCP_SA_JSON');
if (gcpSaJson) {
  try {
    gcpServiceAccount = JSON.parse(gcpSaJson);
  } catch (e) {
    process.stderr.write(`cred-proxy: WARNING — GCP_SA_JSON is not valid JSON: ${e.message}\n`);
  }
}

// ── Audit logging ─────────────────────────────────────────────────────────────

function auditLog(method, urlPrefix, status) {
  // Credential values are NEVER included in audit output
  process.stderr.write(
    JSON.stringify({ timestamp: new Date().toISOString(), method, url_prefix: urlPrefix, status }) + '\n'
  );
}

// ── GCE Metadata / OAuth2 token exchange ─────────────────────────────────────

let cachedGcpToken = null; // { access_token, expires_at_ms }

/**
 * Create a signed JWT assertion for OAuth2 service account auth.
 * Uses Node's built-in crypto (no external deps).
 */
function createJwtAssertion(sa, scopes) {
  const now = Math.floor(Date.now() / 1000);
  const header = Buffer.from(JSON.stringify({ alg: 'RS256', typ: 'JWT' })).toString('base64url');
  const payload = Buffer.from(JSON.stringify({
    iss: sa.client_email,
    scope: scopes.join(' '),
    aud: 'https://oauth2.googleapis.com/token',
    iat: now,
    exp: now + 3600,
  })).toString('base64url');
  const unsigned = `${header}.${payload}`;
  const sign = crypto.createSign('RSA-SHA256');
  sign.update(unsigned);
  return `${unsigned}.${sign.sign(sa.private_key, 'base64url')}`;
}

/**
 * Exchange a service account JWT for a short-lived access token.
 * Caches the result; refreshes 60 s before expiry.
 */
async function getGcpAccessToken() {
  const now = Date.now();
  if (cachedGcpToken && cachedGcpToken.expires_at_ms - 60_000 > now) {
    return cachedGcpToken.access_token;
  }

  if (!gcpServiceAccount) {
    throw new Error('GCP service account not configured (GYRE_CRED_GCP_SA_JSON missing)');
  }

  const assertion = createJwtAssertion(gcpServiceAccount, GCP_SCOPES);
  const body = `grant_type=${encodeURIComponent('urn:ietf:params:oauth:grant-type:jwt-bearer')}&assertion=${encodeURIComponent(assertion)}`;

  const result = await new Promise((resolve, reject) => {
    const req = https.request(
      {
        hostname: 'oauth2.googleapis.com',
        port: 443,
        path: '/token',
        method: 'POST',
        headers: {
          'Content-Type': 'application/x-www-form-urlencoded',
          'Content-Length': String(Buffer.byteLength(body)),
        },
      },
      (res) => {
        const chunks = [];
        res.on('data', (c) => chunks.push(c));
        res.on('end', () => resolve({ status: res.statusCode, body: Buffer.concat(chunks).toString() }));
      }
    );
    req.on('error', reject);
    req.write(body);
    req.end();
  });

  if (result.status !== 200) {
    throw new Error(`OAuth2 exchange failed (HTTP ${result.status})`);
  }

  const json = JSON.parse(result.body);
  cachedGcpToken = {
    access_token: json.access_token,
    expires_at_ms: now + (json.expires_in ?? 3600) * 1000,
    expires_in: json.expires_in ?? 3600,
  };
  auditLog('POST', 'oauth2.googleapis.com/token', result.status);
  return cachedGcpToken.access_token;
}

// ── Core proxy ────────────────────────────────────────────────────────────────

/**
 * Forward a request to targetUrl, injecting the appropriate auth credential.
 */
async function proxyRequest(targetUrl, method, reqHeaders, body) {
  const url = new URL(targetUrl);
  const isHttps = url.protocol === 'https:';
  const transport = isHttps ? https : http;
  const outHeaders = { ...reqHeaders };

  // Credential injection by destination host
  if (url.hostname === 'api.anthropic.com') {
    // Replace any placeholder (proxy-managed) with the real key
    outHeaders['x-api-key'] = ANTHROPIC_API_KEY;
    if (!outHeaders['anthropic-version']) outHeaders['anthropic-version'] = '2023-06-01';
  } else if (url.hostname === 'gitlab.com' || url.hostname.endsWith('.gitlab.com')) {
    outHeaders['private-token'] = GITLAB_TOKEN;
  } else if (url.hostname === 'api.github.com' || url.hostname.endsWith('.github.com')) {
    outHeaders['authorization'] = `token ${GITHUB_TOKEN}`;
  } else if (GYRE_SERVER_URL && targetUrl.startsWith(GYRE_SERVER_URL)) {
    outHeaders['authorization'] = `Bearer ${GYRE_AUTH_TOKEN}`;
  }

  const bodyBuf = body
    ? Buffer.isBuffer(body) ? body : Buffer.from(body)
    : null;
  if (bodyBuf) outHeaders['content-length'] = String(bodyBuf.length);

  return new Promise((resolve, reject) => {
    const req = transport.request(
      {
        hostname: url.hostname,
        port: url.port || (isHttps ? 443 : 80),
        path: url.pathname + url.search,
        method,
        headers: outHeaders,
      },
      (res) => {
        const chunks = [];
        res.on('data', (c) => chunks.push(c));
        res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) }));
      }
    );
    req.on('error', reject);
    if (bodyBuf) req.write(bodyBuf);
    req.end();
  });
}

// ── Main proxy request handler ────────────────────────────────────────────────

async function handleProxy(req, res) {
  // Reject CONNECT tunnels (prevents MITM relay abuse)
  if (req.method === 'CONNECT') {
    auditLog('CONNECT', req.url, 405);
    res.writeHead(405, { 'Content-Type': 'text/plain' });
    res.end('CONNECT not supported');
    return;
  }

  // POST /proxy — explicit forwarding with {target_url, method, headers?, body?}
  if (req.method === 'POST' && req.url === '/proxy') {
    let raw = '';
    for await (const chunk of req) raw += chunk;
    let payload;
    try {
      payload = JSON.parse(raw);
    } catch {
      res.writeHead(400, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ error: 'invalid JSON body' }));
      return;
    }
    const { target_url, method = 'GET', headers = {}, body } = payload;
    if (!target_url) {
      res.writeHead(400, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ error: 'target_url required' }));
      return;
    }
    // M27-A: Destination allowlist — reject requests to unlisted hosts.
    const ALLOWED_HOSTS = (process.env.GYRE_CRED_ALLOWED_HOSTS || 'api.anthropic.com,gitlab.com,api.github.com').split(',').map(h => h.trim().toLowerCase());
    try {
      const targetHost = new URL(target_url).hostname.toLowerCase();
      if (!ALLOWED_HOSTS.some(h => targetHost === h || targetHost.endsWith('.' + h))) {
        auditLog(method, targetHost, 403);
        res.writeHead(403, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ error: `host '${targetHost}' not in GYRE_CRED_ALLOWED_HOSTS allowlist` }));
        return;
      }
    } catch {
      res.writeHead(400, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ error: 'invalid target_url' }));
      return;
    }
    let logPrefix = target_url;
    try { const u = new URL(target_url); logPrefix = u.hostname + u.pathname.substring(0, 20); } catch {}
    try {
      const result = await proxyRequest(target_url, method, headers, body);
      auditLog(method, logPrefix, result.status);
      res.writeHead(result.status, result.headers);
      res.end(result.body);
    } catch (err) {
      auditLog(method, logPrefix, 502);
      res.writeHead(502, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ error: err.message }));
    }
    return;
  }

  // /v1/* — transparent Anthropic API proxy
  if (req.url.startsWith('/v1/')) {
    const targetUrl = `https://api.anthropic.com${req.url}`;
    const bodyChunks = [];
    for await (const chunk of req) bodyChunks.push(chunk);
    const body = bodyChunks.length ? Buffer.concat(bodyChunks) : null;
    const passHeaders = {};
    for (const [k, v] of Object.entries(req.headers)) {
      const lower = k.toLowerCase();
      if (['connection', 'host', 'transfer-encoding'].includes(lower)) continue;
      passHeaders[lower] = v;
    }
    const logPrefix = `api.anthropic.com${req.url.substring(0, 20)}`;
    try {
      const result = await proxyRequest(targetUrl, req.method, passHeaders, body);
      auditLog(req.method, logPrefix, result.status);
      const resHeaders = {};
      for (const [k, v] of Object.entries(result.headers)) {
        if (['connection', 'transfer-encoding'].includes(k.toLowerCase())) continue;
        resHeaders[k] = v;
      }
      res.writeHead(result.status, resHeaders);
      res.end(result.body);
    } catch (err) {
      auditLog(req.method, logPrefix, 502);
      res.writeHead(502, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ error: err.message }));
    }
    return;
  }

  auditLog(req.method, req.url, 404);
  res.writeHead(404, { 'Content-Type': 'application/json' });
  res.end(JSON.stringify({ error: 'no route matched', path: req.url }));
}

// ── GCE metadata server emulator handler ─────────────────────────────────────

async function handleGceMetadata(req, res) {
  const TOKEN_PATH = '/computeMetadata/v1/instance/service-accounts/default/token';

  if (req.method === 'GET' && req.url === TOKEN_PATH) {
    try {
      const token = await getGcpAccessToken();
      const remaining = cachedGcpToken
        ? Math.max(0, Math.floor((cachedGcpToken.expires_at_ms - Date.now()) / 1000))
        : 3599;
      auditLog('GET', 'gce-metadata/token', 200);
      res.writeHead(200, {
        'Content-Type': 'application/json',
        'Metadata-Flavor': 'Google',
      });
      res.end(JSON.stringify({ access_token: token, expires_in: remaining, token_type: 'Bearer' }));
    } catch (err) {
      auditLog('GET', 'gce-metadata/token', 503);
      res.writeHead(503, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ error: err.message }));
    }
    return;
  }

  // Basic liveness: root path returns 200 (some SDKs probe this)
  if (req.method === 'GET' && (req.url === '/' || req.url === '/computeMetadata/v1/')) {
    res.writeHead(200, { 'Metadata-Flavor': 'Google', 'Content-Type': 'text/plain' });
    res.end('gyre-cred-proxy GCE metadata emulator\n');
    return;
  }

  auditLog(req.method, `gce-metadata${req.url.substring(0, 30)}`, 404);
  res.writeHead(404, { 'Content-Type': 'application/json' });
  res.end(JSON.stringify({ error: 'not found', path: req.url }));
}

// ── Server factories ──────────────────────────────────────────────────────────

function makeProxyServer() {
  return http.createServer((req, res) => {
    handleProxy(req, res).catch((err) => {
      process.stderr.write(`cred-proxy error: ${err.message}\n`);
      if (!res.headersSent) { res.writeHead(500); res.end('internal error'); }
    });
  });
}

function makeGceServer() {
  return http.createServer((req, res) => {
    handleGceMetadata(req, res).catch((err) => {
      process.stderr.write(`cred-proxy gce error: ${err.message}\n`);
      if (!res.headersSent) { res.writeHead(500); res.end('internal error'); }
    });
  });
}

// ── Startup ───────────────────────────────────────────────────────────────────

fs.mkdirSync('/run/gyre', { recursive: true });

// Unix socket (all proxy routes)
if (fs.existsSync(SOCKET_PATH)) fs.unlinkSync(SOCKET_PATH);
const unixServer = makeProxyServer();
unixServer.listen(SOCKET_PATH, () => {
  fs.chmodSync(SOCKET_PATH, 0o660);
  process.stderr.write(`cred-proxy: unix socket ready at ${SOCKET_PATH}\n`);
});

// TCP loopback proxy (ANTHROPIC_BASE_URL compat)
const tcpServer = makeProxyServer();
tcpServer.listen(PROXY_TCP_PORT, LOOPBACK, () => {
  process.stderr.write(`cred-proxy: proxy TCP ready at ${LOOPBACK}:${PROXY_TCP_PORT}\n`);
  process.stderr.write(
    `cred-proxy: ${credentials.size} credential(s) loaded, GYRE_CRED_* scrubbed from env\n`
  );
});

// GCE metadata emulator (for Vertex AI SDK)
if (gcpServiceAccount) {
  const gceServer = makeGceServer();
  gceServer.listen(GCE_METADATA_PORT, LOOPBACK, () => {
    process.stderr.write(
      `cred-proxy: GCE metadata emulator ready at ${LOOPBACK}:${GCE_METADATA_PORT}\n`
    );
  });
}

process.on('SIGTERM', () => {
  unixServer.close();
  tcpServer.close();
  if (fs.existsSync(SOCKET_PATH)) fs.unlinkSync(SOCKET_PATH);
});
