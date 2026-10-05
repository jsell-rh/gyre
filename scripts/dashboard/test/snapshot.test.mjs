// Snapshot builder tests: dimension independence, age-out, ordering,
// local-worker compat, workflow mapping, unknown runtime on probe failure.
import { test } from "node:test";
import assert from "node:assert/strict";
import { buildSnapshot } from "../server/snapshot.mjs";

const baseSbx = (name, over = {}) => ({
  name, title: `Title ${name}`, specRef: "system/x.md", progress: "in-progress",
  sbxState: "round-1", round: 1, roundsTotal: 6, sandbox: `gyre-wk-${name}`,
  extra: "", statusUpdated: new Date().toISOString(),
  agentLog: "", workerLog: "", driverAgeMs: 1000, statusAgeMs: 1000,
  driverAlive: true, ...over,
});

test("runtime and result are independent dimensions", () => {
  const snap = buildSnapshot({
    sbxWorkers: [baseSbx("task-1", { progress: "needs-revision" })],
    taskMeta: [{ name: "task-1", title: "Title task-1", deps: [], progress: "needs-revision" }],
  });
  const row = snap.rows[0];
  assert.equal(row.runtime, "active");
  assert.equal(row.result, "needs-revision"); // both at once, not conflated
});

test("dead driver reads inactive, not active", () => {
  const snap = buildSnapshot({
    sbxWorkers: [baseSbx("task-2", { driverAlive: false })],
    taskMeta: [],
  });
  assert.equal(snap.rows[0].runtime, "inactive");
});

test("failed liveness probe reads unknown, not inactive", () => {
  const snap = buildSnapshot({
    sbxWorkers: [baseSbx("task-probe", { driverAlive: null })],
    taskMeta: [],
  });
  assert.equal(snap.rows[0].runtime, "unknown");
});

test("needs-revision never ages out of overview", () => {
  const old = Date.now() - 48 * 60 * 60 * 1000;
  const snap = buildSnapshot({
    sbxWorkers: [baseSbx("task-3", { driverAlive: false, progress: "needs-revision" })],
    taskMeta: [{ name: "task-3", title: "t", deps: [], progress: "needs-revision" }],
  }, Date.now());
  assert.ok(snap.overview.some((r) => r.id === "task-3"));
});

test("complete tasks with no recent activity age out of overview", () => {
  const snap = buildSnapshot({
    sbxWorkers: [baseSbx("task-4", { driverAlive: false, progress: "complete", statusUpdated: "2026-10-01T00:00:00Z" })],
    taskMeta: [{ name: "task-4", title: "t", deps: [], progress: "complete" }],
  });
  assert.ok(!snap.overview.some((r) => r.id === "task-4"), "aged out");
  assert.ok(snap.rows.some((r) => r.id === "task-4"), "still in Tasks table");
});

test("attention alone does not keep a dead old task in overview", () => {
  // 24h-old warning on a dead task must age out (only needs-revision exempts).
  const now = Date.now();
  const snap = buildSnapshot({
    sbxWorkers: [baseSbx("task-old", {
      driverAlive: false, progress: "complete",
      // stale last activity pushes age beyond the window
    })],
    taskMeta: [{ name: "task-old", title: "t", deps: [], progress: "complete" }],
    activity: new Map([["task-old", [
      { taskId: "task-old", kind: "failure", type: "alert", causeKey: "fail:x", summary: "conflicts", severity: "critical", time: now - 48 * 60 * 60 * 1000 },
    ]]]),
  }, now);
  const row = snap.rows[0];
  assert.equal(row.attention, "critical");
  assert.ok(!snap.overview.some((r) => r.id === "task-old"), "aged out despite warning");
});

test("active tasks rank above inactive in overview order", () => {
  const snap = buildSnapshot({
    sbxWorkers: [baseSbx("task-5", { driverAlive: false }), baseSbx("task-6", { driverAlive: true })],
    taskMeta: [],
  });
  assert.equal(snap.overview[0].id, "task-6");
});

test("queue tasks (no worker) appear as inactive rows", () => {
  const snap = buildSnapshot({
    sbxWorkers: [],
    taskMeta: [
      { name: "task-7", title: "queued", deps: [], progress: "not-started" },
      { name: "task-8", title: "queued2", deps: [], progress: "complete" },
    ],
  });
  assert.equal(snap.rows.length, 2);
  assert.equal(snap.rows[0].runtime, "inactive");
  assert.equal(snap.queue["not-started"], 1);
});

test("health aggregates", () => {
  const snap = buildSnapshot({
    sbxWorkers: [baseSbx("task-9"), baseSbx("task-10", { driverAlive: false })],
    taskMeta: [],
    fleet: { parallelism: 4, active: 1 },
    diskFree: 38.5, loopAlive: false, wipGuarded: false,
  });
  assert.equal(snap.health.workersActive, 1);
  assert.equal(snap.health.workersKnown, 2);
  assert.equal(snap.health.fleet.parallelism, 4);
});

test("local workers build rows without throwing (meta lookup fixed)", () => {
  // Regression: the local-worker branch referenced an undefined `meta`;
  // any local worker threw and froze the snapshot.
  const snap = buildSnapshot({
    sbxWorkers: [],
    localWorkers: [{
      name: "task-loc", title: "", specRef: "", progress: "in-progress",
      done: false, agentLog: "", workerLog: "",
      agentAgeMs: 5000,
    }],
    taskMeta: [{ name: "task-loc", title: "Local Task", deps: [], progress: "in-progress" }],
  });
  assert.equal(snap.rows.length, 1);
  assert.equal(snap.rows[0].title, "Local Task", "title from taskMeta fallback");
  assert.equal(snap.rows[0].runtime, "active");
});

test("local worker deduped when the fleet also tracks it", () => {
  const snap = buildSnapshot({
    sbxWorkers: [baseSbx("task-dup")],
    localWorkers: [{
      name: "task-dup", title: "", specRef: "", progress: "in-progress",
      done: false, agentLog: "", workerLog: "", agentAgeMs: 1000,
    }],
    taskMeta: [{ name: "task-dup", title: "T", deps: [], progress: "in-progress" }],
  });
  assert.equal(snap.rows.filter((r) => r.id === "task-dup").length, 1);
});

test("workflow maps review/audit/merge states", () => {
  const snap = buildSnapshot({
    sbxWorkers: [
      baseSbx("task-w1", { sbxState: "reviewing" }),
      baseSbx("task-w2", { sbxState: "auditing" }),
      baseSbx("task-w3", { sbxState: "ready-for-merge" }),
      baseSbx("task-w4", { sbxState: "round-2" }),
    ],
    taskMeta: [],
  });
  const by = (id) => snap.rows.find((r) => r.id === id).workflow;
  assert.equal(by("task-w1"), "review");
  assert.equal(by("task-w2"), "audit");
  assert.equal(by("task-w3"), "merge");
  assert.equal(by("task-w4"), "implementation");
});

test("task-assigned progress maps to in-progress", () => {
  const snap = buildSnapshot({
    sbxWorkers: [],
    taskMeta: [{ name: "task-ta", title: "t", deps: [], progress: "task-assigned" }],
  });
  assert.equal(snap.rows[0].result, "in-progress");
});
