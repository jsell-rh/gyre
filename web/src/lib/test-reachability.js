// Test-reachability computations for the explorer canvas.
//
// Implements the test-coverage computed references from
// view-query-grammar.md §3 — $test_reachable, $test_unreachable,
// $test_fragility(node) — and the §2 test_gaps scope. All functions are pure
// over (nodes, adjacency) so the semantics are unit-testable without the
// component, and ExplorerCanvas.svelte consumes the same code path.

// Edge types traversed for test reachability — matches backend
// TEST_REACHABILITY_EDGES (crates/gyre-domain/src/view_query_resolver.rs).
// Spec §3 defines $test_reachable as "nodes reachable from any test function
// via Calls". Implements, RoutesTo and Contains are intentionally excluded:
// a trait implementation or HTTP route does not prove a test executes the
// node, and Contains would make every sibling in a tested module "covered".
export const TEST_REACHABILITY_EDGES = new Set(['calls']);

// Node types that count as testable for coverage-gap analysis.
const TESTABLE_TYPES = new Set(['function', 'method', 'endpoint', 'type', 'trait', 'class']);

// Build the bidirectional adjacency map used by all traversals here and by
// ExplorerCanvas.svelte. Each edge contributes a forward entry at its source
// and a reversed entry at its target; BFS helpers treat `reverse: true`
// entries as incoming edges.
export function buildAdjacency(edges) {
  const adj = new Map();
  for (const e of edges) {
    const src = e.source_id ?? e.from_node_id ?? e.from;
    const tgt = e.target_id ?? e.to_node_id ?? e.to;
    const et = (e.edge_type ?? e.type ?? '').toLowerCase();
    if (src && tgt) {
      if (!adj.has(src)) adj.set(src, []);
      adj.get(src).push({ targetId: tgt, edgeType: et });
      if (!adj.has(tgt)) adj.set(tgt, []);
      adj.get(tgt).push({ targetId: src, edgeType: et, reverse: true });
    }
  }
  return adj;
}

// $test_reachable — every node id reachable from any test function via
// Calls (outgoing only). Includes the test nodes themselves.
export function computeTestReachable(nodes, adjacency) {
  const testN = nodes.filter(n => n.test_node);
  const reachable = new Set(testN.map(n => n.id));
  const q = [...reachable];
  while (q.length > 0) {
    const id = q.shift();
    for (const nb of (adjacency.get(id) ?? [])) {
      if (reachable.has(nb.targetId) || !TEST_REACHABILITY_EDGES.has(nb.edgeType) || nb.reverse) continue;
      reachable.add(nb.targetId);
      q.push(nb.targetId);
    }
  }
  return reachable;
}

// $test_unreachable — testable, non-test nodes NOT reachable from any test
// function via Calls.
export function computeTestUnreachable(nodes, adjacency) {
  const reachable = computeTestReachable(nodes, adjacency);
  const result = new Set();
  for (const n of nodes) {
    if (!n.test_node && TESTABLE_TYPES.has(n.node_type) && !reachable.has(n.id)) result.add(n.id);
  }
  return result;
}

// §2 test_gaps scope — Map of gap node id -> 0 (matched depth), or null when
// the graph has no coverage gaps. Matches the queryMatchedWithDepth contract.
export function computeTestGaps(nodes, adjacency) {
  const gaps = computeTestUnreachable(nodes, adjacency);
  if (gaps.size === 0) return null;
  const matched = new Map();
  for (const id of gaps) matched.set(id, 0);
  return matched;
}

// $test_fragility — for each node, the count of distinct test functions whose
// Calls-only traversal reaches it. O(T*(N+M)); callers cache the result.
export function computeTestFragilityCounts(nodes, adjacency) {
  const fragility = new Map(); // node_id -> count of distinct tests reaching it
  const testNodes = nodes.filter(n => n.test_node);
  for (const tn of testNodes) {
    const reached = new Set([tn.id]);
    const q = [tn.id];
    while (q.length > 0) {
      const id = q.shift();
      for (const nb of (adjacency.get(id) ?? [])) {
        if (TEST_REACHABILITY_EDGES.has(nb.edgeType) && !nb.reverse && !reached.has(nb.targetId)) {
          reached.add(nb.targetId);
          q.push(nb.targetId);
        }
      }
    }
    for (const id of reached) {
      fragility.set(id, (fragility.get(id) || 0) + 1);
    }
  }
  return fragility;
}
