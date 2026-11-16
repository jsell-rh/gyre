/**
 * Liveness indicators for the Explorer canvas (HSI §3 "The Built Software Is
 * Always Alive", Layer 1 — SDLC activity).
 *
 * Two data legs live here:
 *
 * 1. Active agent runs — `RunStarted`/`RunFinished` Telemetry-tier bus
 *    messages arrive over the workspace WebSocket subscription (the server
 *    broadcasts the Message envelope with `kind: "run_started"` /
 *    `"run_finished"` and a `{ agent_id, task_id? }` payload). The tracker
 *    resolves agent → task → spec_ref (via the task API) so the canvas can
 *    map the run to the graph nodes its spec governs.
 *
 * 2. Pure helpers that turn server records into per-node/per-edge indicator
 *    state: recent `ArchitecturalDelta` changes (fading ring over the last
 *    hour) and MR gate results (green/red edges in the Evaluative lens).
 *
 * The module owns reactive state (`activeRuns`) so App.svelte can wire the
 * WebSocket once while ExplorerCanvas (rendered deep in the tree) reads it
 * without prop drilling.
 */

import { parseDeltaJson } from './timeline-utils.js';

/** Window for the "recent changes" fading ring — one hour per spec. */
export const RECENT_CHANGE_WINDOW_MS = 60 * 60 * 1000;

/** ArchitecturalDelta timestamps are epoch SECONDS (graph_extraction.rs). */
const DELTA_TIMESTAMP_UNIT_MS = 1000;

// ── Active agent runs (Layer 1: agent glow) ───────────────────────────────

/**
 * Reactive map of agentId → { agentId, taskId, specRef, startedAt }.
 * Populated by the WebSocket tracker below; read by the canvas to compute
 * which nodes currently have an agent working on them.
 */
export const activeRuns = $state({});

/** Test/teardown helper — clears all tracked runs. */
export function resetActiveRuns() {
  for (const k of Object.keys(activeRuns)) delete activeRuns[k];
}

/**
 * Wire a WebSocket store to the liveness state. Listens for bus messages
 * with kind run_started/run_finished (Telemetry tier, per message-bus.md),
 * records the run, and asynchronously resolves the task's spec_ref so the
 * canvas can highlight the governed nodes.
 *
 * @param {{ onMessage: Function } | null} wsStore — the shared WS store from ws.js
 * @param {object} [opts]
 * @param {(taskId: string) => Promise<{spec_ref?: string|null}|null>} [opts.fetchTask]
 *        resolves a task id to its record (defaults to no resolution)
 * @returns {{ destroy: () => void }}
 */
export function createLivenessTracker(wsStore, opts = {}) {
  if (!wsStore || typeof wsStore.onMessage !== 'function') {
    return { destroy() {} };
  }
  const { fetchTask = async () => null } = opts;

  const unsub = wsStore.onMessage((msg) => {
    const kind = msg?.kind;
    const payload = msg?.payload;
    if (!payload || typeof payload !== 'object') return;
    const agentId = payload.agent_id;
    if (!agentId) return;

    if (kind === 'run_started') {
      activeRuns[agentId] = {
        agentId,
        taskId: payload.task_id ?? null,
        specRef: null,
        startedAt: Date.now(),
      };
      // Resolve task → spec asynchronously; the bus message carries only
      // ids. Best-effort: an unresolvable task still tracks the run (the
      // agent itself is alive) but no nodes glow until the spec resolves.
      // The task API returns the spec as `spec_path` (e.g.
      // "specs/system/agent-gates.md").
      const taskId = payload.task_id;
      if (taskId) {
        Promise.resolve(fetchTask(taskId))
          .then((task) => {
            const run = activeRuns[agentId];
            // Guard: the run may have finished while the fetch was in flight.
            if (task?.spec_path && run && run.taskId === taskId) {
              activeRuns[agentId] = { ...run, specRef: task.spec_path };
            }
          })
          .catch(() => { /* spec resolution is best-effort */ });
      }
    } else if (kind === 'run_finished') {
      delete activeRuns[agentId];
    }
  });

  return {
    destroy() {
      unsub();
    },
  };
}

// ── Spec-ref → graph node mapping ─────────────────────────────────────────

/**
 * Extract the spec file name from a spec_ref string.
 * Task/MR spec_refs look like "human-system-interface.md §3 Design Principle"
 * — the leading (or embedded) "*.md" path is the file identity.
 * @returns {string|null} the file name (basename), or null when no .md file
 */
export function specRefFilename(specRef) {
  if (!specRef || typeof specRef !== 'string') return null;
  const m = /([\w./-]+\.md)/i.exec(specRef);
  return m ? m[1].split('/').pop().toLowerCase() : null;
}

/**
 * Compute the set of graph node ids governed by a spec reference.
 *
 * Matches two signals, mirroring the canvas's spec-governance display:
 *  - nodes whose spec_path / spec_paths basename equals the spec file name
 *  - nodes with a `governed_by` edge to a spec node whose file_path or name
 *    matches the spec file name (the canonical linkage per realized-model.md)
 *
 * @param {Array} nodes — graph nodes
 * @param {Array} edges — graph edges (normalized via source_id/target_id/edge_type)
 * @param {string} specRef — e.g. "auth.md §2"
 * @returns {Set<string>} node ids
 */
export function nodesForSpecRef(nodes, edges, specRef) {
  const ids = new Set();
  const fname = specRefFilename(specRef);
  if (!fname || !Array.isArray(nodes)) return ids;

  const nodeById = new Map(nodes.map((n) => [n.id, n]));

  for (const n of nodes) {
    const paths = [n.spec_path, ...(Array.isArray(n.spec_paths) ? n.spec_paths : [])].filter(Boolean);
    if (paths.some((p) => String(p).split('/').pop().toLowerCase() === fname)) {
      ids.add(n.id);
    }
  }

  if (Array.isArray(edges)) {
    for (const e of edges) {
      if (e.deleted_at) continue;
      const type = (e.edge_type ?? e.type ?? '').toLowerCase();
      if (type !== 'governed_by') continue;
      const src = e.source_id ?? e.from_node_id ?? e.from;
      const tgt = e.target_id ?? e.to_node_id ?? e.to;
      const specNode = nodeById.get(tgt);
      if (!specNode) continue;
      const specIdentities = [specNode.file_path, specNode.name].filter(Boolean);
      if (specIdentities.some((v) => String(v).split('/').pop().toLowerCase() === fname)) {
        ids.add(src);
      }
    }
  }
  return ids;
}

/**
 * Union of node ids governed by the specs of all currently active runs.
 * @returns {Set<string>}
 */
export function activeAgentNodeIds(nodes, edges) {
  const ids = new Set();
  for (const run of Object.values(activeRuns)) {
    if (!run.specRef) continue;
    for (const id of nodesForSpecRef(nodes, edges, run.specRef)) ids.add(id);
  }
  return ids;
}

// ── Recent changes (Layer 1: fading ring) ─────────────────────────────────

/**
 * Compute nodes added/modified in the last hour from ArchitecturalDelta
 * records (the graph/timeline endpoint), per HSI §3: "nodes modified in the
 * last hour have a fading highlight ring".
 *
 * Delta timestamps are epoch seconds; `nodes_added` entries carry
 * { qualified_name, ... } and `nodes_modified` entries carry
 * { qualified_name, field_changes } (or a bare count when no agent context —
 * count-only deltas cannot name nodes and are skipped).
 *
 * @param {Array} nodes — current graph nodes (matched by qualified_name)
 * @param {Array} timeline — ArchitecturalDelta records
 * @param {number} [nowMs] — evaluation time (ms); defaults to Date.now()
 * @returns {Map<string, {changedAtMs: number}>} node id → most recent change time
 */
export function computeRecentChanges(nodes, timeline, nowMs = Date.now()) {
  const byId = new Map();
  if (!Array.isArray(nodes) || !Array.isArray(timeline) || timeline.length === 0) return byId;

  const qnToNode = new Map();
  for (const n of nodes) {
    if (n?.qualified_name) qnToNode.set(n.qualified_name, n);
  }

  for (const delta of timeline) {
    const tsMs = Number(delta?.timestamp) * DELTA_TIMESTAMP_UNIT_MS;
    if (!Number.isFinite(tsMs)) continue;
    const age = nowMs - tsMs;
    if (age < 0 || age > RECENT_CHANGE_WINDOW_MS) continue;

    const parsed = parseDeltaJson(delta.delta_json);
    const changedQns = new Set();
    for (const entry of parsed.nodes_added ?? []) {
      const qn = typeof entry === 'string' ? entry : entry?.qualified_name;
      if (qn) changedQns.add(qn);
    }
    if (Array.isArray(parsed.nodes_modified)) {
      for (const entry of parsed.nodes_modified) {
        const qn = typeof entry === 'string' ? entry : entry?.qualified_name;
        if (qn) changedQns.add(qn);
      }
    }

    for (const qn of changedQns) {
      const node = qnToNode.get(qn);
      if (!node) continue;
      const prev = byId.get(node.id);
      if (!prev || tsMs > prev.changedAtMs) {
        byId.set(node.id, { changedAtMs: tsMs });
      }
    }
  }
  return byId;
}

/**
 * Fading-ring opacity for a recent-change entry: full at the moment of the
 * change, linearly fading to 0 over the one-hour window. Computed at draw
 * time (not cached) so the ring keeps fading without per-frame timers.
 *
 * @param {{changedAtMs: number}} entry
 * @param {number} [nowMs]
 * @returns {number} alpha in [0, 1]; <=0 means the ring has fully faded
 */
export function recentChangeAlpha(entry, nowMs = Date.now()) {
  if (!entry?.changedAtMs) return 0;
  const age = nowMs - entry.changedAtMs;
  if (age < 0) return 1;
  return Math.max(0, 1 - age / RECENT_CHANGE_WINDOW_MS);
}

// ── Gate status (Layer 1: evaluative edge colors) ─────────────────────────

/**
 * Compute per-node gate pass/fail from MR gate evaluation records for the
 * current scope, per HSI §3: "edges show gate pass/fail as color (green/red)
 * when the Evaluative lens is active".
 *
 * Each record maps an MR's spec_ref to its gate results; the governed nodes
 * inherit the outcome. A failed gate wins over any passing one (a node is
 * only "passed" when every gate on every governing MR passed).
 *
 * @param {Array} nodes — graph nodes
 * @param {Array} edges — graph edges
 * @param {Array} gateRecords — [{ mrId, spec_ref, gates: [{status}] }]
 * @returns {Map<string, 'passed'|'failed'>} node id → status (absent = no
 *          gate data for that node — edge stays structurally colored)
 */
export function computeNodeGateStatus(nodes, edges, gateRecords) {
  const byId = new Map();
  if (!Array.isArray(gateRecords) || gateRecords.length === 0) return byId;

  for (const rec of gateRecords) {
    if (!rec?.spec_ref) continue;
    const gates = Array.isArray(rec.gates) ? rec.gates : [];
    // Pending/running gates prove nothing yet — only terminal states count.
    const terminal = gates.filter((g) => g?.status === 'passed' || g?.status === 'failed');
    if (terminal.length === 0) continue;
    const failed = terminal.some((g) => g.status === 'failed');

    for (const id of nodesForSpecRef(nodes, edges, rec.spec_ref)) {
      if (failed) {
        byId.set(id, 'failed');
      } else if (byId.get(id) !== 'failed') {
        byId.set(id, 'passed');
      }
    }
  }
  return byId;
}

/**
 * Gate status for an edge given per-node statuses, per the implementation
 * plan: green when all gates passed for BOTH connected nodes, red when at
 * least one connected node has a failed gate.
 *
 * @param {Map<string,'passed'|'failed'>} nodeStatus
 * @param {string} srcId
 * @param {string} tgtId
 * @returns {'passed'|'failed'|null} null when neither endpoint has gate data
 */
export function edgeGateStatus(nodeStatus, srcId, tgtId) {
  const s = nodeStatus.get(srcId);
  const t = nodeStatus.get(tgtId);
  if (s === 'failed' || t === 'failed') return 'failed';
  if (s === 'passed' && t === 'passed') return 'passed';
  return null;
}
