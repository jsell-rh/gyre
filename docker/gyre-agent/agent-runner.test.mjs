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

import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import http from 'node:http';
import { once } from 'node:events';
import { mkdtempSync, writeFileSync, mkdirSync, readFileSync, cpSync } from 'node:fs';
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
