/**
 * causal-trace.js — Causal Flow Trace computation (explorer-canvas.md §6).
 *
 * Pure helpers for the "Trace from here" structural-lens feature: BFS
 * traversal over outgoing `Calls`/`RoutesTo` edges from a starting node,
 * producing an execution-ordered step list with governing-spec annotations.
 *
 * Kept free of component/state dependencies so the traversal contract
 * (ordering, cycle safety, spec resolution) is directly unit-testable.
 */

// Edge field normalization (mirrors ExplorerCanvas edgeType/edgeSrc/edgeTgt):
// the API can serialize edges under different field names per path.
export function traceEdgeType(e) {
  return (e?.edge_type ?? e?.type ?? '').toLowerCase();
}

export function traceEdgeSrc(e) {
  return e?.source_id ?? e?.from_node_id ?? e?.from;
}

export function traceEdgeTgt(e) {
  return e?.target_id ?? e?.to_node_id ?? e?.to;
}

/** Causal edges per spec: outgoing `Calls` and `RoutesTo` relationships. */
export function isCausalEdgeType(et) {
  const t = (et ?? '').toLowerCase();
  return t === 'calls' || t === 'routes_to' || t === 'routesto';
}

/**
 * Whether the loaded graph has any non-deleted causal (`calls`/`routes_to`)
 * edges. When zero, trace mode cannot show a flow and the UI must warn:
 * "Call graph not available — run code sync to extract call edges".
 */
export function hasCausalEdges(edges) {
  for (const e of edges ?? []) {
    if (e?.deleted_at) continue;
    if (isCausalEdgeType(traceEdgeType(e))) return true;
  }
  return false;
}

/**
 * Compute the causal flow trace from `rootId` via BFS over outgoing
 * calls/routes_to edges.
 *
 * BFS ordering matches execution flow breadth: all direct callees of a
 * function are numbered before second-level callees. Siblings are sorted
 * by node name for deterministic ordering (static call graphs lack
 * intra-function call ordering). Cycle-safe via a visited set.
 *
 * @returns {{ order: Map<string, number>, pathEdges: Array<{fromId: string, toId: string, edgeType: string, fromStep: number, toStep: number}> }}
 *   `order` maps nodeId → 1-based step number; `pathEdges` lists the
 *   traversed causal edges in step order. Unknown root → empty result.
 */
export function computeTraceOrder(nodes, edges, rootId) {
  const empty = { order: new Map(), pathEdges: [] };
  if (!rootId) return empty;

  const nodeIds = new Set((nodes ?? []).map(n => n.id));
  if (!nodeIds.has(rootId)) return empty;

  // Outgoing causal adjacency, restricted to known nodes.
  const outAdj = new Map();
  for (const e of edges ?? []) {
    if (e?.deleted_at) continue;
    const et = traceEdgeType(e);
    if (!isCausalEdgeType(et)) continue;
    const srcId = traceEdgeSrc(e);
    const tgtId = traceEdgeTgt(e);
    if (!nodeIds.has(srcId) || !nodeIds.has(tgtId)) continue;
    if (!outAdj.has(srcId)) outAdj.set(srcId, []);
    outAdj.get(srcId).push({ toId: tgtId, edgeType: et });
  }

  const nameOf = new Map((nodes ?? []).map(n => [n.id, n.name ?? n.qualified_name ?? '']));

  const order = new Map();
  const pathEdges = [];
  const visited = new Set([rootId]);
  let step = 1;
  order.set(rootId, step);

  const queue = [rootId];
  while (queue.length > 0) {
    const nodeId = queue.shift();
    const children = outAdj.get(nodeId) ?? [];
    // Deterministic sibling ordering by callee name.
    const sorted = [...children].sort((a, b) =>
      (nameOf.get(a.toId) ?? '').localeCompare(nameOf.get(b.toId) ?? ''));
    for (const child of sorted) {
      if (visited.has(child.toId)) continue;
      visited.add(child.toId);
      step++;
      order.set(child.toId, step);
      pathEdges.push({
        fromId: nodeId,
        toId: child.toId,
        edgeType: child.edgeType,
        fromStep: order.get(nodeId),
        toStep: step,
      });
      queue.push(child.toId);
    }
  }

  return { order, pathEdges };
}

/**
 * Governing spec for a node: explicit `spec_path` field, else the target of
 * an outgoing `governed_by` edge (spec path or spec node id), else null.
 */
export function governingSpecFor(node, edges) {
  if (!node) return null;
  if (node.spec_path) return node.spec_path;
  for (const e of edges ?? []) {
    if (e?.deleted_at) continue;
    if (traceType(e) !== 'governed_by') continue;
    if (traceEdgeSrc(e) !== node.id) continue;
    const tgt = traceEdgeTgt(e);
    if (tgt) return tgt;
  }
  return null;
}

function traceType(e) {
  return (e?.edge_type ?? e?.type ?? '').toLowerCase();
}

/**
 * Build the detail-panel step list for an active trace.
 *
 * @param {Array} nodes graph nodes
 * @param {Array} edges graph edges (for governed_by spec resolution)
 * @param {Map<string, number>} order nodeId → 1-based step number
 * @param {Array} pathEdges traversed causal edges (from computeTraceOrder)
 * @returns {Array<{step: number, nodeId: string, name: string, qualifiedName: string, type: string, specPath: string|null, outEdges: Array}>}
 *   Sorted by step number.
 */
export function buildTraceSteps(nodes, edges, order, pathEdges) {
  if (!order || order.size === 0) return [];
  const nodeMap = new Map((nodes ?? []).map(n => [n.id, n]));
  const steps = [];
  for (const [nodeId, stepNum] of order) {
    const n = nodeMap.get(nodeId);
    if (!n) continue;
    const outEdges = (pathEdges ?? []).filter(e => e.fromId === nodeId);
    steps.push({
      step: stepNum,
      nodeId,
      name: n.name ?? n.qualified_name ?? '?',
      qualifiedName: n.qualified_name ?? '',
      type: n.node_type ?? '',
      specPath: governingSpecFor(n, edges),
      outEdges: outEdges.map(e => ({ toId: e.toId, edgeType: e.edgeType, toStep: e.toStep })),
    });
  }
  steps.sort((a, b) => a.step - b.step);
  return steps;
}
