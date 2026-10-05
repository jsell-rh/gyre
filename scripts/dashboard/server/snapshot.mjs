// Snapshot builder: shape collected data into the four independent state
// dimensions (runtime / workflow / result / attention) + view-ready records.
// Pure — no filesystem calls; everything arrives as inputs.
import { collapseEvents, lastMeaningfulActivity, groupIncidents } from "./classify.mjs";

const STALL_MS = 15 * 60 * 1000;     // no heartbeat for 15 min = stalled (long builds are legit)
const AGE_OUT_MS = 24 * 60 * 60 * 1000; // dead tasks leave Overview after 24h

export function buildSnapshot(input, now = Date.now()) {
  const {
    sbxWorkers = [], localWorkers = [], taskMeta = [], fleet = {},
    coverage = null, coveragePrecise = null, coverageTrend = [],
    diskFree = null, loopAlive = false, wipGuarded = false,
    orchestratorLog = "", activity = new Map(), // Map or ring ({get, tasks})
  } = input;
  const activityMap = activity instanceof Map ? activity : ringAsMap(activity);

  const metaByName = new Map(taskMeta.map((t) => [t.name, t]));
  const rows = [];

  // --- sandbox fleet tasks --------------------------------------------------
  for (const w of sbxWorkers) {
    const meta = metaByName.get(w.name) || {};
    const events = activityMap.get(w.name) || [];
    const lastAct = lastMeaningfulActivity(events);
    // Liveness: driverAlive (pgrep) is authoritative for "a worker exists".
    // driverAlive === null means the liveness probe itself failed — report
    // unknown rather than guessing. Stall detection uses the driver's
    // heartbeat (driver.log OR stream.log OR status.json — whichever wrote
    // most recently); long cargo builds legitimately pause streams for tens
    // of minutes, so stall threshold is generous (15 min).
    const heartbeatMs = Math.min(
      ...[w.driverAgeMs, w.statusAgeMs, w.streamAgeMs].filter((x) => x != null));
    let runtime;
    if (w.driverAlive === null) runtime = "unknown";
    else if (!w.driverAlive) runtime = "inactive";
    else runtime = (heartbeatMs != null && heartbeatMs > STALL_MS) ? "stalled" : "active";
    rows.push({
      id: w.name,
      title: w.title || meta.title || w.name,
      specRef: w.specRef || meta.specRef || "",
      workflow: mapWorkflow(w.sbxState || ""),
      result: mapResult(w.progress || meta.progress || "unknown"),
      runtime,
      round: w.round, roundsTotal: w.roundsTotal,
      sandbox: w.sandbox, sbxState: w.sbxState,
      lastActivity: lastAct ? { summary: lastAct.summary, kind: lastAct.kind, time: lastAct.time } : null,
      lastActivityAgeMs: lastAct && lastAct.time != null ? now - lastAct.time : null,
      events: collapseEvents(events).slice(-40),
      driverAgeMs: w.driverAgeMs,
      // legacy compat fields (existing consumers: dag panel stream, etc.)
      agentLog: w.agentLog, workerLog: w.workerLog,
    });
  }

  // --- legacy local-loop workers ---------------------------------------------
  // Skip tasks already tracked by the sandbox fleet (task migrated from the
  // old local loop to sbx — stale worktree dirs linger).
  const sbxIds = new Set(rows.map((r) => r.id));
  for (const w of localWorkers) {
    if (sbxIds.has(w.name)) continue;
    const meta = metaByName.get(w.name) || {}; // was: undefined `meta` — threw
    const events = activityMap.get(w.name) || [];
    const lastAct = lastMeaningfulActivity(events);
    rows.push({
      id: w.name,
      title: w.title || meta.title || w.name,
      specRef: w.specRef || meta.specRef || "",
      workflow: mapWorkflow(w.workerLog || ""),
      result: mapResult(w.progress || meta.progress || "unknown"),
      runtime: w.agentAgeMs != null && w.agentAgeMs < STALL_MS ? "active" : "inactive",
      round: null, roundsTotal: null,
      sandbox: null, sbxState: "local",
      lastActivity: lastAct ? { summary: lastAct.summary, kind: lastAct.kind, time: lastAct.time } : null,
      lastActivityAgeMs: lastAct && lastAct.time != null ? now - lastAct.time : w.agentAgeMs,
      events: collapseEvents(events).slice(-40),
      driverAgeMs: w.agentAgeMs,
      agentLog: w.agentLog, workerLog: w.workerLog,
    });
  }

  // --- tasks with no worker at all (queue) ------------------------------------
  const seen = new Set(rows.map((r) => r.id));
  for (const t of taskMeta) {
    if (seen.has(t.name)) continue;
    rows.push({
      id: t.name, title: t.title || t.name, specRef: t.specRef || "",
      workflow: "implementation",
      result: mapResult(t.progress),
      runtime: "inactive",
      round: null, roundsTotal: null, sandbox: null, sbxState: null,
      lastActivity: null, lastActivityAgeMs: null,
      events: [], driverAgeMs: null,
    });
  }

  // --- attention dimension ------------------------------------------------------
  const incidents = groupIncidents(
    [...activityMap.values()].flat().filter((e) => e.taskId)
  );
  for (const r of rows) {
    r.attention = deriveAttention(r, incidents);
  }

  // --- overview ordering + age-out -----------------------------------------------
  // Only needs-revision is exempt from age-out (it always needs an operator
  // decision). Attention alone does NOT keep a dead task visible forever —
  // a 24h-old warning nobody cleared shouldn't pin the Overview.
  const overview = rows
    .filter((r) =>
      r.result === "needs-revision" ||
      r.runtime === "active" || r.runtime === "stalled" || r.runtime === "unknown" ||
      (r.lastActivityAgeMs != null && r.lastActivityAgeMs < AGE_OUT_MS)
    )
    .sort((a, b) =>
      attentionRank(b) - attentionRank(a) ||
      runtimeRank(b) - runtimeRank(a) ||
      (b.lastActivityAgeMs ?? Infinity) - (a.lastActivityAgeMs ?? Infinity) // most recent first, nulls last
    );

  const queue = countBy(rows, (r) => r.result);

  return {
    when: now,
    health: {
      loopAlive, diskFree, wipGuarded,
      fleet,
      workersKnown: rows.filter((r) => r.driverAgeMs != null).length,
      workersActive: rows.filter((r) => r.runtime === "active").length,
      workersStalled: rows.filter((r) => r.runtime === "stalled").length,
      workersReceiving: 0,
    },
    coverage: coveragePrecise || coverage,
    coverageSummary: coverage, coverageTrend,
    rows,
    overview,
    incidents,
    queue,
    deps: Object.fromEntries(taskMeta.map((t) => [t.name, t.deps || []])),
    orchestratorLog,
  };
}

function mapWorkflow(stateText) {
  const s = (stateText || "").toLowerCase();
  if (/audit/.test(s)) return "audit";
  if (/review/.test(s)) return "review";
  if (/merge|ready-for-merge|rebasing/.test(s)) return "merge";
  return "implementation";
}

function mapResult(progress) {
  switch ((progress || "").toLowerCase()) {
    case "complete": case "done": return "complete";
    case "needs-revision": return "needs-revision";
    case "in-progress": case "assigned": case "task-assigned": return "in-progress";
    case "not-started": return "not-started";
    default: return "unknown";
  }
}

function hasRecentEvent(events, now) {
  for (let i = events.length - 1; i >= 0; i--) {
    if (events[i].time != null && now - events[i].time < AGE_OUT_MS) return true;
  }
  return false;
}

function deriveAttention(row, incidents) {
  const taskIncidents = incidents.filter((i) => i.taskId === row.id && i.severity === "critical");
  if (taskIncidents.length) return "critical";
  if (row.runtime === "stalled") return "warning";
  if (row.result === "needs-revision" && row.runtime !== "active") return "warning";
  const warnIncidents = incidents.filter((i) => i.taskId === row.id && i.severity === "warning");
  if (warnIncidents.length >= 3) return "warning";
  return "none";
}

function attentionRank(r) {
  return r.attention === "critical" ? 3 : r.attention === "warning" ? 2 : 0;
}
function runtimeRank(r) {
  return r.runtime === "active" ? 3 : r.runtime === "stalled" ? 2 : r.runtime === "unknown" ? 1 : 0;
}
function countBy(arr, f) {
  const out = {};
  for (const x of arr) { const k = f(x); out[k] = (out[k] || 0) + 1; }
  return out;
}

// The activity ring ({get, tasks, drop}) from log-tail.mjs is not a Map;
// adapt it so buildSnapshot stays pure and testable with plain Maps.
function ringAsMap(ring) {
  const m = new Map();
  try {
    for (const t of ring.tasks()) m.set(t, ring.get(t));
  } catch { /* empty ring */ }
  return m;
}
