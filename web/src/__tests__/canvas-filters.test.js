// Pins the ExplorerCanvas filterEdge mapping (extracted to canvas-filters.js)
// against the filter values handled by filterOpacity in ExplorerCanvas.svelte.
// task-062 F4: a refactor commit deleted the 'dependencies' case from
// filterEdge while its filterOpacity counterpart survived, so under
// filter='dependencies' ALL edges rendered (contains, governed_by, renders...)
// instead of only depends_on/calls. This test pins every filter value's
// edge-type set, including both included and excluded edge types.
import { describe, expect, it } from 'vitest';
import { edgePassesFilter } from '../lib/canvas-filters.js';

describe('edgePassesFilter (ExplorerCanvas filterEdge mapping)', () => {
  it('filter=all renders every edge type', () => {
    for (const et of ['calls', 'contains', 'depends_on', 'governed_by', 'renders', 'routes_to', 'field_of']) {
      expect(edgePassesFilter('all', et)).toBe(true);
    }
  });

  it("filter='calls' renders only calls edges", () => {
    expect(edgePassesFilter('calls', 'calls')).toBe(true);
    expect(edgePassesFilter('calls', 'contains')).toBe(false);
    expect(edgePassesFilter('calls', 'depends_on')).toBe(false);
  });

  it("filter='endpoints' renders calls and routes_to", () => {
    expect(edgePassesFilter('endpoints', 'calls')).toBe(true);
    expect(edgePassesFilter('endpoints', 'routes_to')).toBe(true);
    expect(edgePassesFilter('endpoints', 'contains')).toBe(false);
  });

  it("filter='types' renders field_of and depends_on", () => {
    expect(edgePassesFilter('types', 'field_of')).toBe(true);
    expect(edgePassesFilter('types', 'depends_on')).toBe(true);
    expect(edgePassesFilter('types', 'calls')).toBe(false);
  });

  it("filter='dependencies' renders depends_on and calls — not contains/governed_by/renders (task-062 F4 regression)", () => {
    // The F4 regression: filterEdge lost its 'dependencies' case and fell
    // through to `default: return true`, rendering ALL edge types while
    // filterOpacity still dims non-dependency nodes to 0.1.
    expect(edgePassesFilter('dependencies', 'depends_on')).toBe(true);
    expect(edgePassesFilter('dependencies', 'calls')).toBe(true);
    // Excluded edge types — the contrast that failed pre-fix:
    expect(edgePassesFilter('dependencies', 'contains')).toBe(false);
    expect(edgePassesFilter('dependencies', 'governed_by')).toBe(false);
    expect(edgePassesFilter('dependencies', 'renders')).toBe(false);
    expect(edgePassesFilter('dependencies', 'routes_to')).toBe(false);
    expect(edgePassesFilter('dependencies', 'field_of')).toBe(false);
  });

  it('normalizes edge_type casing like filterEdge did', () => {
    expect(edgePassesFilter('dependencies', 'Depends_On')).toBe(true);
    expect(edgePassesFilter('dependencies', 'CALLS')).toBe(true);
    expect(edgePassesFilter('dependencies', 'CONTAINS')).toBe(false);
  });

  it('unknown filter values render everything (default case)', () => {
    expect(edgePassesFilter('nonexistent', 'contains')).toBe(true);
  });
});
