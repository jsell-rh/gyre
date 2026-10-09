import { test } from "node:test";
import assert from "node:assert/strict";
import { appendFile, mkdtemp, mkdir, copyFile, cp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { associatePullRequests, controllerPaths, collectController, attemptLog, setSlots, coverageHistory, coverageMetrics, taskTitles } from "../server/dev-controller.mjs";
import { createDashboardServer } from "../server/dev-http.mjs";

const execFileP = promisify(execFile);

test("PRs attach from task branch, title, or explicit body reference", () => {
  const base = { state: "OPEN", updatedAt: "2026-10-05T00:00:00Z" };
  const links = associatePullRequests([
    { ...base, number: 631, title: "sandbox worker", headRefName: "worker/task-082", url: "https://github.com/jsell-rh/gyre/pull/631" },
    { ...base, number: 632, title: "Fix TASK-099", headRefName: "other", url: "https://github.com/jsell-rh/gyre/pull/632" },
    { ...base, number: 633, title: "Bootstrap", headRefName: "other", body: "Closes task-099", url: "https://github.com/jsell-rh/gyre/pull/633" },
    { ...base, number: 634, title: "No association", body: "Related to task-099", url: "https://github.com/jsell-rh/gyre/pull/634" },
  ]);
  assert.deepEqual(links["task-082"].map((pr) => pr.number), [631]);
  assert.deepEqual(links["task-099"].map((pr) => pr.number), [632, 633]);
});

test("coverage follows the controller's fetched main instead of a stale checkout", async () => {
  const root = await mkdtemp(join(tmpdir(), "gyre-coverage-"));
  const local = join(root, "specs", "coverage", "system");
  const source = join(root, ".gyre-pipeline", "source");
  await mkdir(local, { recursive: true });
  await mkdir(join(root, "specs", "tasks"), { recursive: true });
  await writeFile(join(local, "example.md"), "| 1 | section | evidence | verified | note |\n");
  await writeFile(join(root, "specs", "tasks", "task-001.md"), "---\ntitle: Stale title\n---\n");
  await mkdir(join(source, "specs", "coverage", "system"), { recursive: true });
  await mkdir(join(source, "specs", "tasks"), { recursive: true });
  await writeFile(join(source, "specs", "coverage", "system", "example.md"), "| 1 | section | evidence | task-assigned | note |\n");
  await writeFile(join(source, "specs", "tasks", "task-001.md"), "---\ntitle: Remote title\nspec_ref: example.md\n---\n");
  await writeFile(join(source, "specs", "coverage", "SUMMARY.md"), "| **TOTAL** | **1** | **0** | **0** | **1** | **0** | **0** | **0%** |\n");
  for (const args of [["init", "-q"], ["-c", "user.name=Test", "-c", "user.email=test@example.com", "add", "."]])
    await execFileP("git", args, { cwd: source });
  await execFileP("git", ["-c", "user.name=Test", "-c", "user.email=test@example.com", "commit", "-qm", "coverage"], { cwd: source });
  await execFileP("git", ["update-ref", "refs/remotes/origin/main", "HEAD"], { cwd: source });
  const metrics = await coverageMetrics(root);
  assert.equal(metrics.assigned, 1);
  assert.equal(metrics.verified, 0);
  assert.equal((await taskTitles(root))["task-001"].title, "Remote title");
  const history = await coverageHistory(root, metrics);
  assert.equal(history[0].pct, 0);
});

test("cockpit reads pipeline stages and updates the durable sandbox budget", async (t) => {
  const root = await mkdtemp(join(tmpdir(), "gyre-cockpit-"));
  await mkdir(join(root, "scripts"));
  await mkdir(join(root, "specs", "tasks"), { recursive: true });
  for (const name of ["dev-pipeline.py", "dev-contract.py", "dev-gateway-job.py"])
    await copyFile(resolve("scripts", name), join(root, "scripts", name));
  await cp(resolve("scripts/pipeline"), join(root, "scripts/pipeline"), { recursive: true });
  await execFileP("python3", [join(root, "scripts", "dev-pipeline.py"), "status", "--json"], { cwd: root });
  const paths = controllerPaths(root);
  const state = await collectController(paths);
  assert.equal(state.present, true);
  assert.equal(state.online, false);
  assert.deepEqual(state.tasks, []);
  assert.deepEqual(Object.keys(state.stages), ["triage", "implement", "review", "verify", "publish", "cleanup"]);

  await setSlots(paths, 7);
  assert.equal((await collectController(paths)).slots, 7);
  await assert.rejects(setSlots(paths, 1001), /slots/);

  const id = "abcdef0123456789abcdef0123456789-1";
  const log = join(paths.attempts, id.slice(0, 32), "1", "output.log");
  await mkdir(join(paths.attempts, id.slice(0, 32), "1"), { recursive: true });
  await writeFile(log, "attempt output\n");
  assert.equal(await attemptLog(paths, id), "attempt output\n");
  await assert.rejects(attemptLog(paths, "../../etc/passwd"), /invalid attempt/);

  const server = createDashboardServer({ root });
  t.after(() => server.close());
  await server.refreshSnapshot();
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const base = `http://127.0.0.1:${server.address().port}`;
  const snap = await (await fetch(base + "/api/snapshot")).json();
  assert.equal(snap.slots, 7);
  const changed = await fetch(base + "/api/slots", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ value: 0 }) });
  assert.equal(changed.status, 200);
  assert.equal((await collectController(paths)).slots, 0);
  const bad = await fetch(base + "/api/slots", { method: "POST", headers: { "content-type": "application/json" }, body: "{}" });
  assert.equal(bad.status, 400);
  const retryAll = await (await fetch(base + "/api/retry-all", { method: "POST" })).json();
  assert.deepEqual(retryAll, { ok: true, retried: 0 });
  const invalidStream = await fetch(base + "/api/attempt-stream?attempt=../../etc/passwd");
  assert.equal(invalidStream.status, 400);
  const abort = new AbortController();
  const stream = await fetch(base + `/api/attempt-stream?attempt=${id}`, { signal: abort.signal });
  assert.match(stream.headers.get("content-type"), /text\/event-stream/);
  const reader = stream.body.getReader();
  await reader.read(); // connection comment
  await appendFile(log, 'GYRE_AGENT_EVENT {"role":"implement","type":"text","text":"live"}\n');
  const next = await Promise.race([reader.read(), new Promise((_, reject) => setTimeout(() => reject(new Error("stream did not advance")), 3000))]);
  assert.match(new TextDecoder().decode(next.value), /"text":"live"/);
  abort.abort();
});
