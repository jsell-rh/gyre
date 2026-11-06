// Pins the ExplorerCanvas filter mappings extracted to canvas-filters.js:
// - edgePassesFilter: which edge types render under each toolbar filter.
//   task-062 F4: a refactor commit deleted the 'dependencies' case from
//   filterEdge while its filterOpacity counterpart survived, so under
//   filter='dependencies' ALL edges rendered (contains, governed_by, renders...)
//   instead of only depends_on/calls.
// - nodeFilterOpacity: which layout nodes stay lit under each filter.
//   task-065 F1: node-side dimming was inline and unpinned — a mutation
//   reverting the 'dependencies' case to unconditional dimming passed the
//   whole suite.
import { describe, expect, it } from 'vitest';
import { edgePassesFilter, nodeFilterOpacity, collectEdgeParticipants } from '../lib/canvas-filters.js';

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

// ── nodeFilterOpacity (ExplorerCanvas filterOpacity mapping) ────────────

// Fixture edges: part1—part2 via calls; part1—lone1 via depends_on; lone2 isolated.
const F1_EDGES = [
  { id: 'e1', source_id: 'part1', target_id: 'part2', edge_type: 'calls' },
  { id: 'e2', source_id: 'part1', target_id: 'lone1', edge_type: 'depends_on' },
];

function leafOf(node) {
  // Layout-node shape ExplorerCanvas passes: leaf cells carry the graph node.
  return { kind: 'leaf', node, id: node.id };
}

describe('collectEdgeParticipants', () => {
  it('collects both endpoints of matching edges', () => {
    const calls = collectEdgeParticipants(F1_EDGES, 'calls');
    expect([...calls].sort()).toEqual(['part1', 'part2']);
    const deps = collectEdgeParticipants(F1_EDGES, 'depends_on');
    expect([...deps].sort()).toEqual(['lone1', 'part1']);
  });

  it('accepts from/to edge key aliases and lowercases edge_type', () => {
    const edges = [{ source_id: 'a', target_id: 'b', edge_type: 'depends_on' }, { from: 'b', to: 'c', type: 'Depends_On' }];
    expect([...collectEdgeParticipants(edges, 'depends_on')].sort()).toEqual(['a', 'b', 'c']);
  });

  it('excludes non-matching edge types', () => {
    const edges = [{ source_id: 'a', target_id: 'b', edge_type: 'contains' }];
    expect(collectEdgeParticipants(edges, 'depends_on').size).toBe(0);
  });
});

describe('nodeFilterOpacity (ExplorerCanvas filterOpacity mapping)', () => {
  const PARTICIPANTS = {
    calls: collectEdgeParticipants(F1_EDGES, 'calls'),
    dependencies: collectEdgeParticipants(F1_EDGES, 'depends_on'),
  };
  const fn = (id) => leafOf({ id, node_type: 'function' });

  it('tree-group containers never dim', () => {
    expect(nodeFilterOpacity('dependencies', { kind: 'tree-group', node: null }, PARTICIPANTS)).toBe(1.0);
    expect(nodeFilterOpacity('calls', { kind: 'tree-group', node: null }, PARTICIPANTS)).toBe(1.0);
  });

  it("filter='all' keeps every leaf lit", () => {
    for (const id of ['part1', 'part2', 'lone1', 'lone2']) {
      expect(nodeFilterOpacity('all', fn(id), PARTICIPANTS)).toBe(1.0);
    }
  });

  it("filter='endpoints' lights only endpoint-typed nodes", () => {
    const ep = leafOf({ id: 'x', node_type: 'endpoint' });
    expect(nodeFilterOpacity('endpoints', ep, PARTICIPANTS)).toBe(1.0);
    expect(nodeFilterOpacity('endpoints', fn('part1'), PARTICIPANTS)).toBe(0.1);
  });

  it("filter='types' lights type/interface/field nodes", () => {
    for (const t of ['type', 'interface', 'field']) {
      expect(nodeFilterOpacity('types', leafOf({ id: 'x', node_type: t }), PARTICIPANTS)).toBe(1.0);
    }
    expect(nodeFilterOpacity('types', fn('part1'), PARTICIPANTS)).toBe(0.1);
  });

  it("filter='calls' lights calls participants, dims non-participants", () => {
    expect(nodeFilterOpacity('calls', fn('part1'), PARTICIPANTS)).toBe(1.0);
    expect(nodeFilterOpacity('calls', fn('part2'), PARTICIPANTS)).toBe(1.0);
    expect(nodeFilterOpacity('calls', fn('lone1'), PARTICIPANTS)).toBe(0.1); // depends_on participant, but not a calls participant
    expect(nodeFilterOpacity('calls', fn('lone2'), PARTICIPANTS)).toBe(0.1);
  });

  it("filter='dependencies' lights depends_on AND calls participants — task-065 F1: reverting this to unconditional 0.1 dims every node (dependencies view goes dark)", () => {
    // part1/part2: calls participants. lone1: depends_on participant only.
    // All three are dependency participants and must stay lit.
    expect(nodeFilterOpacity('dependencies', fn('part1'), PARTICIPANTS)).toBe(1.0);
    expect(nodeFilterOpacity('dependencies', fn('part2'), PARTICIPANTS)).toBe(1.0);
    expect(nodeFilterOpacity('dependencies', fn('lone1'), PARTICIPANTS)).toBe(1.0); // depends_on-only participant
    // lone2: no dependency edges at all → dimmed.
    expect(nodeFilterOpacity('dependencies', fn('lone2'), PARTICIPANTS)).toBe(0.1);
  });

  it('layout nodes without a graph node render at 0.1', () => {
    expect(nodeFilterOpacity('calls', { kind: 'leaf', node: null }, PARTICIPANTS)).toBe(0.1);
  });

  it('unknown filter values keep everything lit (default case)', () => {
    expect(nodeFilterOpacity('nonexistent', fn('lone2'), PARTICIPANTS)).toBe(1.0);
  });
});
