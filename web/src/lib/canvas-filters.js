// ExplorerCanvas filter mapping — which edge types render and which layout
// nodes stay lit under each toolbar filter value. Extracted from
// ExplorerCanvas.svelte (mirrors the F3 pattern that extracted
// test-reachability.js) so the filter→edge-type and filter→node-dimming
// mappings are unit-testable. ExplorerCanvas consumes the same code path.
//
// Task-065 F1: node-side dimming was inline in ExplorerCanvas.svelte with no
// test pinning it — a mutation reverting the 'dependencies' case to
// unconditional dimming passed the whole suite. Extracted here so the
// semantics are unit-testable and the component consumes the same code path.

// Set of node ids participating in edges of the given type (either endpoint).
// Mirrors ExplorerCanvas's edgeParticipants().
export function collectEdgeParticipants(edges, type) {
  const s = new Set();
  for (const e of edges) {
    const et = (e.edge_type ?? e.type ?? '').toLowerCase();
    if (et === type) {
      const src = e.source_id ?? e.from_node_id ?? e.from;
      const tgt = e.target_id ?? e.to_node_id ?? e.to;
      if (src) s.add(src);
      if (tgt) s.add(tgt);
    }
  }
  return s;
}

// Opacity for a layout node under `filter`. Layout nodes carry `kind`
// ('tree-group' | 'leaf') and a `node` (graph node) when they wrap one.
// Tree-group containers never dim (1.0); leaf nodes dim to 0.1 unless they
// participate in the filter's subject matter.
export function nodeFilterOpacity(filter, ln, participants) {
  if (ln.kind === 'tree-group') return 1.0;
  if (!ln.node) return 0.1;
  if (filter === 'all') return 1.0;
  switch (filter) {
    case 'endpoints': return ln.node.node_type === 'endpoint' ? 1.0 : 0.1;
    case 'types': return (ln.node.node_type === 'type' || ln.node.node_type === 'interface' || ln.node.node_type === 'field') ? 1.0 : 0.1;
    case 'calls': return participants.calls.has(ln.node.id) ? 1.0 : 0.1;
    case 'dependencies': return (participants.dependencies.has(ln.node.id) || participants.calls.has(ln.node.id)) ? 1.0 : 0.1;
    default: return 1.0;
  }
}

// Returns true when an edge of `edgeType` renders under `filter`.
// `edgeType` is normalized to lowercase here.
export function edgePassesFilter(filter, edgeType) {
  if (filter === 'all') return true;
  const et = (edgeType ?? '').toLowerCase();
  switch (filter) {
    case 'endpoints': return et === 'calls' || et === 'routes_to';
    case 'types': return et === 'field_of' || et === 'depends_on';
    case 'calls': return et === 'calls';
    case 'dependencies': return et === 'depends_on' || et === 'calls';
    default: return true;
  }
}
