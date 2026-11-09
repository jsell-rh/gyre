import { describe, it, expect } from 'vitest';
import {
  isCausalEdgeType,
  hasCausalEdges,
  computeTraceOrder,
  governingSpecFor,
  buildTraceSteps,
} from '../lib/causal-trace.js';

// ── Test graph ────────────────────────────────────────────────────────────
//
//   fnA ──calls──▶ fnB ──calls──▶ fnD
//    │
//    └─calls──▶ fnC ──calls──▶ fnE
//
//   (fnZ is disconnected; specNode is a GovernedBy target)

const NODES = [
  { id: 'fnA', node_type: 'function', name: 'fnA', qualified_name: 'app.fnA' },
  { id: 'fnB', node_type: 'function', name: 'fnB', qualified_name: 'app.fnB' },
  { id: 'fnC', node_type: 'function', name: 'fnC', qualified_name: 'app.fnC' },
  { id: 'fnD', node_type: 'function', name: 'fnD', qualified_name: 'app.fnD' },
  { id: 'fnE', node_type: 'function', name: 'fnE', qualified_name: 'app.fnE' },
  { id: 'fnZ', node_type: 'function', name: 'fnZ', qualified_name: 'app.fnZ' },
  { id: 'ep1', node_type: 'endpoint', name: 'GET /x', qualified_name: 'app.ep1' },
];

const EDGES = [
  { id: 'e1', source_id: 'fnA', target_id: 'fnB', edge_type: 'calls' },
  { id: 'e2', source_id: 'fnA', target_id: 'fnC', edge_type: 'calls' },
  { id: 'e3', source_id: 'fnB', target_id: 'fnD', edge_type: 'calls' },
  { id: 'e4', source_id: 'fnC', target_id: 'fnE', edge_type: 'calls' },
  { id: 'e5', source_id: 'ep1', target_id: 'fnA', edge_type: 'routes_to' },
  // Non-causal edges must be ignored by the traversal
  { id: 'e6', source_id: 'fnB', target_id: 'fnZ', edge_type: 'contains' },
  { id: 'e7', source_id: 'fnA', target_id: 'specNode', edge_type: 'governed_by' },
];

describe('isCausalEdgeType', () => {
  it('accepts calls and routes_to (and serialization variants)', () => {
    expect(isCausalEdgeType('calls')).toBe(true);
    expect(isCausalEdgeType('Calls')).toBe(true);
    expect(isCausalEdgeType('CALLS')).toBe(true);
    expect(isCausalEdgeType('routes_to')).toBe(true);
    expect(isCausalEdgeType('routesto')).toBe(true);
  });

  it('rejects non-causal edge types', () => {
    expect(isCausalEdgeType('contains')).toBe(false);
    expect(isCausalEdgeType('governed_by')).toBe(false);
    expect(isCausalEdgeType('implements')).toBe(false);
    expect(isCausalEdgeType('')).toBe(false);
    expect(isCausalEdgeType(null)).toBe(false);
  });
});

describe('hasCausalEdges', () => {
  it('true when calls edges exist', () => {
    expect(hasCausalEdges(EDGES)).toBe(true);
  });

  it('false for graphs without calls/routes_to edges', () => {
    expect(hasCausalEdges([EDGES[5], EDGES[6]])).toBe(false);
    expect(hasCausalEdges([])).toBe(false);
    expect(hasCausalEdges(null)).toBe(false);
  });

  it('ignores deleted edges', () => {
    const deleted = EDGES.slice(0, 4).map(e => ({ ...e, deleted_at: 123 }));
    expect(hasCausalEdges([...deleted, EDGES[5]])).toBe(false);
  });
});

describe('computeTraceOrder', () => {
  it('numbers nodes in BFS execution order (siblings before subtrees)', () => {
    const { order } = computeTraceOrder(NODES, EDGES, 'fnA');
    // BFS: root 1, then all direct callees (2, 3), then second level (4, 5).
    // DFS would number fnD=3 before fnC — this assertion kills that bug.
    expect(order.get('fnA')).toBe(1);
    expect(order.get('fnB')).toBe(2);
    expect(order.get('fnC')).toBe(3);
    expect(order.get('fnD')).toBe(4);
    expect(order.get('fnE')).toBe(5);
    expect(order.size).toBe(5);
  });

  it('follows routes_to edges as causal flow', () => {
    const { order } = computeTraceOrder(NODES, EDGES, 'ep1');
    expect(order.get('ep1')).toBe(1);
    expect(order.get('fnA')).toBe(2);
    expect(order.get('fnB')).toBe(3);
  });

  it('does not traverse non-causal edge types', () => {
    const { order } = computeTraceOrder(NODES, EDGES, 'fnB');
    // fnB → fnD via calls; fnB → fnZ via contains must NOT match
    expect(order.get('fnB')).toBe(1);
    expect(order.get('fnD')).toBe(2);
    expect(order.has('fnZ')).toBe(false);
  });

  it('returns path edges in step order with from/to steps', () => {
    const { pathEdges } = computeTraceOrder(NODES, EDGES, 'fnA');
    expect(pathEdges).toHaveLength(4);
    const bd = pathEdges.find(e => e.fromId === 'fnB' && e.toId === 'fnD');
    expect(bd).toEqual({ fromId: 'fnB', toId: 'fnD', edgeType: 'calls', fromStep: 2, toStep: 4 });
  });

  it('sorts siblings by name for deterministic ordering', () => {
    // Reverse insertion order of the two direct callees; ordering must stay stable
    const shuffled = [EDGES[1], EDGES[0], EDGES[2], EDGES[3], EDGES[4]];
    const { order } = computeTraceOrder(NODES, shuffled, 'fnA');
    expect([...order.entries()].map(([id, step]) => [id, step])).toEqual([
      ['fnA', 1], ['fnB', 2], ['fnC', 3], ['fnD', 4], ['fnE', 5],
    ]);
  });

  it('terminates on cyclic call graphs', () => {
    const cyclic = [
      { source_id: 'fnA', target_id: 'fnB', edge_type: 'calls' },
      { source_id: 'fnB', target_id: 'fnA', edge_type: 'calls' },
      { source_id: 'fnB', target_id: 'fnC', edge_type: 'calls' },
    ];
    const { order, pathEdges } = computeTraceOrder(NODES, cyclic, 'fnA');
    expect(order.get('fnA')).toBe(1);
    expect(order.get('fnB')).toBe(2);
    expect(order.get('fnC')).toBe(3);
    expect(order.size).toBe(3);
    expect(pathEdges).toHaveLength(2);
  });

  it('empty result for unknown root or missing root', () => {
    expect(computeTraceOrder(NODES, EDGES, 'nope').order.size).toBe(0);
    expect(computeTraceOrder(NODES, EDGES, null).order.size).toBe(0);
    expect(computeTraceOrder(NODES, EDGES, undefined).order.size).toBe(0);
  });

  it('leaf node traces to itself only', () => {
    const { order, pathEdges } = computeTraceOrder(NODES, EDGES, 'fnZ');
    expect([...order.entries()]).toEqual([['fnZ', 1]]);
    expect(pathEdges).toHaveLength(0);
  });

  it('ignores edges whose endpoints are not in the node list', () => {
    const dangling = [...EDGES, { source_id: 'fnA', target_id: 'ghost', edge_type: 'calls' }];
    const { order } = computeTraceOrder(NODES, dangling, 'fnA');
    expect(order.has('ghost')).toBe(false);
    expect(order.size).toBe(5);
  });
});

describe('governingSpecFor', () => {
  it('prefers the explicit spec_path field', () => {
    expect(governingSpecFor({ id: 'fnA', spec_path: 'specs/system/x.md' }, EDGES))
      .toBe('specs/system/x.md');
  });

  it('falls back to the governed_by edge target when spec_path is absent', () => {
    expect(governingSpecFor({ id: 'fnA' }, EDGES)).toBe('specNode');
  });

  it('null when no spec governs the node', () => {
    expect(governingSpecFor({ id: 'fnB' }, EDGES)).toBeNull();
    expect(governingSpecFor(null, EDGES)).toBeNull();
  });

  it('ignores governed_by edges from other nodes and deleted edges', () => {
    expect(governingSpecFor({ id: 'fnB' }, [
      { source_id: 'fnA', target_id: 'specNode', edge_type: 'governed_by' },
    ])).toBeNull();
    expect(governingSpecFor({ id: 'fnA' }, [
      { source_id: 'fnA', target_id: 'specNode', edge_type: 'governed_by', deleted_at: 1 },
    ])).toBeNull();
  });
});

describe('buildTraceSteps', () => {
  it('builds a step list sorted by step number with spec annotations', () => {
    const { order, pathEdges } = computeTraceOrder(NODES, EDGES, 'fnA');
    const steps = buildTraceSteps(NODES, EDGES, order, pathEdges);
    expect(steps.map(s => s.name)).toEqual(['fnA', 'fnB', 'fnC', 'fnD', 'fnE']);
    expect(steps.map(s => s.step)).toEqual([1, 2, 3, 4, 5]);
    // fnA is governed by specNode via its governed_by edge
    expect(steps[0].specPath).toBe('specNode');
    expect(steps[1].specPath).toBeNull();
    expect(steps[0].type).toBe('function');
    expect(steps[0].qualifiedName).toBe('app.fnA');
  });

  it('steps carry outgoing edge info with target step numbers', () => {
    const { order, pathEdges } = computeTraceOrder(NODES, EDGES, 'fnA');
    const steps = buildTraceSteps(NODES, EDGES, order, pathEdges);
    const stepB = steps.find(s => s.nodeId === 'fnB');
    expect(stepB.outEdges).toEqual([{ toId: 'fnD', edgeType: 'calls', toStep: 4 }]);
  });

  it('empty for empty order', () => {
    expect(buildTraceSteps(NODES, EDGES, new Map(), [])).toEqual([]);
    expect(buildTraceSteps(NODES, EDGES, null, null)).toEqual([]);
  });
});
