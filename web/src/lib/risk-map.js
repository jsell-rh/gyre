/**
 * risk-map.js — Risk Map overlay utilities (system-explorer.md §7)
 *
 * The Risk Map is an Evaluative-lens overlay on the architecture canvas:
 *   - Heat map mode: nodes are colored on a continuous blue → yellow → red
 *     scale by a user-selected metric (churn, coupling, complexity, spec
 *     coverage, test coverage). Node positions and layout never change —
 *     only the fill color does.
 *   - Anomaly callouts: automated warnings derived from the same data
 *     (high complexity + low test coverage, co-changing modules without a
 *     shared spec, agent-created types with no governing spec, traits with
 *     partially tested implementations).
 *
 * Data sources:
 *   - GET /repos/:id/graph/risks (RiskMetricsResponse): churn_rate, fan_in,
 *     fan_out, complexity, spec_covered per node.
 *   - Graph node properties (test_coverage, spec_path, last_modified_*).
 *
 * Everything here is pure so the coloring rules and anomaly patterns are
 * unit-testable independently of the canvas render loop.
 */

// ── Metric definitions ────────────────────────────────────────────────────

/** Selectable Risk Map metrics (system-explorer.md §7 heat map mode).
 *  `invert` marks coverage-family metrics where LOW value = HIGH risk, so
 *  the heat scale runs cool blue = covered/tested → hot red = uncovered. */
export const RISK_MAP_METRICS = [
  {
    key: 'churn',
    label: 'Churn',
    lowLabel: 'Stable',
    highLabel: 'High churn',
    description: 'Commits touching the node in the last 30 days (cool blue = stable, hot red = high churn)',
    invert: false,
  },
  {
    key: 'coupling',
    label: 'Coupling',
    lowLabel: 'Low',
    highLabel: 'High',
    description: 'Coupling score: fan-in + fan-out edges (hot red = tightly coupled)',
    invert: false,
  },
  {
    key: 'complexity',
    label: 'Complexity',
    lowLabel: 'Low',
    highLabel: 'High',
    description: 'Cyclomatic complexity (hot red = most complex)',
    invert: false,
  },
  {
    key: 'spec_coverage',
    label: 'Spec Coverage',
    lowLabel: 'Governed',
    highLabel: 'No spec',
    description: 'Spec coverage (cool blue = governed by a spec, hot red = no governing spec)',
    invert: true,
  },
  {
    key: 'test_coverage',
    label: 'Test Coverage',
    lowLabel: 'Tested',
    highLabel: 'Untested',
    description: 'Test coverage (cool blue = well tested, hot red = untested)',
    invert: true,
  },
];

export const RISK_MAP_METRIC_KEYS = RISK_MAP_METRICS.map((m) => m.key);

export function riskMetricDef(key) {
  return RISK_MAP_METRICS.find((m) => m.key === key) ?? null;
}

// ── Heat map color scale ──────────────────────────────────────────────────

/** Continuous heat scale: cool blue (t=0) → yellow (t=0.5) → hot red (t=1).
 *  `t` is the normalized risk position in [0, 1] (1 = most attention
 *  needed). Endpoint anchors: hsl(210,70%,42%) / hsl(55,80%,50%) /
 *  hsl(0,90%,42%). */
export function riskHeatColor(t) {
  const x = Number.isFinite(t) ? Math.min(1, Math.max(0, t)) : 0;
  if (x < 0.5) {
    const u = x / 0.5; // blue (210°) → yellow (55°)
    return `hsl(${Math.round(210 - u * 155)}, ${Math.round(70 + u * 10)}%, ${Math.round(42 + u * 8)}%)`;
  }
  const u = (x - 0.5) / 0.5; // yellow (55°) → red (0°)
  return `hsl(${Math.round(55 - u * 55)}, ${Math.round(80 + u * 10)}%, ${Math.round(50 - u * 8)}%)`;
}

// ── Per-node metric values ────────────────────────────────────────────────

function num(v) {
  if (v == null) return null; // Number(null) is 0 — null means no data
  const n = Number(v);
  return Number.isFinite(n) ? n : null;
}

/** Raw metric value for one node. Churn/coupling/spec coverage come from
 *  the /graph/risks entry; complexity and test coverage come from the
 *  knowledge graph node properties (falling back to the risk entry for
 *  complexity when the node prop is absent). Returns null when no data
 *  exists for the metric — the node gets no heat color rather than a
 *  fabricated zero. */
export function riskMetricRawValue(metricKey, riskEntry, node) {
  switch (metricKey) {
    case 'churn': {
      if (!riskEntry) return null;
      return num(riskEntry.churn_rate) ?? 0;
    }
    case 'coupling': {
      if (!riskEntry) return null;
      const fanIn = num(riskEntry.fan_in) ?? 0;
      const fanOut = num(riskEntry.fan_out) ?? 0;
      return fanIn + fanOut;
    }
    case 'complexity': {
      const c = num(node?.complexity) ?? num(riskEntry?.complexity);
      return c;
    }
    case 'spec_coverage': {
      if (!riskEntry) return null;
      return riskEntry.spec_covered ? 1 : 0;
    }
    case 'test_coverage': {
      // Graph node property per spec; ratio in [0, 1].
      const cov = num(node?.test_coverage);
      if (cov == null) return null;
      return Math.min(1, Math.max(0, cov));
    }
    default:
      return null;
  }
}

/** Build the Risk Map heat layer for a metric: maps every node with data
 *  to its normalized risk position t and its heat color. Positions and
 *  layout are untouched — the caller only swaps the node fill.
 *
 *  Returns { metric, max, byNodeId: Map<nodeId, {value, t, color}> }.
 *  `max` is the largest raw value observed (normalization denominator). */
export function buildRiskHeatMap(riskEntries, nodes, metricKey) {
  const def = riskMetricDef(metricKey) ?? RISK_MAP_METRICS[0];
  const riskById = new Map();
  for (const r of Array.isArray(riskEntries) ? riskEntries : []) {
    if (r?.node_id) riskById.set(r.node_id, r);
  }

  const entries = [];
  for (const n of Array.isArray(nodes) ? nodes : []) {
    if (!n?.id || n.deleted_at || n.node_type === 'tree-group') continue;
    const value = riskMetricRawValue(def.key, riskById.get(n.id), n);
    if (value == null) continue;
    entries.push({ nodeId: n.id, value });
  }

  let max = 0;
  for (const e of entries) {
    if (e.value > max) max = e.value;
  }

  const byNodeId = new Map();
  for (const e of entries) {
    // max <= 0 means every node sits at the metric floor (e.g. zero churn
    // everywhere) — that is a valid uniform cool-blue state, not no-data.
    const norm = max > 0 ? Math.min(1, e.value / max) : 0;
    const t = def.invert ? 1 - norm : norm;
    byNodeId.set(e.nodeId, { value: e.value, t, color: riskHeatColor(t) });
  }
  return { metric: def.key, max, byNodeId };
}

// ── Anomaly detection (system-explorer.md §7 anomaly callouts) ───────────

function edgeSrc(e) {
  return e?.source_id ?? e?.from_node_id ?? e?.from;
}

function edgeTgt(e) {
  return e?.target_id ?? e?.to_node_id ?? e?.to;
}

function edgeType(e) {
  return (e?.edge_type ?? e?.type ?? '').toLowerCase();
}

/** Detect anomaly callouts from the risk data. Pure: takes the canvas's
 *  nodes, edges, and the /graph/risks entries; returns callouts sorted by
 *  severity (high first), each linking to the offending node(s).
 *
 *  Patterns (spec §7 examples):
 *    1. "Module X has high complexity (42) but low test coverage (23%)"
 *    2. "These 3 modules always change together but have no shared spec"
 *    3. "Type Y was created by an agent but has no governing spec"
 *    4. "This trait has 5 implementations but only 2 are tested"
 *  plus heavily-depended-on-without-spec and dead-code orphans that the
 *  same risk data surfaces. Thresholds are distribution-relative
 *  (percentiles), not hardcoded, so they adapt to each codebase. */
export function detectRiskAnomalies({ nodes = [], edges = [], riskEntries = [] } = {}) {
  const results = [];
  const nodeById = new Map();
  for (const n of nodes) nodeById.set(n.id, n);
  const riskById = new Map();
  for (const r of Array.isArray(riskEntries) ? riskEntries : []) {
    if (r?.node_id) riskById.set(r.node_id, r);
  }

  // Server-computed risk metrics take precedence over stale node props;
  // fall back to the graph node when no risk entry exists.
  const complexityOf = (n) => num(riskById.get(n.id)?.complexity) ?? num(n.complexity) ?? 0;
  const specCovered = (n) => {
    const r = riskById.get(n.id);
    if (r) return !!r.spec_covered;
    return !!n.spec_path;
  };

  // Incoming call counts (heavily-depended-on and dead-code patterns).
  const incomingCallCounts = new Map();
  for (const e of edges) {
    const tgt = edgeTgt(e);
    if (edgeType(e) === 'calls' && tgt) {
      incomingCallCounts.set(tgt, (incomingCallCounts.get(tgt) ?? 0) + 1);
    }
  }

  // GovernedBy index: node ids with an explicit governing-spec edge.
  const governedByIndex = new Set();
  for (const e of edges) {
    if (e.deleted_at) continue;
    if (edgeType(e) === 'governed_by') governedByIndex.add(edgeSrc(e));
  }

  // Trait implementation counts (pattern 4).
  const traitImplCounts = new Map(); // traitId → { total, tested }
  for (const e of edges) {
    const src = edgeSrc(e);
    const tgt = edgeTgt(e);
    if (edgeType(e) !== 'implements') continue;
    const tgtNode = nodeById.get(tgt);
    if (tgtNode && (tgtNode.node_type === 'interface' || tgtNode.node_type === 'trait')) {
      if (!traitImplCounts.has(tgt)) traitImplCounts.set(tgt, { total: 0, tested: 0 });
      const entry = traitImplCounts.get(tgt);
      entry.total++;
      const srcNode = nodeById.get(src);
      if (srcNode?.test_coverage > 0 || srcNode?.test_node) entry.tested++;
    }
  }

  // Distribution-relative thresholds (percentile-based, not hardcoded).
  const complexities = nodes.filter(n => n.complexity != null && !n.deleted_at).map(n => complexityOf(n));
  const coverages = nodes.filter(n => n.test_coverage != null && !n.deleted_at).map(n => n.test_coverage);
  const callCounts = [];
  for (const n of nodes) {
    if (!n.deleted_at && n.node_type !== 'tree-group') callCounts.push(incomingCallCounts.get(n.id) ?? 0);
  }
  function p90(arr) {
    if (arr.length === 0) return Infinity;
    const sorted = [...arr].sort((a, b) => a - b);
    const idx = Math.min(Math.floor(sorted.length * 0.9), sorted.length - 1);
    return sorted[idx];
  }
  function p10(arr) {
    if (arr.length === 0) return 0;
    const sorted = [...arr].sort((a, b) => a - b);
    const idx = Math.min(Math.floor(sorted.length * 0.1), sorted.length - 1);
    return sorted[idx];
  }
  const complexityThreshold = Math.max(10, p90(complexities));
  const coverageThreshold = Math.min(0.5, p10(coverages));
  const callThreshold = Math.max(3, p90(callCounts));

  for (const n of nodes) {
    if (n.node_type === 'tree-group' || n.deleted_at) continue;
    const calls = incomingCallCounts.get(n.id) ?? 0;

    // Pattern 1: High complexity + low test coverage (distribution-relative)
    if (complexityOf(n) > complexityThreshold && (n.test_coverage ?? 0) < coverageThreshold) {
      results.push({
        nodeId: n.id,
        nodeName: n.name ?? n.qualified_name ?? '?',
        message: `High complexity (${complexityOf(n)}, p90=${complexityThreshold}) but low test coverage (${Math.round((n.test_coverage ?? 0) * 100)}%)`,
        severity: 'high',
      });
    }
    // Pattern 2: Heavily depended on with no spec (distribution-relative)
    if (calls > callThreshold && !specCovered(n)) {
      results.push({
        nodeId: n.id,
        nodeName: n.name ?? n.qualified_name ?? '?',
        message: `Heavily depended on (${calls} callers, p90=${callThreshold}) with no spec`,
        severity: 'medium',
      });
    }
    // Pattern 3: Trait has N implementations but only M tested
    if (n.node_type === 'interface' || n.node_type === 'trait') {
      const entry = traitImplCounts.get(n.id);
      if (entry && entry.total > 1 && entry.tested < entry.total) {
        results.push({
          nodeId: n.id,
          nodeName: n.name ?? n.qualified_name ?? '?',
          message: `${entry.total} implementations but only ${entry.tested} tested`,
          severity: 'medium',
        });
      }
    }
    // Pattern 4: Type/interface created by agent but has no governing spec
    if ((n.node_type === 'type' || n.node_type === 'interface') && !specCovered(n) && n.last_modified_by) {
      const hasGovEdge = governedByIndex.has(n.id);
      if (!hasGovEdge) {
        results.push({
          nodeId: n.id,
          nodeName: n.name ?? n.qualified_name ?? '?',
          message: `Agent-created ${n.node_type} has no governing spec`,
          severity: 'medium',
        });
      }
    }
    // Pattern 5: Orphan function — no callers (possible dead code)
    if (n.node_type === 'function' && !n.test_node && calls === 0) {
      results.push({
        nodeId: n.id,
        nodeName: n.name ?? n.qualified_name ?? '?',
        message: 'No callers \u2014 possible dead code',
        severity: 'low',
      });
    }
  }
  // Pattern 6: Co-change detection — modules that change together but have
  // no shared spec. Two heuristics: (a) same commit SHA, (b) modified
  // within the same time window (1 hour). This catches co-changes in
  // separate commits within the same PR.
  const coChangeGroups = new Map(); // key → [node]
  for (const n of nodes) {
    if (n.deleted_at || n.node_type === 'tree-group') continue;
    // Group by SHA (exact same commit)
    if (n.last_modified_sha) {
      const key = `sha:${n.last_modified_sha}`;
      if (!coChangeGroups.has(key)) coChangeGroups.set(key, []);
      coChangeGroups.get(key).push(n);
    }
    // Group by time window (same hour, to catch separate commits in same PR)
    if (n.last_modified_at) {
      const hourBucket = Math.floor(n.last_modified_at / 3600);
      const key = `time:${hourBucket}`;
      if (!coChangeGroups.has(key)) coChangeGroups.set(key, []);
      coChangeGroups.get(key).push(n);
    }
  }
  // Deduplicate: track which node pairs we've already reported
  const reportedPairs = new Set();
  // Find groups of 3+ nodes from different modules that share a change
  // pattern but no shared spec.
  for (const [, group] of coChangeGroups) {
    if (group.length < 3) continue;
    const modules = new Set(group.map(n => n.file_path?.split('/').slice(0, -1).join('/') ?? ''));
    if (modules.size < 2) continue; // Same module, not interesting
    const specs = new Set(group.filter(n => n.spec_path).map(n => n.spec_path));
    if (specs.size > 0) continue; // Have shared spec governance
    // Dedup by first 2 node IDs
    const pairKey = group.slice(0, 2).map(n => n.id).sort().join(',');
    if (reportedPairs.has(pairKey)) continue;
    reportedPairs.add(pairKey);
    const names = group.slice(0, 3).map(n => n.name ?? '?').join(', ');
    results.push({
      nodeId: group[0].id,
      nodeName: names,
      message: `${group.length} nodes across ${modules.size} modules change together but have no shared spec`,
      severity: 'medium',
    });
  }
  // Sort by severity (high first) — show all anomalies, not an arbitrary cap.
  // The panel scrolls so users can explore the full risk landscape.
  const order = { high: 0, medium: 1, low: 2 };
  results.sort((a, b) => (order[a.severity] ?? 3) - (order[b.severity] ?? 3));
  return results;
}
