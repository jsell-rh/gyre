// Classifier tests: precedence, null bytes, repeat collapse, incident grouping,
// driver-prefix stripping, generic tool shapes.
import { test } from "node:test";
import assert from "node:assert/strict";
import { classifyLine, classifyDriverLine, stripDriverPrefix, cleanText, collapseEvents, groupIncidents, lastMeaningfulActivity } from "../server/classify.mjs";

test("cleanText strips null bytes and control chars", () => {
  assert.equal(cleanText("a\x00\x00\x00b"), "ab");
  assert.equal(cleanText("x\x07y"), "xy");
  assert.equal(cleanText(null), "");
});

test("lifecycle beats failure/action/diagnostic (precedence)", () => {
  const r = classifyLine("[15:33:09] >>> Worker round 1/6");
  assert.equal(r.kind, "lifecycle");
});

test("round start is lifecycle", () => {
  const r = classifyLine(">>> Worker round 2/6");
  assert.equal(r.kind, "lifecycle");
  assert.match(r.summary, /round 2/);
});

test("sandbox deletion is lifecycle", () => {
  const r = classifyLine("=== Sandbox gyre-wk-task-063 deleted");
  assert.equal(r.kind, "lifecycle");
  assert.match(r.summary, /deleted/);
});

test("alert markers are failures with severity", () => {
  const r = classifyLine("!!! Unresolvable conflicts in: crates/foo");
  assert.equal(r.kind, "failure");
  assert.equal(r.severity, "critical");
});

test("TLS warn line is diagnostic with stable causeKey", () => {
  const a = classifyLine("2026-10-03T03:16:21.138246Z  WARN openshell_cli::tls: TLS cert expired");
  const b = classifyLine("2026-10-03T03:16:25.000000Z  WARN openshell_cli::tls: TLS cert expired");
  assert.equal(a.kind, "diagnostic");
  assert.equal(b.kind, "diagnostic");
  assert.equal(a.causeKey, b.causeKey, "same cause collapses");
});

test("cargo commands are actions", () => {
  const r = classifyLine("$ cargo test -p gyre-domain");
  assert.equal(r.kind, "action");
  assert.match(r.summary, /cargo test/);
});

test("cargo download noise is diagnostic", () => {
  const r = classifyLine("  Downloaded ahash v0.8.12");
  assert.equal(r.kind, "diagnostic");
});

test("edit tool call is action", () => {
  const r = classifyLine("$ edit crates/server/src/lib.rs");
  assert.equal(r.kind, "action");
  assert.match(r.summary, /editing/);
});

test("error exits are failures", () => {
  const r = classifyLine("$ cargo build && echo done");
  assert.ok(r === null || r.kind !== "failure"); // plain command, no error marker
  const f = classifyLine("error: failed to download crates");
  assert.equal(f.kind, "failure");
});

// --- driver prefix stripping (real driver.log shapes) ------------------------

test("stripDriverPrefix removes time and task/sandbox tags", () => {
  const { text } = stripDriverPrefix("[23:08:44] [task-173/sandbox] !!! clone/branch failed");
  assert.equal(text, "!!! clone/branch failed");
  const { text: t2 } = stripDriverPrefix("[23:08:44] !!! clone/branch failed");
  assert.equal(t2, "!!! clone/branch failed");
});

test("stripDriverPrefix parses the clock into a timestamp", () => {
  const { time } = stripDriverPrefix("[12:34:56] message");
  const d = new Date(time);
  assert.equal(d.getHours(), 12);
  assert.equal(d.getMinutes(), 34);
  assert.equal(d.getSeconds(), 56);
});

test("timestamped driver alert classifies as failure (was unclassified)", () => {
  const r = classifyDriverLine({ taskId: "task-173", now: 0, text: "[23:08:44] [task-173/sandbox] !!! clone/branch failed" });
  assert.equal(r.kind, "failure");
  assert.equal(r.type, "alert");
  assert.match(r.summary, /clone\/branch failed/);
  assert.ok(r.time > 0, "prefix clock used as time, not collection time");
});

// --- generic tool shapes (any tool name, not a fixed list) -------------------

test("→ grep(...) invocation is an action with parsed summary", () => {
  const r = classifyLine("→ grep(path=specs pattern=Editor Split)");
  assert.equal(r.kind, "action");
  assert.match(r.summary, /searching/);
});

test("✓ grep: result with 'error:' text is diagnostic, not failure", () => {
  const r = classifyLine("✓ grep: error: failed to x");
  assert.equal(r.kind, "diagnostic");
  assert.equal(r.type, "tool-ok");
});

test("unknown tool invocation shape still classified as action", () => {
  const r = classifyLine("→ someTool(foo=1)");
  assert.equal(r.kind, "action");
  assert.equal(r.type, "tool-invocation");
});

test("collapseEvents merges adjacent same-cause events", () => {
  const mk = (i) => ({ taskId: "task-1", kind: "diagnostic", type: "log-warn", causeKey: "warn:tls", summary: "tls", time: 1000 * i, severity: null });
  const out = collapseEvents([mk(0), mk(1), mk(2), mk(100)]); // gap > window? all within 10min
  assert.equal(out.length, 1);
  assert.equal(out[0].count, 4);
  assert.equal(out[0].firstTime, 0);
  assert.equal(out[0].lastTime, 100000);
});

test("collapseEvents splits on time gap", () => {
  const mk = (t) => ({ taskId: "task-1", kind: "diagnostic", type: "log-warn", causeKey: "warn:tls", summary: "tls", time: t, severity: null });
  const out = collapseEvents([mk(0), mk(1), mk(20 * 60 * 1000), mk(20 * 60 * 1000 + 1)]);
  assert.equal(out.length, 2);
});

test("collapseEvents splits across tasks", () => {
  const mk = (taskId, t) => ({ taskId, kind: "diagnostic", type: "log-warn", causeKey: "warn:tls", summary: "tls", time: t, severity: null });
  const out = collapseEvents([mk("a", 0), mk("b", 1)]);
  assert.equal(out.length, 2);
});

test("lastMeaningfulActivity skips diagnostics", () => {
  const events = [
    { taskId: "a", kind: "diagnostic", summary: "tls", time: 1 },
    { taskId: "a", kind: "action", summary: "editing lib.rs", time: 2 },
    { taskId: "a", kind: "diagnostic", summary: "cargo", time: 3 },
  ];
  const r = lastMeaningfulActivity(events);
  assert.equal(r.summary, "editing lib.rs");
});

test("groupIncidents groups failures by task+cause", () => {
  const events = [
    { taskId: "a", kind: "failure", type: "alert", causeKey: "fail:x", summary: "conflicts", severity: "critical", time: 1 },
    { taskId: "a", kind: "failure", type: "alert", causeKey: "fail:x", summary: "conflicts", severity: "critical", time: 2 },
    { taskId: "b", kind: "failure", type: "alert", causeKey: "fail:x", summary: "conflicts", severity: "critical", time: 3 },
    { taskId: "a", kind: "action", type: "cargo", causeKey: "cargo:test", summary: "cargo test", severity: null, time: 4 },
  ];
  const groups = groupIncidents(events);
  assert.equal(groups.length, 2);
  const a = groups.find((g) => g.taskId === "a");
  assert.equal(a.count, 2);
});

test("unclassified keeps prose without inventing meaning", () => {
  const r = classifyLine("Looking at the spec, I will now implement the port trait.");
  assert.equal(r.kind, "unclassified");
});
