/**
 * lens-rendering.js — Three Lenses rendering pipeline (human-system-interface.md §3)
 *
 * Every Explorer view can be overlaid with one of three lenses:
 *   - Structural (default): knowledge graph nodes + edges — the base topology.
 *   - Evaluative: test coverage, gate outcomes, spec assertion/approval status
 *     overlaid ON TOP of the structural topology (lenses compose).
 *   - Observable (future): production telemetry — disabled until integrated.
 *
 * This module is the lens decoration pipeline: pure functions that accept a
 * base node (or edge) plus lens data and return the decorations to render.
 * Keeping them pure lets the decoration rules be unit-tested independently
 * of the canvas render loop.
 *
 * Edge/node field names follow the knowledge graph API shapes
 * (`source_id`/`from_node_id`, `target_id`/`to_node_id`, `edge_type`/`type`).
 */

export const LENSES = ['structural', 'evaluative', 'observable'];

/** Normalize an external lens value (e.g. `?lens=` URL param).
 *  The Observable lens is explicitly future work (production telemetry not
 *  yet integrated) so it cannot be activated — anything that is not
 *  'evaluative' falls back to the structural default. */
export function normalizeLens(value) {
  return value === 'evaluative' ? 'evaluative' : 'structural';
}

function edgeSrc(e) {
  return e?.source_id ?? e?.from_node_id ?? e?.from;
}

function edgeTgt(e) {
  return e?.target_id ?? e?.to_node_id ?? e?.to;
}

function edgeType(e) {
  return (e?.edge_type ?? e?.type ?? '').toLowerCase();
}

// ── Evaluative: test coverage (GraphNode.test_coverage, 0.0–1.0) ─────────

export const COVERAGE_GREEN_THRESHOLD = 0.7;
export const COVERAGE_YELLOW_THRESHOLD = 0.3;

/** Classify a coverage ratio: 'high' (green), 'partial' (yellow), 'low' (red).
 *  Returns null when coverage data is unavailable (GraphNode.test_coverage
 *  is `None` server-side). Thresholds match the tooltip semantics:
 *  <30% low, <70% partial, >=70% well tested. */
export function coverageLevel(coverage) {
  if (coverage == null || Number.isNaN(coverage)) return null;
  if (coverage >= COVERAGE_GREEN_THRESHOLD) return 'high';
  if (coverage >= COVERAGE_YELLOW_THRESHOLD) return 'partial';
  return 'low';
}

const COVERAGE_COLORS = {
  high: { bg: '#14532d', fg: '#86efac', color: '#22c55e' },
  partial: { bg: '#713f12', fg: '#fde047', color: '#eab308' },
  low: { bg: '#7f1d1d', fg: '#fca5a5', color: '#ef4444' },
};

/** Color-coded coverage badge decoration for a node (green/yellow/red heatmap).
 *  Null when the node carries no coverage data. */
export function coverageDecoration(node) {
  const level = coverageLevel(node?.test_coverage);
  if (!level) return null;
  return {
    level,
    label: `${Math.round(node.test_coverage * 100)}%`,
    bg: COVERAGE_COLORS[level].bg,
    fg: COVERAGE_COLORS[level].fg,
    color: COVERAGE_COLORS[level].color,
  };
}

// ── Evaluative: gate status (most recent gate results, joined via spec) ──

export const GATE_PASS_COLOR = '#22c55e';
export const GATE_FAIL_COLOR = '#ef4444';

/** All spec paths that govern a node: the primary `spec_path`, the N:M
 *  `spec_paths` extras, and the targets of GovernedBy edges (canonical link).
 *  Gate results are recorded per MR, and MRs carry a `spec_ref`
 *  ("path@sha"), so the node→spec join is what routes a gate outcome to a
 *  node. */
export function nodeSpecPaths(node, edges = [], nodesById = new Map()) {
  const paths = new Set();
  if (node?.spec_path) paths.add(node.spec_path);
  for (const p of node?.spec_paths ?? []) {
    if (p) paths.add(p);
  }
  if (node?.id) {
    for (const e of edges) {
      if (e?.deleted_at) continue;
      if (edgeType(e) !== 'governed_by') continue;
      if (edgeSrc(e) !== node.id) continue;
      const target = nodesById.get(edgeTgt(e));
      const p = target?.file_path || target?.name;
      if (p) paths.add(p);
    }
  }
  return [...paths];
}

/** Most recent gate outcome for a set of spec paths. A failing gate on any
 *  governing spec fails the node (conservative: red must not be masked by a
 *  green sibling spec). Null when no spec has a concluded gate run. */
export function gateStatusForPaths(paths, gateStatusBySpec = {}) {
  let status = null;
  for (const p of paths) {
    const s = gateStatusBySpec[p];
    if (s === 'fail') return 'fail';
    if (s === 'pass') status = 'pass';
  }
  return status;
}

/** Gate status decoration for a node: a colored status ring.
 *  Green = all recent gates passed, red = at least one failed. */
export function gateStatusDecoration(node, gateStatusBySpec = {}, edges = [], nodesById = new Map()) {
  const status = gateStatusForPaths(nodeSpecPaths(node, edges, nodesById), gateStatusBySpec);
  if (!status) return null;
  return { status, color: status === 'pass' ? GATE_PASS_COLOR : GATE_FAIL_COLOR };
}

/** Edge decoration for the evaluative lens: edges leaving a node governed by
 *  a spec with a concluded gate run are colored green (pass) / red (fail). */
export function gateEdgeDecoration(sourceNode, gateStatusBySpec = {}, edges = [], nodesById = new Map()) {
  if (!sourceNode) return null;
  const status = gateStatusForPaths(nodeSpecPaths(sourceNode, edges, nodesById), gateStatusBySpec);
  if (!status) return null;
  return { status, color: status === 'pass' ? GATE_PASS_COLOR : GATE_FAIL_COLOR };
}

// ── Evaluative: spec approval (spec approval ledger) ─────────────────────

export const APPROVAL_ICONS = {
  approved: { icon: '\u2713', color: '#22c55e', label: 'Approved' }, // ✓
  pending: { icon: '\u23F3', color: '#eab308', label: 'Pending' },   // ⏳
  rejected: { icon: '\u2717', color: '#ef4444', label: 'Rejected' }, // ✗
};

/** Approval status derived from a single ledger record
 *  (GET /api/v1/specs/approvals — SpecApprovalResponse). */
export function approvalStatusForRecord(record) {
  if (!record) return null;
  if (record.active) return 'approved';
  if (record.revoked_at != null) {
    // Approval withdrawn — the spec is no longer approved, but it was not
    // rejected either; it goes back to awaiting approval.
    return 'pending';
  }
  // Inactive without revocation = rejected by a human reviewer.
  return 'rejected';
}

/** Approval status for a spec path from its full ledger history:
 *  any active record approves the path; otherwise the most recent record
 *  decides (rejected stays rejected, revoked returns to pending). */
export function approvalStatusForRecords(records = []) {
  let status = null;
  let latest = null;
  for (const r of records) {
    const s = approvalStatusForRecord(r);
    if (s === 'approved') return 'approved';
    if (!latest || (r.approved_at ?? 0) >= (latest.approved_at ?? 0)) latest = r;
  }
  if (latest) status = approvalStatusForRecord(latest);
  return status; // 'rejected' | 'pending' | null (no records at all)
}

/** Approval decoration for a spec-linked node: ✓ Approved / ⏳ Pending /
 *  ✗ Rejected icon from the ledger. Null for nodes with no spec linkage. */
export function approvalDecoration(node, approvalRecordsByPath = new Map(), edges = [], nodesById = new Map()) {
  const paths = nodeSpecPaths(node, edges, nodesById);
  if (paths.length === 0) return null;
  let status = null;
  for (const p of paths) {
    const records = approvalRecordsByPath.get(p);
    if (!records?.length) continue;
    const s = approvalStatusForRecords(records);
    if (s === 'rejected') return { status: 'rejected', ...APPROVAL_ICONS.rejected };
    if (s === 'approved') status = 'approved';
    else if (s === 'pending' && !status) status = 'pending';
  }
  if (!status) {
    // Spec-linked but no ledger record at all → pending approval.
    return { status: 'pending', ...APPROVAL_ICONS.pending };
  }
  return { status, ...APPROVAL_ICONS[status] };
}

// ── Composed decorations (lenses compose on the structural base) ─────────

/** Full evaluative decoration set for a node: coverage badge, gate status
 *  ring, and spec approval icon composed on top of the structural base
 *  (HSI §3: "you can view the domain model (structural) with test coverage
 *  overlay (evaluative) to see which types have tests?"). */
export function evaluativeNodeDecorations(node, ctx = {}) {
  if (!node) return null;
  const { gateStatusBySpec = {}, approvalRecordsByPath = new Map(), edges = [], nodesById = new Map() } = ctx;
  const coverage = coverageDecoration(node);
  const gate = gateStatusDecoration(node, gateStatusBySpec, edges, nodesById);
  const approval = approvalDecoration(node, approvalRecordsByPath, edges, nodesById);
  if (!coverage && !gate && !approval) return null;
  return { coverage, gate, approval };
}

// ── URL persistence (?lens=evaluative) ───────────────────────────────────

/** Read the lens from a URL's `lens` query param (validated + normalized).
 *  Accepts a URL string or a Location-like object. */
export function lensFromUrl(url) {
  try {
    const u = typeof url === 'string' ? new URL(url) : new URL(url?.href ?? 'http://x/');
    return normalizeLens(u.searchParams.get('lens'));
  } catch {
    return 'structural';
  }
}

/** Encode a lens onto a URL string, preserving all other params/hash.
 *  Returns the new URL string (caller owns history.replaceState). */
export function lensToUrl(lens, url) {
  const u = typeof url === 'string' ? new URL(url) : new URL(url?.href ?? 'http://x/');
  const normalized = normalizeLens(lens);
  if (normalized === 'structural') {
    // Default lens: keep URLs clean — drop the param instead of ?lens=structural
    u.searchParams.delete('lens');
  } else {
    u.searchParams.set('lens', normalized);
  }
  return u.toString();
}
