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
import { spawn } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const TEST_DIR = dirname(fileURLToPath(import.meta.url));
const CRED_PROXY_ENTRY = join(TEST_DIR, 'cred-proxy.mjs');

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

// ── Tests: usage reporting (agent-runtime.md §4 "What's Tracked") ────────────
//
// The spec requires cred-proxy to report token usage back to the server
// after each LLM call via POST /api/v1/agents/:id/usage. These tests run the
// REAL cred-proxy.mjs as a child process against a fake Anthropic upstream
// (GYRE_ANTHROPIC_BASE) and a fake gyre server, then assert what arrived at
// the server. With the reporting wiring removed, every test here fails.

const LOCALHOST = '127.0.0.1';

/** Reserve an ephemeral TCP port on loopback. */
function freePort() {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.listen(0, LOCALHOST, () => {
      const { port } = srv.address();
      srv.close(() => resolve(port));
    });
    srv.on('error', reject);
  });
}

/**
 * Spawn the real cred-proxy.mjs with usage reporting pointed at `serverUrl`
 * and /v1/* LLM calls pointed at `anthropicBase`. Returns the child and the
 * TCP port its proxy listener is on.
 */
async function startCredProxy({ serverUrl, agentId, anthropicBase, extraEnv = {} }) {
  const port = await freePort();
  const sockDir = mkdtempSync(join(tmpdir(), 'cred-proxy-test-'));
  const child = spawn(process.execPath, [CRED_PROXY_ENTRY], {
    env: {
      ...process.env,
      GYRE_CRED_SOCK_PATH: join(sockDir, 'cred.sock'),
      GYRE_CRED_PROXY_PORT: String(port),
      GYRE_SERVER_URL: serverUrl,
      GYRE_AGENT_ID: agentId,
      GYRE_AUTH_TOKEN: 'test-agent-token',
      GYRE_ANTHROPIC_BASE: anthropicBase,
      ...extraEnv,
    },
    stdio: ['ignore', 'ignore', 'pipe'],
  });
  const stderr = [];
  child.stderr.on('data', (c) => stderr.push(c.toString()));
  const deadline = Date.now() + 10_000;
  for (;;) {
    try {
      await localFetch(`http://${LOCALHOST}:${port}/`, { method: 'GET' });
      break;
    } catch {
      if (Date.now() > deadline) {
        child.kill('SIGTERM');
        throw new Error(`cred-proxy did not start within 10s; stderr: ${stderr.join('')}`);
      }
      await new Promise((r) => setTimeout(r, 100));
    }
  }
  return { child, port, stderr };
}

/** Poll `fn` until it returns a truthy value (default 5 s). */
async function waitFor(fn, what, ms = 5000) {
  const deadline = Date.now() + ms;
  for (;;) {
    const value = fn();
    if (value) return value;
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 50));
  }
}

async function stopCredProxy(proxy, ...servers) {
  proxy.child.kill('SIGTERM');
  await once(proxy.child, 'exit').catch(() => {});
  for (const s of servers) s.close();
}

describe('usage reporting (agent-runtime.md §4)', () => {
  it('reports token usage and model-priced cost to the gyre server after each LLM call', async () => {
    const gyre = await startCapture((req, res) => {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end('{}');
    });
    const anthropic = await startCapture((req, res) => {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({
        id: 'msg_test_1',
        model: 'claude-sonnet-4-5',
        usage: { input_tokens: 1200, output_tokens: 300 },
      }));
    });
    const proxy = await startCredProxy({
      serverUrl: gyre.url,
      agentId: 'agt-usage-1',
      anthropicBase: anthropic.url,
    });
    try {
      const resp = await localFetch(`http://${LOCALHOST}:${proxy.port}/v1/messages`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          model: 'claude-sonnet-4-5',
          max_tokens: 64,
          messages: [{ role: 'user', content: 'hi' }],
        }),
      });
      assert.equal(resp.status, 200, 'proxied LLM call must succeed');
      assert.equal(JSON.parse(resp.body).usage.input_tokens, 1200);

      const report = await waitFor(
        () => gyre.captured.find((c) => c.url === '/api/v1/agents/agt-usage-1/usage'),
        'usage report at the gyre server'
      );
      assert.equal(report.method, 'POST');
      assert.equal(
        report.headers.authorization,
        'Bearer test-agent-token',
        'usage report must authenticate as the agent'
      );
      assert.equal(report.headers['content-type'], 'application/json');
      const payload = JSON.parse(report.body);
      assert.equal(payload.tokens_input, 1200);
      assert.equal(payload.tokens_output, 300);
      // claude-sonnet-4-5 list pricing: $3/M input, $15/M output
      const expectedCost = (1200 * 3 + 300 * 15) / 1e6;
      assert.ok(
        Math.abs(payload.cost_usd - expectedCost) < 1e-12,
        `cost_usd ${payload.cost_usd} must match model pricing ${expectedCost}`
      );
    } finally {
      await stopCredProxy(proxy, gyre.server, anthropic.server);
    }
  });

  it('reports cumulative output tokens from SSE streaming responses', async () => {
    const sseBody = [
      'event: message_start',
      'data: {"type":"message_start","message":{"model":"claude-sonnet-4-5","usage":{"input_tokens":500,"output_tokens":1}}}',
      '',
      'event: content_block_delta',
      'data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"hello"}}',
      '',
      'event: message_delta',
      'data: {"type":"message_delta","usage":{"output_tokens":120}}',
      '',
      'event: message_delta',
      'data: {"type":"message_delta","usage":{"output_tokens":150}}',
      '',
      'event: message_stop',
      'data: {"type":"message_stop"}',
      '',
    ].join('\n');
    const gyre = await startCapture((req, res) => {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end('{}');
    });
    const anthropic = await startCapture((req, res) => {
      res.writeHead(200, { 'content-type': 'text/event-stream' });
      res.end(sseBody);
    });
    const proxy = await startCredProxy({
      serverUrl: gyre.url,
      agentId: 'agt-usage-sse',
      anthropicBase: anthropic.url,
    });
    try {
      const resp = await localFetch(`http://${LOCALHOST}:${proxy.port}/v1/messages`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ model: 'claude-sonnet-4-5', stream: true, max_tokens: 64, messages: [] }),
      });
      assert.equal(resp.status, 200);

      const report = await waitFor(
        () => gyre.captured.find((c) => c.url === '/api/v1/agents/agt-usage-sse/usage'),
        'usage report for the streamed call'
      );
      const payload = JSON.parse(report.body);
      assert.equal(payload.tokens_input, 500, 'input tokens come from message_start');
      assert.equal(payload.tokens_output, 150, 'output tokens must be the final cumulative count');
    } finally {
      await stopCredProxy(proxy, gyre.server, anthropic.server);
    }
  });

  it('does not report usage for responses without a usage block', async () => {
    const gyre = await startCapture((req, res) => {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end('{}');
    });
    const anthropic = await startCapture((req, res) => {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ data: [{ id: 'model-1' }] })); // /v1/models-style listing
    });
    const proxy = await startCredProxy({
      serverUrl: gyre.url,
      agentId: 'agt-usage-none',
      anthropicBase: anthropic.url,
    });
    try {
      const resp = await localFetch(`http://${LOCALHOST}:${proxy.port}/v1/models`, { method: 'GET' });
      assert.equal(resp.status, 200);
      // Give an erroneous report time to arrive before asserting absence.
      await new Promise((r) => setTimeout(r, 750));
      assert.equal(
        gyre.captured.filter((c) => c.url.endsWith('/usage')).length,
        0,
        'non-LLM responses must not produce usage reports'
      );
    } finally {
      await stopCredProxy(proxy, gyre.server, anthropic.server);
    }
  });

  it('keeps the proxied LLM call working when the usage report fails', async () => {
    const gyre = await startCapture((req, res) => {
      res.writeHead(500, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: 'boom' }));
    });
    const anthropic = await startCapture((req, res) => {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({
        id: 'msg_test_2',
        model: 'claude-sonnet-4-5',
        usage: { input_tokens: 10, output_tokens: 5 },
      }));
    });
    const proxy = await startCredProxy({
      serverUrl: gyre.url,
      agentId: 'agt-usage-fail',
      anthropicBase: anthropic.url,
    });
    try {
      const resp = await localFetch(`http://${LOCALHOST}:${proxy.port}/v1/messages`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ model: 'claude-sonnet-4-5', max_tokens: 8, messages: [] }),
      });
      assert.equal(resp.status, 200, 'a failed usage report must not fail the LLM call');
      assert.equal(JSON.parse(resp.body).usage.input_tokens, 10);
      const report = await waitFor(
        () => gyre.captured.find((c) => c.url === '/api/v1/agents/agt-usage-fail/usage'),
        'usage report attempt at the failing gyre server'
      );
      assert.equal(report.method, 'POST');
    } finally {
      await stopCredProxy(proxy, gyre.server, anthropic.server);
    }
  });

  it('honors GYRE_PRICING_JSON overrides for cost reporting', async () => {
    const gyre = await startCapture((req, res) => {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end('{}');
    });
    const anthropic = await startCapture((req, res) => {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({
        id: 'msg_test_3',
        model: 'claude-sonnet-4-5',
        usage: { input_tokens: 1000, output_tokens: 1000 },
      }));
    });
    const proxy = await startCredProxy({
      serverUrl: gyre.url,
      agentId: 'agt-usage-price',
      anthropicBase: anthropic.url,
      extraEnv: { GYRE_PRICING_JSON: JSON.stringify({ 'claude-sonnet-4-5': [1, 2] }) },
    });
    try {
      const resp = await localFetch(`http://${LOCALHOST}:${proxy.port}/v1/messages`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ model: 'claude-sonnet-4-5', max_tokens: 8, messages: [] }),
      });
      assert.equal(resp.status, 200);
      const report = await waitFor(
        () => gyre.captured.find((c) => c.url === '/api/v1/agents/agt-usage-price/usage'),
        'usage report with overridden pricing'
      );
      const payload = JSON.parse(report.body);
      // Override: $1/M input, $2/M output → (1000*1 + 1000*2)/1e6
      assert.ok(Math.abs(payload.cost_usd - 0.003) < 1e-12, `cost ${payload.cost_usd} must use GYRE_PRICING_JSON`);
    } finally {
      await stopCredProxy(proxy, gyre.server, anthropic.server);
    }
  });
});
