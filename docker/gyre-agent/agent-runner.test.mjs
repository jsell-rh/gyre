/**
 * agent-runner prompt delivery tests — agent-runtime spec §2.
 *
 * Runs the REAL agent-runner.mjs as a child process with a stubbed
 * @anthropic-ai/claude-agent-sdk that captures the prompt handed to query().
 * This guards the last hop of meta-spec prompt assembly: the server injects
 * GYRE_META_SPEC_PROMPT into the container env; the runner must prepend it to
 * the LLM prompt. A regression here means the meta-spec registry is stored and
 * attested but never shapes agent behavior.
 *
 * Run with: node --test agent-runner.test.mjs
 */

import { describe, it, before, beforeEach, after } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import http from 'node:http';
import { once } from 'node:events';
import { mkdtempSync, writeFileSync, mkdirSync, readFileSync, cpSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';

const RUNNER_SRC = join(dirname(fileURLToPath(import.meta.url)), 'agent-runner.mjs');

/** Minimal capture HTTP server: answers every request with 404 (runner treats
 * all post-query calls as best-effort). */
async function startCaptureServer() {
  const server = http.createServer((req, res) => {
    res.writeHead(404, { 'Content-Type': 'application/json' });
    res.end('{"error":"not found"}');
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const { port } = server.address();
  return { server, url: `http://127.0.0.1:${port}` };
}

/**
 * Build an isolated copy of the runner with a stub SDK that records the
 * prompt/options passed to query() into captureFile, then yields one text
 * message so the runner completes its normal post-query path.
 */
function buildStubbedRunner(captureFile) {
  const dir = mkdtempSync(join(tmpdir(), 'gyre-runner-test-'));
  cpSync(RUNNER_SRC, join(dir, 'agent-runner.mjs'));

  const pkgDir = join(dir, 'node_modules', '@anthropic-ai', 'claude-agent-sdk');
  mkdirSync(pkgDir, { recursive: true });
  writeFileSync(
    join(pkgDir, 'package.json'),
    JSON.stringify({
      name: '@anthropic-ai/claude-agent-sdk',
      version: '0.0.0-stub',
      type: 'module',
      main: 'index.mjs',
      exports: './index.mjs',
    }),
  );
  writeFileSync(
    join(pkgDir, 'index.mjs'),
    `import { writeFileSync } from 'node:fs';
export async function* query({ prompt, options }) {
  writeFileSync(
    process.env.GYRE_STUB_CAPTURE_FILE,
    JSON.stringify({ prompt, options }, null, 2),
  );
  yield { type: 'text', content: 'stub-done' };
}
`,
  );
  return dir;
}

/** Run the stubbed runner with the given env; resolve on exit. */
function runRunner(dir, extraEnv) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, ['agent-runner.mjs'], {
      cwd: dir,
      env: {
        ...process.env,
        HOME: dir, // hooks settings file writes go to the temp dir
        GYRE_STUB_CAPTURE_FILE: join(dir, 'capture.json'),
        ...extraEnv,
      },
    });
    let stderr = '';
    child.stderr.on('data', (c) => { stderr += c; });
    child.on('error', reject);
    child.on('close', (code) => resolve({ code, stderr }));
  });
}

/** Recording HTTP server: answers every request with 404 (runner treats
 * post-query calls as best-effort) while recording method + path so tests
 * can prove whether the MR-creating POST /complete was reached. */
async function startRecordingServer() {
  const requests = [];
  const server = http.createServer((req, res) => {
    requests.push({ method: req.method, path: req.url });
    res.writeHead(404, { 'Content-Type': 'application/json' });
    res.end('{"error":"not found"}');
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const { port } = server.address();
  return { server, requests, url: `http://127.0.0.1:${port}` };
}

/**
 * Build an isolated copy of the runner with a stub SDK that appends EVERY
 * query prompt (the initial task prompt and each gate-remediation
 * follow-up) as a JSON line to queries.log, then yields one text message.
 */
function buildStubbedRunnerRecordingQueries() {
  const dir = mkdtempSync(join(tmpdir(), 'gyre-runner-gates-'));
  cpSync(RUNNER_SRC, join(dir, 'agent-runner.mjs'));

  const pkgDir = join(dir, 'node_modules', '@anthropic-ai', 'claude-agent-sdk');
  mkdirSync(pkgDir, { recursive: true });
  writeFileSync(
    join(pkgDir, 'package.json'),
    JSON.stringify({
      name: '@anthropic-ai/claude-agent-sdk',
      version: '0.0.0-stub',
      type: 'module',
      main: 'index.mjs',
      exports: './index.mjs',
    }),
  );
  writeFileSync(
    join(pkgDir, 'index.mjs'),
    `import { appendFileSync } from 'node:fs';
export async function* query({ prompt }) {
  appendFileSync(process.env.GYRE_STUB_QUERIES_FILE, JSON.stringify(prompt) + '\\n');
  yield { type: 'text', content: 'stub-done' };
}
`,
  );
  writeFileSync(join(dir, 'queries.log'), '');
  return dir;
}

/** Run the stubbed recording runner with the given env; resolve on exit
 * with both stdout and stderr captured (the runner log is the gate-failure
 * feedback channel). */
function runRunnerCapturingOutput(dir, extraEnv) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, ['agent-runner.mjs'], {
      cwd: dir,
      env: {
        ...process.env,
        HOME: dir,
        GYRE_STUB_QUERIES_FILE: join(dir, 'queries.log'),
        ...extraEnv,
      },
    });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (c) => { stdout += c; });
    child.stderr.on('data', (c) => { stderr += c; });
    child.on('error', reject);
    child.on('close', (code) => resolve({ code, stdout, stderr }));
  });
}

describe('meta-spec prompt delivery (GYRE_META_SPEC_PROMPT)', () => {
  let cap;

  before(async () => {
    cap = await startCaptureServer();
  });

  after(async () => {
    cap.server.close();
  });

  it('prepends GYRE_META_SPEC_PROMPT to the prompt handed to the SDK', async () => {
    const dir = buildStubbedRunner();
    const metaPrompt = '# meta:principle — conventional-commits (scope: global, v1)\nAll commits must follow Conventional Commits.';
    const { code, stderr } = await runRunner(dir, {
      GYRE_SERVER_URL: cap.url,
      GYRE_AUTH_TOKEN: 'test-token',
      GYRE_AGENT_ID: 'agt-test-1',
      GYRE_TASK_ID: 'task-test-1',
      GYRE_BRANCH: 'feat/test',
      GYRE_REPO_ID: 'repo-1',
      GYRE_META_SPEC_PROMPT: metaPrompt,
      GYRE_META_SPEC_SET_SHA: 'deadbeef',
    });
    assert.equal(code, 0, `runner must exit 0, stderr:\n${stderr}`);

    const captured = JSON.parse(readFileSync(join(dir, 'capture.json'), 'utf8'));
    assert.ok(
      captured.prompt.startsWith(metaPrompt),
      'meta-spec prompt must LEAD the LLM prompt (injection order)',
    );
    const sep = captured.prompt.indexOf('\n---\n');
    assert.ok(sep > 0, 'meta-spec block and task prompt must be separated by ---');
    const taskPart = captured.prompt.slice(sep);
    assert.match(taskPart, /You are a Gyre autonomous agent/, 'task prompt must follow the meta-spec block');
  });

  it('omits the meta-spec block entirely when the env var is unset', async () => {
    const dir = buildStubbedRunner();
    const { code, stderr } = await runRunner(dir, {
      GYRE_SERVER_URL: cap.url,
      GYRE_AUTH_TOKEN: 'test-token',
      GYRE_AGENT_ID: 'agt-test-2',
      GYRE_TASK_ID: 'task-test-2',
      GYRE_BRANCH: 'feat/test2',
      GYRE_REPO_ID: 'repo-1',
      // no GYRE_META_SPEC_PROMPT
    });
    assert.equal(code, 0, `runner must exit 0, stderr:\n${stderr}`);

    const captured = JSON.parse(readFileSync(join(dir, 'capture.json'), 'utf8'));
    assert.ok(
      captured.prompt.startsWith('You are a Gyre autonomous agent'),
      'without meta-specs the task prompt leads unchanged',
    );
    assert.ok(!captured.prompt.includes('\n---\n'), 'no separator when no meta-specs apply');
  });

  it('honors GYRE_TASK_PROMPT as the task portion after the meta-spec block', async () => {
    const dir = buildStubbedRunner();
    const metaPrompt = '# meta:standard — test-coverage (scope: global, v1)\nCover new code.';
    const { code, stderr } = await runRunner(dir, {
      GYRE_SERVER_URL: cap.url,
      GYRE_AUTH_TOKEN: 'test-token',
      GYRE_AGENT_ID: 'agt-test-3',
      GYRE_TASK_ID: 'task-test-3',
      GYRE_BRANCH: 'feat/test3',
      GYRE_REPO_ID: 'repo-1',
      GYRE_TASK_PROMPT: 'CUSTOM TASK PROMPT BODY',
      GYRE_META_SPEC_PROMPT: metaPrompt,
    });
    assert.equal(code, 0, `runner must exit 0, stderr:\n${stderr}`);

    const captured = JSON.parse(readFileSync(join(dir, 'capture.json'), 'utf8'));
    assert.ok(captured.prompt.startsWith(metaPrompt), 'meta-spec block leads');
    assert.ok(captured.prompt.endsWith('CUSTOM TASK PROMPT BODY'), 'custom task prompt is the tail');
  });
});

// ── Protocol prompt delivery (GYRE_PROTOCOL_PROMPT) — task-268 (4) ──────────
//
// The server injects the system-managed protocol section at spawn as
// GYRE_PROTOCOL_PROMPT, but until task-268 the runner never read it — the
// protocol's "run the repo's quality gates" instruction died in an unread
// env var. These tests fail if the GYRE_PROTOCOL_PROMPT read is removed
// from the runner's prompt assembly.

describe('protocol prompt delivery (GYRE_PROTOCOL_PROMPT)', () => {
  let cap;

  before(async () => {
    cap = await startCaptureServer();
  });

  after(async () => {
    cap.server.close();
  });

  it('includes GYRE_PROTOCOL_PROMPT content in the LLM query prompt when set', async () => {
    const dir = buildStubbedRunner();
    const protocolText = '## Ralph loop rules\n3. **Act** — implement, run the repo\'s quality gates, commit, push your branch.';
    const { code, stderr } = await runRunner(dir, {
      GYRE_SERVER_URL: cap.url,
      GYRE_AUTH_TOKEN: 'test-token',
      GYRE_AGENT_ID: 'agt-proto-1',
      GYRE_TASK_ID: 'task-proto-1',
      GYRE_BRANCH: 'feat/proto1',
      GYRE_REPO_ID: 'repo-1',
      GYRE_PROTOCOL_PROMPT: protocolText,
    });
    assert.equal(code, 0, `runner must exit 0, stderr:\n${stderr}`);

    const captured = JSON.parse(readFileSync(join(dir, 'capture.json'), 'utf8'));
    assert.ok(
      captured.prompt.includes(protocolText),
      'GYRE_PROTOCOL_PROMPT content must reach the LLM query prompt:\n' + captured.prompt,
    );
  });

  it('orders the prompt meta-spec → protocol → task so behavioral instructions lead', async () => {
    const dir = buildStubbedRunner();
    const metaPrompt = '# meta:principle — conventional-commits (scope: global, v1)';
    const protocolText = '## Ralph loop rules\nAct step: run the repo\'s quality gates.';
    const { code, stderr } = await runRunner(dir, {
      GYRE_SERVER_URL: cap.url,
      GYRE_AUTH_TOKEN: 'test-token',
      GYRE_AGENT_ID: 'agt-proto-2',
      GYRE_TASK_ID: 'task-proto-2',
      GYRE_BRANCH: 'feat/proto2',
      GYRE_REPO_ID: 'repo-1',
      GYRE_META_SPEC_PROMPT: metaPrompt,
      GYRE_PROTOCOL_PROMPT: protocolText,
    });
    assert.equal(code, 0, `runner must exit 0, stderr:\n${stderr}`);

    const captured = JSON.parse(readFileSync(join(dir, 'capture.json'), 'utf8'));
    const iMeta = captured.prompt.indexOf(metaPrompt);
    const iProto = captured.prompt.indexOf(protocolText);
    const iTask = captured.prompt.indexOf('You are a Gyre autonomous agent');
    assert.ok(iMeta >= 0 && iProto > iMeta && iTask > iProto,
      `prompt must order meta-spec < protocol < task (meta=${iMeta}, proto=${iProto}, task=${iTask})`);
  });
});

// ── Pre-PR local verification (GYRE_LOCAL_GATES) — task-268 (1)–(3) ─────────
//
// agent-workflow.md §Immediate Feedback: the repo's command gates run
// locally in the agent's environment BEFORE any MR is opened. The stub SDK
// records every query prompt (the initial task prompt AND each remediation
// follow-up); the recording server records every HTTP request so the tests
// can prove POST /complete — the MR-creating call — is or is not made.

describe('pre-PR local gate verification (GYRE_LOCAL_GATES)', () => {
  let cap;

  before(async () => {
    cap = await startRecordingServer();
  });

  beforeEach(() => {
    // Each test asserts on the requests ITS OWN runner made.
    cap.requests.length = 0;
  });

  after(async () => {
    cap.server.close();
  });

  function readQueries(dir) {
    const raw = readFileSync(join(dir, 'queries.log'), 'utf8').trim();
    if (!raw) return [];
    return raw.split('\n').map((line) => JSON.parse(line));
  }

  it('failing gate: no completion call, no MR; failure output fed back to the agent', async () => {
    const dir = buildStubbedRunnerRecordingQueries();
    const gates = JSON.stringify([
      { id: 'gate-fail-1', name: 'unit tests', command: 'echo gate-failure-marker-alpha; exit 7' },
    ]);
    const { code, stdout, stderr } = await runRunnerCapturingOutput(dir, {
      GYRE_SERVER_URL: cap.url,
      GYRE_AUTH_TOKEN: 'test-token',
      GYRE_AGENT_ID: 'agt-gates-1',
      GYRE_TASK_ID: 'task-gates-1',
      GYRE_BRANCH: 'feat/gates1',
      GYRE_REPO_ID: 'repo-1',
      GYRE_LOCAL_GATES: gates,
      GYRE_LOCAL_GATE_MAX_RETRIES: '1',
    });

    // Unambiguous failure: nonzero exit, and the completion (MR-creating)
    // call was never made.
    assert.notEqual(code, 0, 'runner must exit nonzero when gates stay failing');
    assert.ok(
      cap.requests.every((r) => !r.path.includes('/complete')),
      `POST /complete must never be reached on gate failure, got: ${JSON.stringify(cap.requests)}`,
    );

    // The runner log carries the failing gate's captured output.
    assert.match(stdout, /gate-failure-marker-alpha/, 'runner log must contain the gate output');
    assert.match(stdout, /unit tests/, 'runner log must name the failing gate');

    // The remediation loop fed the failure text back to the agent in a
    // follow-up query: initial prompt + one remediation prompt.
    const queries = readQueries(dir);
    assert.equal(queries.length, 2, `expected initial + 1 remediation query, got ${queries.length}`);
    assert.match(queries[0], /You are a Gyre autonomous agent/, 'first query is the task prompt');
    assert.match(queries[1], /unit tests/, 'remediation query must name the failing gate');
    assert.match(queries[1], /gate-failure-marker-alpha/, 'remediation query must carry the captured output');
    assert.match(queries[1], /exited with code 7/, 'remediation query must carry the exit status');
  });

  it('passing gates: runner proceeds to POST /complete (MR flow unbroken)', async () => {
    const dir = buildStubbedRunnerRecordingQueries();
    const gates = JSON.stringify([
      { id: 'gate-pass-1', name: 'unit tests', command: 'echo gate-pass-ok' },
      { id: 'gate-pass-2', name: 'lint', command: 'true' },
    ]);
    const { code, stderr } = await runRunnerCapturingOutput(dir, {
      GYRE_SERVER_URL: cap.url,
      GYRE_AUTH_TOKEN: 'test-token',
      GYRE_AGENT_ID: 'agt-gates-2',
      GYRE_TASK_ID: 'task-gates-2',
      GYRE_BRANCH: 'feat/gates2',
      GYRE_REPO_ID: 'repo-1',
      GYRE_LOCAL_GATES: gates,
    });
    assert.equal(code, 0, `runner must exit 0 when all gates pass, stderr:\n${stderr}`);

    // Exactly one query (no remediation) and the completion call was made.
    assert.equal(readQueries(dir).length, 1, 'no remediation query when gates pass');
    assert.ok(
      cap.requests.some((r) => r.method === 'POST' && r.path.endsWith('/agents/agt-gates-2/complete')),
      `runner must proceed to POST /complete when gates pass, got: ${JSON.stringify(cap.requests)}`,
    );
  });

  it('no GYRE_LOCAL_GATES configured: completion proceeds directly', async () => {
    const dir = buildStubbedRunnerRecordingQueries();
    const { code, stderr } = await runRunnerCapturingOutput(dir, {
      GYRE_SERVER_URL: cap.url,
      GYRE_AUTH_TOKEN: 'test-token',
      GYRE_AGENT_ID: 'agt-gates-3',
      GYRE_TASK_ID: 'task-gates-3',
      GYRE_BRANCH: 'feat/gates3',
      GYRE_REPO_ID: 'repo-1',
      // no GYRE_LOCAL_GATES — repo configures no local gates
    });
    assert.equal(code, 0, `runner must exit 0, stderr:\n${stderr}`);
    assert.ok(
      cap.requests.some((r) => r.method === 'POST' && r.path.endsWith('/agents/agt-gates-3/complete')),
      'completion must proceed when no local gates are configured',
    );
  });

  it('a hanging gate is killed at its timeout and treated as a failure (bounded wait)', async () => {
    const dir = buildStubbedRunnerRecordingQueries();
    const gates = JSON.stringify([
      { id: 'gate-hang-1', name: 'hanging tests', command: 'sleep 60', timeout_secs: 1 },
    ]);
    const started = Date.now();
    const { code, stdout, stderr } = await runRunnerCapturingOutput(dir, {
      GYRE_SERVER_URL: cap.url,
      GYRE_AUTH_TOKEN: 'test-token',
      GYRE_AGENT_ID: 'agt-gates-4',
      GYRE_TASK_ID: 'task-gates-4',
      GYRE_BRANCH: 'feat/gates4',
      GYRE_REPO_ID: 'repo-1',
      GYRE_LOCAL_GATES: gates,
      GYRE_LOCAL_GATE_MAX_RETRIES: '0',
    });
    const elapsed = Date.now() - started;

    assert.notEqual(code, 0, 'a hung gate must fail the runner, not hang it');
    // Bounded: killed at the 1s gate timeout (+ remediation/exits), far
    // short of the 60s the command would otherwise sleep.
    assert.ok(elapsed < 30_000, `runner must not wait out a hanging gate (took ${elapsed}ms)`);
    assert.match(`${stdout}\n${stderr}`, /timed out/, 'timeout must be reported as the failure reason');
    assert.ok(
      cap.requests.every((r) => !r.path.includes('/complete')),
      'no completion call after a timed-out gate',
    );
  });

  it('malformed GYRE_LOCAL_GATES fails closed: no completion, nonzero exit', async () => {
    const dir = buildStubbedRunnerRecordingQueries();
    const { code } = await runRunnerCapturingOutput(dir, {
      GYRE_SERVER_URL: cap.url,
      GYRE_AUTH_TOKEN: 'test-token',
      GYRE_AGENT_ID: 'agt-gates-5',
      GYRE_TASK_ID: 'task-gates-5',
      GYRE_BRANCH: 'feat/gates5',
      GYRE_REPO_ID: 'repo-1',
      GYRE_LOCAL_GATES: 'not-json{',
    });
    assert.notEqual(code, 0, 'malformed gate config must fail the runner closed');
    assert.ok(
      cap.requests.every((r) => !r.path.includes('/complete')),
      'no completion call on malformed gate config',
    );
  });

  // Regression (review 4fba4dc6): heartbeats used to fire only inside the
  // SDK message loop, so a gate subprocess running longer than the
  // server's 60s stale timeout got the agent torn down mid-verification.
  // The runner must keep heartbeating while local gates execute. This test
  // fails if the gate-phase heartbeat keep-alive is removed: the stub SDK
  // query resolves in milliseconds, so every heartbeat observed below
  // fired while the 1s gate subprocess was running.
  it('keeps heartbeating while local gates execute (no stale-agent teardown mid-verification)', async () => {
    const dir = buildStubbedRunnerRecordingQueries();
    const gates = JSON.stringify([
      { id: 'gate-hb-1', name: 'slow tests', command: 'sleep 1', timeout_secs: 10 },
    ]);
    const { code, stderr } = await runRunnerCapturingOutput(dir, {
      GYRE_SERVER_URL: cap.url,
      GYRE_AUTH_TOKEN: 'test-token',
      GYRE_AGENT_ID: 'agt-gates-6',
      GYRE_TASK_ID: 'task-gates-6',
      GYRE_BRANCH: 'feat/gates6',
      GYRE_REPO_ID: 'repo-1',
      GYRE_LOCAL_GATES: gates,
      // Shrink the heartbeat interval (same code path, default 15s) so the
      // keep-alive is observable within a 1s gate run.
      GYRE_HEARTBEAT_INTERVAL_MS: '150',
    });
    assert.equal(code, 0, `slow-but-passing gate must complete normally, stderr:\n${stderr}`);

    const heartbeats = cap.requests.filter(
      (r) => r.method === 'PUT' && r.path.endsWith('/agents/agt-gates-6/heartbeat'),
    );
    assert.ok(
      heartbeats.length >= 2,
      `heartbeats must continue during gate execution (a gate longer than the server's ` +
        `stale timeout would otherwise get the agent torn down mid-verification), ` +
        `got ${heartbeats.length} heartbeat(s): ${JSON.stringify(cap.requests)}`,
    );
    // And the gate passing still proceeds to completion (MR flow unbroken).
    assert.ok(
      cap.requests.some((r) => r.method === 'POST' && r.path.endsWith('/agents/agt-gates-6/complete')),
      'heartbeating must not break the completion flow',
    );
  });
});

describe('preview mode (meta-spec reconciliation §5): draft reaches the agent, ceremony skipped', () => {
  let cap;

  before(async () => {
    cap = await startRecordingServer();
  });

  beforeEach(() => {
    // Each test asserts on the requests ITS OWN runner made.
    cap.requests.length = 0;
  });

  after(async () => {
    cap.server.close();
  });

  // The preview early-exit sits BETWEEN the SDK query loop and the pre-PR
  // local-gate phase (§5 skips gates, provenance and completion — the
  // server derives the result from the pushed branch). This test fails if
  // the exit moves after gate execution (a failing gate would exit nonzero
  // and trigger a remediation query), if the draft stops reaching the LLM
  // prompt, or if any completion/provenance call leaks into preview mode.
  it('preview run with configured gates: draft in prompt, gates never executed, no upload, no completion', async () => {
    const dir = buildStubbedRunnerRecordingQueries();
    const gateMarker = join(dir, 'preview-gate-ran-marker');
    const gates = JSON.stringify([
      // A gate that would both "fail" (nonzero exit) and leave a marker if
      // preview mode wrongly executed it.
      { id: 'gate-preview-1', name: 'unit tests', command: `touch ${gateMarker}; exit 7` },
    ]);
    const { code, stdout, stderr } = await runRunnerCapturingOutput(dir, {
      GYRE_SERVER_URL: cap.url,
      GYRE_AUTH_TOKEN: 'test-token',
      GYRE_AGENT_ID: 'agt-preview-1',
      // GYRE_PREVIEW_ID (not GYRE_TASK_ID) puts the runner in preview mode.
      GYRE_PREVIEW_ID: 'prev-206',
      GYRE_BRANCH: 'preview/prev-206/search',
      GYRE_REPO_ID: 'repo-1',
      GYRE_WORK_DIR: dir,
      GYRE_META_SPEC_DRAFT_KIND: 'meta:persona',
      GYRE_META_SPEC_DRAFT_CONTENT: 'DRAFT-PERSONA-MARKER-7f3a: always write the test before the code',
      GYRE_TARGET_SPEC_PATH: 'specs/system/search.md',
      // Gates ARE configured — preview mode must skip them anyway (§5).
      GYRE_LOCAL_GATES: gates,
    });

    assert.equal(code, 0, `preview run must exit 0 (throwaway, not a gate failure), stderr:\n${stderr}`);
    assert.match(stdout, /Preview mode complete/, 'runner must log the preview completion');

    // Exactly one query (the preview prompt) — no gate-remediation loop.
    const queries = readFileSync(join(dir, 'queries.log'), 'utf8').trim()
      .split('\n').filter(Boolean).map((l) => JSON.parse(l));
    assert.equal(queries.length, 1, `preview mode must not remediate on gate failures, got ${queries.length} queries`);
    // The draft actually reached the LLM prompt (not just the env).
    assert.match(queries[0], /DRAFT-PERSONA-MARKER-7f3a/, 'draft meta-spec content must reach the SDK prompt');
    assert.match(queries[0], /specs\/system\/search\.md/, 'target spec path must reach the SDK prompt');
    assert.match(queries[0], /preview\/prev-206\/search/, 'preview branch must reach the SDK prompt');

    // The gate was never executed.
    assert.equal(existsSync(gateMarker), false, 'preview mode must not execute local quality gates');

    // No completion call (no MR) and no provenance upload — only liveness
    // traffic (heartbeats, logs) may reach the server.
    assert.ok(
      cap.requests.every((r) => !r.path.includes('/complete')),
      `preview mode must never signal completion, got: ${JSON.stringify(cap.requests)}`,
    );
    assert.ok(
      cap.requests.every((r) => !r.path.includes('/mcp')),
      `preview mode must never upload conversation provenance, got: ${JSON.stringify(cap.requests)}`,
    );
  });
});

// ── WebSocket transport (architecture.md §API, task-310) ────────────────────
//
// "WebSocket as the primary transport for agent ↔ server communication".
// These tests run the REAL agent-runner.mjs as a child process against a
// minimal RFC 6455 WebSocket server (handshake + text frames) and prove:
//   1. the runner maintains an authenticated WS session (Auth →
//      AuthResult → Subscribe to its own workspace) and its heartbeats
//      travel over the WS, NOT the HTTP fallback;
//   2. a server-pushed agent-directed shutdown message reaches the runner
//      over the WS (no polling) and triggers the graceful stop inside the
//      grace window — no completion call, no MR;
//   3. the transport survives a transient drop (reconnect with backoff).

import { createHash } from 'node:crypto';

const WS_GUID = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11';

/** Minimal RFC 6455 server-side WebSocket: text frames both ways on one
 * connection (handshake done by the enclosing HTTP server's upgrade
 * handler). Client frames are masked and unmasked here. */
class MiniWsConnection {
  constructor(socket) {
    this.socket = socket;
    this.buffer = Buffer.alloc(0);
    this.onmessage = null;
    this.onclose = null;
    socket.on('data', (chunk) => this._feed(chunk));
    socket.on('close', () => this.onclose?.());
    socket.on('error', () => {
      socket.destroy();
    });
  }

  _feed(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    for (;;) {
      const frame = this._readFrame();
      if (!frame) return;
      if (frame.opcode === 0x1) {
        this.onmessage?.(frame.payload.toString('utf8'));
      } else if (frame.opcode === 0x8) {
        this._sendFrame(0x8, Buffer.alloc(0));
        this.socket.end();
        return;
      }
      // ping/pong/continuation frames: not needed for these tests
    }
  }

  _readFrame() {
    const buf = this.buffer;
    if (buf.length < 2) return null;
    const opcode = buf[0] & 0x0f;
    const masked = (buf[1] & 0x80) !== 0;
    let len = buf[1] & 0x7f;
    let offset = 2;
    if (len === 126) {
      if (buf.length < 4) return null;
      len = buf.readUInt16BE(2);
      offset = 4;
    } else if (len === 127) {
      if (buf.length < 10) return null;
      len = Number(buf.readBigUInt64BE(2));
      offset = 10;
    }
    const maskLen = masked ? 4 : 0;
    if (buf.length < offset + maskLen + len) return null;
    let payload = buf.subarray(offset + maskLen, offset + maskLen + len);
    if (masked) {
      const mask = buf.subarray(offset, offset + 4);
      const unmasked = Buffer.allocUnsafe(len);
      for (let i = 0; i < len; i++) unmasked[i] = payload[i] ^ mask[i & 3];
      payload = unmasked;
    }
    this.buffer = buf.subarray(offset + maskLen + len);
    return { opcode, payload };
  }

  send(text) {
    this._sendFrame(0x1, Buffer.from(text, 'utf8'));
  }

  close() {
    this._sendFrame(0x8, Buffer.alloc(0));
    this.socket.end();
  }

  _sendFrame(opcode, payload) {
    const len = payload.length;
    let header;
    if (len < 126) {
      header = Buffer.from([0x80 | opcode, len]);
    } else if (len < 65536) {
      header = Buffer.allocUnsafe(4);
      header[0] = 0x80 | opcode;
      header[1] = 126;
      header.writeUInt16BE(len, 2);
    } else {
      header = Buffer.allocUnsafe(10);
      header[0] = 0x80 | opcode;
      header[1] = 127;
      header.writeBigUInt64BE(BigInt(len), 2);
    }
    this.socket.write(Buffer.concat([header, payload]));
  }
}

/**
 * Recording server: plain HTTP requests answered with 404 (the runner
 * treats post-query calls as best-effort) PLUS a /ws upgrade endpoint
 * that speaks enough of the server protocol for the runner (Auth →
 * AuthResult, Heartbeat → HeartbeatAck) while recording every WS text
 * message and connection so tests can assert on the transport.
 */
async function startWsRecordingServer() {
  const requests = [];
  const wsMessages = []; // { connection, text }
  const connections = [];
  let authCount = 0;

  function handleProtocol(conn, text) {
    let msg;
    try {
      msg = JSON.parse(text);
    } catch {
      return;
    }
    if (msg.type === 'Auth') {
      authCount += 1;
      conn.send(JSON.stringify({ type: 'AuthResult', success: true, message: 'authenticated' }));
    } else if (msg.type === 'Heartbeat') {
      conn.send(JSON.stringify({ type: 'HeartbeatAck', success: true, message: 'heartbeat recorded' }));
    }
  }

  const server = http.createServer((req, res) => {
    requests.push({ method: req.method, path: req.url });
    res.writeHead(404, { 'Content-Type': 'application/json' });
    res.end('{"error":"not found"}');
  });
  server.on('upgrade', (req, socket, head) => {
    const key = req.headers['sec-websocket-key'];
    const accept = createHash('sha1').update(key + WS_GUID).digest('base64');
    socket.write(
      'HTTP/1.1 101 Switching Protocols\r\n' +
        'Upgrade: websocket\r\n' +
        'Connection: Upgrade\r\n' +
        `Sec-WebSocket-Accept: ${accept}\r\n\r\n`,
    );
    const conn = new MiniWsConnection(socket);
    if (head && head.length) conn._feed(head);
    connections.push(conn);
    conn.onmessage = (text) => {
      wsMessages.push({ connection: conn, text });
      handleProtocol(conn, text);
    };
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const { port } = server.address();
  return {
    server,
    url: `http://127.0.0.1:${port}`,
    requests,
    wsMessages,
    connections,
    authCount: () => authCount,
  };
}

/** Build an isolated copy of the runner with a custom stub SDK. */
function buildStubbedRunnerWithSdk(sdkSource) {
  const dir = mkdtempSync(join(tmpdir(), 'gyre-runner-ws-'));
  cpSync(RUNNER_SRC, join(dir, 'agent-runner.mjs'));
  const pkgDir = join(dir, 'node_modules', '@anthropic-ai', 'claude-agent-sdk');
  mkdirSync(pkgDir, { recursive: true });
  writeFileSync(
    join(pkgDir, 'package.json'),
    JSON.stringify({
      name: '@anthropic-ai/claude-agent-sdk',
      version: '0.0.0-stub',
      type: 'module',
      main: 'index.mjs',
      exports: './index.mjs',
    }),
  );
  writeFileSync(join(pkgDir, 'index.mjs'), sdkSource);
  writeFileSync(join(dir, 'queries.log'), '');
  return dir;
}

/** Slow stub SDK: yields one message every `ms` so the runner's message
 * loop (and its heartbeat cadence) is observable over time. */
function slowSdk(ms, count) {
  return `import { appendFileSync } from 'node:fs';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
export async function* query({ prompt, options }) {
  appendFileSync(process.env.GYRE_STUB_QUERIES_FILE, JSON.stringify(prompt) + '\\n');
  for (let i = 0; i < ${count}; i++) {
    await sleep(${ms});
    yield { type: 'text', content: 'tick ' + i };
  }
}
`;
}

/** Abort-aware stub SDK: a long-running query that ends promptly when the
 * runner aborts it (server-directed shutdown) — the same contract the real
 * SDK's options.abortController provides. */
function abortableSdk(ms, count) {
  return `import { appendFileSync } from 'node:fs';
const abortableSleep = (ms, signal) => new Promise((resolve) => {
  const t = setTimeout(resolve, ms);
  if (signal) signal.addEventListener('abort', () => { clearTimeout(t); resolve(); }, { once: true });
});
export async function* query({ prompt, options }) {
  appendFileSync(process.env.GYRE_STUB_QUERIES_FILE, JSON.stringify(prompt) + '\\n');
  const signal = options?.abortController?.signal;
  for (let i = 0; i < ${count}; i++) {
    await abortableSleep(${ms}, signal);
    if (signal?.aborted) return;
    yield { type: 'text', content: 'tick ' + i };
  }
}
`;
}

/** Poll until predicate() returns truthy (or fail with `what`). */
async function waitFor(predicate, what, timeoutMs = 5000) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const value = predicate();
    if (value) return value;
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 25));
  }
}

function parseWsMessages(cap, type) {
  return cap.wsMessages
    .map((m) => {
      try {
        return JSON.parse(m.text);
      } catch {
        return null;
      }
    })
    .filter((m) => m && m.type === type);
}

describe('WebSocket primary transport (architecture.md §API)', () => {
  it('maintains an authenticated WS session and heartbeats ride the WS, not the HTTP fallback', async () => {
    const cap = await startWsRecordingServer();
    const dir = buildStubbedRunnerWithSdk(slowSdk(200, 8)); // ~1.6s of query time

    const { code, stderr } = await runRunnerCapturingOutput(dir, {
      GYRE_SERVER_URL: cap.url,
      GYRE_AUTH_TOKEN: 'ws-agent-token',
      GYRE_AGENT_ID: 'agt-ws-hb',
      GYRE_TASK_ID: 'task-ws-hb',
      GYRE_BRANCH: 'feat/ws-hb',
      GYRE_REPO_ID: 'repo-1',
      GYRE_WORKSPACE_ID: 'ws-42',
      GYRE_SVID_PEM: 'test-svid-pem',
      GYRE_STACK_HASH: 'test-stack-fingerprint',
      GYRE_HEARTBEAT_INTERVAL_MS: '100',
    });

    assert.equal(code, 0, `runner must exit 0, stderr:\n${stderr}`);

    // Authenticated session: Auth (answered by AuthResult) then Subscribe
    // to the agent's OWN workspace with a replay bound.
    const auths = parseWsMessages(cap, 'Auth');
    assert.equal(auths.length, 1, 'exactly one Auth per connection');
    assert.equal(auths[0].token, 'ws-agent-token', 'Auth must carry the agent token');
    const subs = parseWsMessages(cap, 'Subscribe');
    assert.equal(subs.length, 1, 'Subscribe must follow successful auth');
    assert.deepEqual(
      subs[0].scopes.map((s) => s.workspace_id),
      ['ws-42'],
      "Subscribe must scope to the agent's own workspace (GYRE_WORKSPACE_ID)",
    );
    assert.ok(
      typeof subs[0].last_seen === 'number' && subs[0].last_seen > 0,
      'Subscribe must bound replay with last_seen (no stale-event re-fire)',
    );

    // Heartbeats traveled over the WS as the PRIMARY channel: at least two
    // WS heartbeats carrying the session's workload-identity proof...
    const wsHeartbeats = parseWsMessages(cap, 'Heartbeat');
    assert.ok(
      wsHeartbeats.length >= 2,
      `heartbeats must ride the WS transport, got ${wsHeartbeats.length}: ` +
        JSON.stringify(cap.wsMessages.map((m) => m.text.slice(0, 80))),
    );
    for (const hb of wsHeartbeats) {
      assert.equal(hb.svid, 'test-svid-pem');
      assert.equal(hb.stack_fingerprint, 'test-stack-fingerprint');
    }
    // ...and the HTTP fallback heartbeat was NEVER used while connected.
    assert.ok(
      cap.requests.every((r) => !(r.method === 'PUT' && r.path.includes('/heartbeat'))),
      `no HTTP heartbeat fallback while the WS transport is up, got: ${JSON.stringify(cap.requests)}`,
    );

    // The normal flow is unbroken: completion still happens.
    assert.ok(
      cap.requests.some((r) => r.method === 'POST' && r.path.endsWith('/agents/agt-ws-hb/complete')),
      'WS transport must not break the completion flow',
    );

    cap.server.close();
  });

  it('receives a server-pushed shutdown over the WS (no polling) and stops gracefully inside the window', async () => {
    const cap = await startWsRecordingServer();
    // Long abortable query (~10s if never aborted) — the shutdown must cut
    // it short.
    const dir = buildStubbedRunnerWithSdk(abortableSdk(250, 40));

    const runPromise = runRunnerCapturingOutput(dir, {
      GYRE_SERVER_URL: cap.url,
      GYRE_AUTH_TOKEN: 'ws-agent-token',
      GYRE_AGENT_ID: 'agt-ws-shutdown',
      GYRE_TASK_ID: 'task-ws-shutdown',
      GYRE_BRANCH: 'feat/ws-shutdown',
      GYRE_REPO_ID: 'repo-1',
      GYRE_WORKSPACE_ID: 'ws-42',
      GYRE_SVID_PEM: 'test-svid-pem',
      GYRE_STACK_HASH: 'test-stack-fingerprint',
      GYRE_HEARTBEAT_INTERVAL_MS: '100',
    });

    // Wait until the runner is subscribed, then push the exact
    // spec-rejection shutdown message shape (api/specs.rs reject_spec →
    // MessageKind::StatusUpdate, Destination::Agent) at its live session.
    const subEntry = await waitFor(
      () =>
        cap.wsMessages.find((m) => {
          try {
            return JSON.parse(m.text).type === 'Subscribe';
          } catch {
            return false;
          }
        }),
      'runner Subscribe',
    );
    subEntry.connection.send(
      JSON.stringify({
        id: 'msg-shutdown-1',
        tenant_id: 't1',
        from: 'server',
        workspace_id: 'ws-42',
        to: { agent: 'agt-ws-shutdown' },
        kind: 'status_update',
        payload: {
          action: 'shutdown',
          reason: 'spec rejected: test',
          spec_path: 'specs/system/test.md',
          grace_period_secs: 60,
        },
        created_at: Date.now(),
        signature: null,
        key_id: null,
        acknowledged: false,
      }),
    );

    const started = Date.now();
    const { code, stdout, stderr } = await runPromise;
    const elapsed = Date.now() - started;

    // Graceful stop inside the grace window — far short of both the 60s
    // grace period and the 10s the query would otherwise run.
    assert.equal(code, 0, `shutdown stop must exit 0, stderr:\n${stderr}`);
    assert.ok(elapsed < 8000, `graceful stop must be prompt (took ${elapsed}ms)`);
    assert.match(
      `${stdout}\n${stderr}`,
      /shutdown signal received: spec rejected: test/,
      'the shutdown reason must be logged',
    );

    // Graceful stop, NOT completion: the task was cancelled server-side,
    // so the MR-creating call must never happen.
    assert.ok(
      cap.requests.every((r) => !r.path.includes('/complete')),
      `no completion call after a server-directed shutdown, got: ${JSON.stringify(cap.requests)}`,
    );
    // Exactly one LLM query (the task prompt) — no remediation loop.
    const queries = readFileSync(join(dir, 'queries.log'), 'utf8').trim()
      .split('\n').filter(Boolean);
    assert.equal(queries.length, 1, 'shutdown must end the query loop, not remediate');

    cap.server.close();
  });

  it('reconnects with backoff after a transient drop and keeps the session alive', async () => {
    const cap = await startWsRecordingServer();
    const dir = buildStubbedRunnerWithSdk(slowSdk(200, 8));

    const runPromise = runRunnerCapturingOutput(dir, {
      GYRE_SERVER_URL: cap.url,
      GYRE_AUTH_TOKEN: 'ws-agent-token',
      GYRE_AGENT_ID: 'agt-ws-reconnect',
      GYRE_TASK_ID: 'task-ws-reconnect',
      GYRE_BRANCH: 'feat/ws-reconnect',
      GYRE_REPO_ID: 'repo-1',
      GYRE_WORKSPACE_ID: 'ws-42',
      GYRE_SVID_PEM: 'test-svid-pem',
      GYRE_STACK_HASH: 'test-stack-fingerprint',
      GYRE_HEARTBEAT_INTERVAL_MS: '100',
      // Fast reconnect for the test (same code path, default 500ms).
      GYRE_WS_RECONNECT_BASE_MS: '50',
    });

    // Drop the first connection once the runner is subscribed — a
    // transient transport failure.
    const firstSub = await waitFor(
      () =>
        cap.wsMessages.find((m) => {
          try {
            return JSON.parse(m.text).type === 'Subscribe';
          } catch {
            return false;
          }
        }),
      'first Subscribe',
    );
    firstSub.connection.close();

    const { code, stderr } = await runPromise;
    assert.equal(code, 0, `runner must survive the drop and complete, stderr:\n${stderr}`);

    // The transport re-established itself: a second Auth on a NEW
    // connection (backoff reconnect), not just the dropped one.
    const authConnections = new Set(
      cap.wsMessages
        .filter((m) => {
          try {
            return JSON.parse(m.text).type === 'Auth';
          } catch {
            return false;
          }
        })
        .map((m) => m.connection),
    );
    assert.ok(
      authConnections.size >= 2,
      `runner must reconnect after a drop (auths on ${authConnections.size} connection(s))`,
    );

    // And the run still completes normally (MR flow unbroken).
    assert.ok(
      cap.requests.some((r) => r.method === 'POST' && r.path.endsWith('/agents/agt-ws-reconnect/complete')),
      'reconnect must not break the completion flow',
    );

    cap.server.close();
  });
});
