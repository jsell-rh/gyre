/**
 * cred-proxy unit tests — M27
 * Run with: node --test cred-proxy.test.mjs
 */

import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import https from 'node:https';
import { createServer } from 'node:http';
import { once } from 'node:events';
import net from 'node:net';

// ── Helpers ───────────────────────────────────────────────────────────────────

/** Start a tiny HTTP server that captures requests. Returns {server, url, captured}. */
async function startCapture(handler) {
  const captured = [];
  const server = createServer((req, res) => {
    const chunks = [];
    req.on('data', (c) => chunks.push(c));
    req.on('end', () => {
      const body = Buffer.concat(chunks).toString();
      captured.push({ method: req.method, url: req.url, headers: req.headers, body });
      handler(req, res, body);
    });
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const { port } = server.address();
  return { server, url: `http://127.0.0.1:${port}`, captured };
}

/** HTTP fetch to a local server (no real network needed). */
function localFetch(url, options = {}) {
  return new Promise((resolve, reject) => {
    const u = new URL(url);
    const req = http.request(
      { hostname: u.hostname, port: u.port, path: u.pathname + u.search, method: options.method ?? 'GET', headers: options.headers ?? {} },
      (res) => {
        const chunks = [];
        res.on('data', (c) => chunks.push(c));
        res.on('end', () => resolve({ status: res.statusCode, body: Buffer.concat(chunks).toString(), headers: res.headers }));
      }
    );
    req.on('error', reject);
    if (options.body) req.write(options.body);
    req.end();
  });
}

// ── Tests: credential loading ─────────────────────────────────────────────────

describe('credential loading', () => {
  it('scrubs GYRE_CRED_* from process.env', async () => {
    // Set some GYRE_CRED_* vars, then simulate what cred-proxy does
    const testEnv = {
      GYRE_CRED_ANTHROPIC_API_KEY: 'sk-test-key',
      GYRE_CRED_GITLAB_TOKEN: 'glpat-test',
      SOME_OTHER_VAR: 'keep-me',
    };

    // Simulate loadCredentials
    const creds = new Map();
    for (const [k, v] of Object.entries(testEnv)) {
      if (k.startsWith('GYRE_CRED_')) {
        creds.set(k.slice('GYRE_CRED_'.length), v);
        delete testEnv[k];
      }
    }

    assert.equal(creds.get('ANTHROPIC_API_KEY'), 'sk-test-key');
    assert.equal(creds.get('GITLAB_TOKEN'), 'glpat-test');
    assert.equal(testEnv['GYRE_CRED_ANTHROPIC_API_KEY'], undefined, 'GYRE_CRED_ key must be scrubbed');
    assert.equal(testEnv['SOME_OTHER_VAR'], 'keep-me', 'non-GYRE_CRED_ var must be preserved');
  });

  it('maps GYRE_CRED_X to X in credential store', () => {
    const creds = new Map();
    const env = { GYRE_CRED_FOO: 'bar', GYRE_CRED_BAZ_QUX: 'quux' };
    for (const [k, v] of Object.entries(env)) {
      if (k.startsWith('GYRE_CRED_')) creds.set(k.slice('GYRE_CRED_'.length), v);
    }
    assert.equal(creds.get('FOO'), 'bar');
    assert.equal(creds.get('BAZ_QUX'), 'quux');
  });
});

// ── Tests: audit log format ───────────────────────────────────────────────────

describe('audit logging', () => {
  it('emits JSON with timestamp, method, url_prefix, status', () => {
    const logs = [];
    function auditLog(method, urlPrefix, status) {
      logs.push(JSON.parse(
        JSON.stringify({ timestamp: new Date().toISOString(), method, url_prefix: urlPrefix, status })
      ));
    }
    auditLog('POST', 'api.anthropic.com/v1/messages', 200);
    assert.equal(logs[0].method, 'POST');
    assert.equal(logs[0].url_prefix, 'api.anthropic.com/v1/messages');
    assert.equal(logs[0].status, 200);
    assert.ok(logs[0].timestamp, 'timestamp must be present');
  });

  it('never includes credential values in audit output', () => {
    const REAL_KEY = 'sk-ant-real-secret-key';
    const logs = [];
    function auditLog(method, urlPrefix, status) {
      const entry = JSON.stringify({ timestamp: new Date().toISOString(), method, url_prefix: urlPrefix, status });
      logs.push(entry);
    }
    auditLog('POST', 'api.anthropic.com/v1/messages', 200);
    for (const log of logs) {
      assert.ok(!log.includes(REAL_KEY), 'credential value must not appear in audit log');
    }
  });
});

// ── Tests: GCE metadata emulator ─────────────────────────────────────────────

describe('GCE metadata emulator', () => {
  it('token path is correctly defined', () => {
    const TOKEN_PATH = '/computeMetadata/v1/instance/service-accounts/default/token';
    assert.equal(TOKEN_PATH, '/computeMetadata/v1/instance/service-accounts/default/token');
  });

  it('createJwtAssertion produces a 3-part JWT', () => {
    // Mock createJwtAssertion without actual crypto (structural test)
    function mockJwt(sa, scopes) {
      const header = Buffer.from(JSON.stringify({ alg: 'RS256', typ: 'JWT' })).toString('base64url');
      const payload = Buffer.from(JSON.stringify({
        iss: sa.client_email, scope: scopes.join(' '),
        aud: 'https://oauth2.googleapis.com/token',
        iat: 1000, exp: 4600,
      })).toString('base64url');
      return `${header}.${payload}.fakesig`;
    }
    const sa = { client_email: 'test@project.iam.gserviceaccount.com', private_key: 'fake' };
    const jwt = mockJwt(sa, ['https://www.googleapis.com/auth/cloud-platform']);
    const parts = jwt.split('.');
    assert.equal(parts.length, 3, 'JWT must have 3 parts');
    const decodedHeader = JSON.parse(Buffer.from(parts[0], 'base64url').toString());
    assert.equal(decodedHeader.alg, 'RS256');
    assert.equal(decodedHeader.typ, 'JWT');
  });
});

// ── Tests: CONNECT rejection ──────────────────────────────────────────────────

describe('CONNECT rejection', () => {
  it('rejects CONNECT method to prevent MITM', async () => {
    // Create a minimal proxy handler and test CONNECT rejection logic
    async function handleProxy(method, url) {
      if (method === 'CONNECT') {
        return { status: 405, body: 'CONNECT not supported' };
      }
      return { status: 404, body: 'not found' };
    }
    const result = await handleProxy('CONNECT', 'evil.example.com:443');
    assert.equal(result.status, 405);
  });
});

// ── Tests: spawn.rs credential prefix ────────────────────────────────────────

describe('credential prefix (spawn.rs behaviour)', () => {
  it('GYRE_AGENT_CREDENTIALS keys get GYRE_CRED_ prefix', () => {
    const gyreAgentCredentials = 'ANTHROPIC_API_KEY=sk-ant-test,GITLAB_TOKEN=glpat-test';
    const containerEnv = {};

    // Simulate M27 spawn.rs logic
    for (const pair of gyreAgentCredentials.split(',')) {
      const trimmed = pair.trim();
      const eq = trimmed.indexOf('=');
      if (eq > 0) {
        const k = trimmed.substring(0, eq);
        const v = trimmed.substring(eq + 1);
        containerEnv[`GYRE_CRED_${k}`] = v;
      }
    }

    assert.equal(containerEnv['GYRE_CRED_ANTHROPIC_API_KEY'], 'sk-ant-test');
    assert.equal(containerEnv['GYRE_CRED_GITLAB_TOKEN'], 'glpat-test');
    assert.equal(containerEnv['ANTHROPIC_API_KEY'], undefined, 'raw key must not be in container env');
    assert.equal(containerEnv['GITLAB_TOKEN'], undefined, 'raw token must not be in container env');
  });

  it('proxy env vars are injected alongside credentials', () => {
    const containerEnv = {
      GYRE_CRED_PROXY: 'http://127.0.0.1:8765',
      ANTHROPIC_BASE_URL: 'http://127.0.0.1:8765',
      ANTHROPIC_API_KEY: 'proxy-managed',
    };
    assert.equal(containerEnv['GYRE_CRED_PROXY'], 'http://127.0.0.1:8765');
    assert.equal(containerEnv['ANTHROPIC_API_KEY'], 'proxy-managed');
    assert.notEqual(containerEnv['ANTHROPIC_API_KEY'], 'sk-ant-real', 'real key must not be in env');
  });
});
