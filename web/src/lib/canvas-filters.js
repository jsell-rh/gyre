// ExplorerCanvas edge-filter mapping — which edge types render under each
// toolbar filter value. Extracted from ExplorerCanvas.svelte's filterEdge so
// the filter→edge-type mapping is unit-testable (mirrors the F3 pattern that
// extracted test-reachability.js). ExplorerCanvas consumes the same code path.
//
// filterOpacity's node-dimming cases in ExplorerCanvas.svelte must stay in
// agreement with this mapping: every filter value handled here has a
// counterpart case there ('endpoints', 'types', 'calls', 'dependencies').

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
