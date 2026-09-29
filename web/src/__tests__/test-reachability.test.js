// Pins the frontend test-reachability semantics from view-query-grammar.md §3
// ("nodes reachable from any test function via Calls") against the real
// production module consumed by ExplorerCanvas.svelte. Mirrors the Rust
// pinning test test_reachability_is_calls_only_not_implements_or_routes_to
// (crates/gyre-domain/src/view_query_resolver.rs) so both surfaces are held
// to the identical spec contract.
import { describe, expect, it } from 'vitest';
import {
  buildAdjacency,
  computeTestFragilityCounts,
  computeTestGaps,
  computeTestReachable,
  computeTestUnreachable,
} from '../lib/test-reachability.js';

// Graph mirroring the Rust pinning test: t1 (test) reaches n1 via Calls,
// n2 via Implements only, n3 via RoutesTo only. pkg1 is a non-testable
// container node; n4 is an untested function with no edges at all.
const NODES = [
  { id: 'n1', node_type: 'function', name: 'called_fn', test_node: false },
  { id: 'n2', node_type: 'function', name: 'impl_only_fn', test_node: false },
  { id: 'n3', node_type: 'function', name: 'route_only_fn', test_node: false },
  { id: 'n4', node_type: 'function', name: 'orphan_fn', test_node: false },
  { id: 'pkg1', node_type: 'package', name: 'api', test_node: false },
  { id: 't1', node_type: 'function', name: 'test_fn', test_node: true },
];

const EDGES = [
  { id: 'e1', source_id: 't1', target_id: 'n1', edge_type: 'calls' },
  { id: 'e2', source_id: 't1', target_id: 'n2', edge_type: 'implements' },
  { id: 'e3', source_id: 't1', target_id: 'n3', edge_type: 'routes_to' },
  { id: 'e4', source_id: 'pkg1', target_id: 'n2', edge_type: 'contains' },
];

const adjacency = buildAdjacency(EDGES);

describe('test-reachability (view-query-grammar.md §3: via Calls)', () => {
  it('test reachability is Calls-only — Implements and RoutesTo paths from a test do not count', () => {
    const reachable = computeTestReachable(NODES, adjacency);
    expect(reachable.has('n1')).toBe(true);
    expect(reachable.has('n2')).toBe(false);
    expect(reachable.has('n3')).toBe(false);
  });

  it('test reachability follows transitive Calls chains from a test node', () => {
    // t1 -> n1 -> n5 (both Calls) must mark n5 reachable.
    const nodes = [
      ...NODES,
      { id: 'n5', node_type: 'function', name: 'transitive_fn', test_node: false },
    ];
    const edges = [
      ...EDGES,
      { id: 'e5', source_id: 'n1', target_id: 'n5', edge_type: 'calls' },
    ];
    const reachable = computeTestReachable(nodes, buildAdjacency(edges));
    expect(reachable.has('n5')).toBe(true);
    const unreachable = computeTestUnreachable(nodes, buildAdjacency(edges));
    expect(unreachable.has('n5')).toBe(false);
  });

  it('test unreachable contains Implements-only and RoutesTo-only nodes, not Calls-reached nodes', () => {
    const unreachable = computeTestUnreachable(NODES, adjacency);
    expect(unreachable.has('n2')).toBe(true);
    expect(unreachable.has('n3')).toBe(true);
    expect(unreachable.has('n1')).toBe(false);
  });

  it('test unreachable includes orphan functions with no test path and excludes test nodes and non-testable types', () => {
    const unreachable = computeTestUnreachable(NODES, adjacency);
    expect(unreachable.has('n4')).toBe(true);
    expect(unreachable.has('t1')).toBe(false);
    expect(unreachable.has('pkg1')).toBe(false);
  });

  it('test gaps maps each coverage-gap node id to zero depth, or null when no gaps', () => {
    const gaps = computeTestGaps(NODES, adjacency);
    expect(gaps).not.toBeNull();
    expect(gaps.get('n2')).toBe(0);
    expect(gaps.get('n3')).toBe(0);
    expect(gaps.get('n4')).toBe(0);
    expect(gaps.has('n1')).toBe(false);
    expect(gaps.has('t1')).toBe(false);
    expect(gaps.has('pkg1')).toBe(false);

    // Fully-covered graph: one test Calls the one function.
    const coveredNodes = [
      { id: 'f1', node_type: 'function', name: 'f', test_node: false },
      { id: 't1', node_type: 'function', name: 't', test_node: true },
    ];
    const coveredEdges = [{ id: 'ce1', source_id: 't1', target_id: 'f1', edge_type: 'calls' }];
    expect(computeTestGaps(coveredNodes, buildAdjacency(coveredEdges))).toBeNull();
  });

  it('test fragility counts distinct tests reaching a node via Calls only', () => {
    const fragility = computeTestFragilityCounts(NODES, adjacency);
    expect(fragility.get('n1')).toBe(1);
    expect(fragility.get('n2') ?? 0).toBe(0);
    expect(fragility.get('n3') ?? 0).toBe(0);
  });

  it('test fragility increments once per test, not once per path', () => {
    // Two tests each reach n1 via Calls (one directly, one through n5):
    // n1's fragility is 2, and a test reaching a node via two different
    // Calls routes still contributes 1.
    const nodes = [
      { id: 'n1', node_type: 'function', name: 'f1', test_node: false },
      { id: 'n5', node_type: 'function', name: 'f5', test_node: false },
      { id: 't1', node_type: 'function', name: 't1', test_node: true },
      { id: 't2', node_type: 'function', name: 't2', test_node: true },
    ];
    const edges = [
      { id: 'x1', source_id: 't1', target_id: 'n1', edge_type: 'calls' },
      { id: 'x2', source_id: 't2', target_id: 'n5', edge_type: 'calls' },
      { id: 'x3', source_id: 'n5', target_id: 'n1', edge_type: 'calls' },
    ];
    const fragility = computeTestFragilityCounts(nodes, buildAdjacency(edges));
    expect(fragility.get('n1')).toBe(2);
    expect(fragility.get('n5')).toBe(1);

    // Single test with two Calls routes to n1: still contributes 1.
    const edges2 = [
      { id: 'y1', source_id: 't1', target_id: 'n5', edge_type: 'calls' },
      { id: 'y2', source_id: 'n5', target_id: 'n1', edge_type: 'calls' },
      { id: 'y3', source_id: 't1', target_id: 'n1', edge_type: 'calls' },
    ];
    const fragility2 = computeTestFragilityCounts(nodes.slice(0, 3), buildAdjacency(edges2));
    expect(fragility2.get('n1')).toBe(1);
  });
});
