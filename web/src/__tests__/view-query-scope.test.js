/**
 * view-query-scope unit tests (ui-layout.md §10): the projection the
 * Explorer shell applies before handing graph data to the registry layout
 * renderers. Semantics mirror ExplorerCanvas's query matching for the
 * scope shapes a generated ViewSpec can produce.
 */
import { describe, it, expect } from 'vitest';
import { resolveScopeNodes, edgeSrc, edgeTgt } from '../lib/view-query-scope.js';

const NODES = [
  { id: 'fn1', name: 'AuthService', node_type: 'function', spec_path: null },
  { id: 'fn2', name: 'TokenService', node_type: 'function', spec_path: 'specs/auth.md' },
  { id: 'ty1', name: 'UserModel', node_type: 'type', qualified_name: 'models::UserModel' },
  { id: 'sp1', name: 'specs/auth.md', node_type: 'spec', spec_path: 'specs/auth.md' },
];

const EDGES = [
  { id: 'e1', source_id: 'fn1', target_id: 'fn2', edge_type: 'calls' },
  { id: 'e2', source_id: 'fn2', target_id: 'ty1', edge_type: 'calls' },
  { id: 'e3', source_id: 'fn2', target_id: 'sp1', edge_type: 'governed_by' },
  { id: 'e4', source_id: 'ty1', target_id: 'fn1', edge_type: 'contains' },
];

describe('resolveScopeNodes', () => {
  it('returns null for absent or unsupported scopes (caller shows the full graph)', () => {
    expect(resolveScopeNodes(null, NODES, EDGES)).toBeNull();
    expect(resolveScopeNodes(undefined, NODES, EDGES)).toBeNull();
    // focus/test_gaps/diff scopes are canvas-internal (not producible from
    // a generated ViewSpec) — the shell must not guess at their semantics.
    expect(resolveScopeNodes({ type: 'focus', node: 'x' }, NODES, EDGES)).toBeNull();
    expect(resolveScopeNodes({ type: 'test_gaps' }, NODES, EDGES)).toBeNull();
  });

  it("scope 'all' returns every node", () => {
    expect(resolveScopeNodes({ type: 'all' }, NODES, EDGES)).toBe(NODES);
  });

  it("filter scope matches node_types (normalized lowercase, like the canvas)", () => {
    const out = resolveScopeNodes(
      { type: 'filter', node_types: ['function'] },
      NODES,
      EDGES
    );
    expect(out.map((n) => n.id)).toEqual(['fn1', 'fn2']);
  });

  it('filter scope with name_pattern matches name and qualified_name', () => {
    const out = resolveScopeNodes(
      { type: 'filter', name_pattern: '^models::' },
      NODES,
      EDGES
    );
    expect(out.map((n) => n.id)).toEqual(['ty1']);
  });

  it("filter scope with an empty match returns null (canvas: unmatched scope → full graph)", () => {
    expect(
      resolveScopeNodes({ type: 'filter', node_types: ['endpoint'] }, NODES, EDGES)
    ).toBeNull();
  });

  it("computed $governed_by resolves spec_path matches and governed_by edges", () => {
    const out = resolveScopeNodes(
      { type: 'filter', computed: "$governed_by('specs/auth.md')" },
      NODES,
      EDGES
    );
    // fn2 carries the spec_path; sp1 is the spec node itself; fn2 also has
    // a governed_by edge into sp1 (same node). fn1/ty1 are not governed.
    expect(out.map((n) => n.id).sort()).toEqual(['fn2', 'sp1']);
  });

  it('computed expressions other than $governed_by return null (no guessed semantics)', () => {
    expect(
      resolveScopeNodes({ type: 'filter', computed: '$test_unreachable' }, NODES, EDGES)
    ).toBeNull();
  });

  it('concept scope expands from seeds along the allowed edges up to the depth', () => {
    // Default: 'calls' edges, depth 2, both directions.
    const out = resolveScopeNodes(
      { type: 'concept', seed_nodes: ['UserModel'] },
      NODES,
      EDGES
    );
    // ty1 (seed) ← calls ← fn2 (depth 1) ← calls ← fn1 (depth 2).
    // The 'contains' edge ty1→fn1 is not followed (not in expand_edges).
    expect(out.map((n) => n.id).sort()).toEqual(['fn1', 'fn2', 'ty1']);
  });

  it('concept scope honors expand_edges, direction and depth', () => {
    // Only outgoing 'contains' edges from the seed
    const out = resolveScopeNodes(
      {
        type: 'concept',
        seed_nodes: ['UserModel'],
        expand_edges: ['contains'],
        expand_direction: 'outgoing',
      },
      NODES,
      EDGES
    );
    expect(out.map((n) => n.id)).toEqual(['ty1', 'fn1']);
  });

  it('concept scope with an unmatched seed returns null', () => {
    expect(
      resolveScopeNodes({ type: 'concept', seed_nodes: ['Nope'] }, NODES, EDGES)
    ).toBeNull();
  });

  it("concept scope seed_match 'substring' seeds every node whose name/qualified_name contains the seed (ViewSpec data.concept mechanism)", () => {
    // 'auth' is no node's name — exact matching resolves nothing. As a
    // substring it matches AuthService (fn1) and specs/auth.md (sp1) by
    // name, case-insensitively. depth 0: the concept is exactly the
    // matched set.
    const out = resolveScopeNodes(
      { type: 'concept', seed_nodes: ['auth'], seed_match: 'substring', expand_depth: 0 },
      NODES,
      EDGES
    );
    expect(out.map((n) => n.id).sort()).toEqual(['fn1', 'sp1']);
  });

  it("concept scope seed_match 'substring' seeds expand along edges like exact seeds", () => {
    // Seeds fn1 + sp1 (substring 'AUTH', case-insensitive), then the
    // default calls expansion: fn1 → fn2 (depth 1) → ty1 (depth 2).
    const out = resolveScopeNodes(
      { type: 'concept', seed_nodes: ['AUTH'], seed_match: 'substring' },
      NODES,
      EDGES
    );
    expect(out.map((n) => n.id).sort()).toEqual(['fn1', 'fn2', 'sp1', 'ty1']);
  });

  it("concept scope seed_match 'substring' with no match returns null (full graph, canvas parity)", () => {
    expect(
      resolveScopeNodes(
        { type: 'concept', seed_nodes: ['zzz'], seed_match: 'substring' },
        NODES,
        EDGES
      )
    ).toBeNull();
  });

  it("concept scope defaults to exact seed matching (seed_match absent)", () => {
    // A seed that is a substring of a node name but not a node name must
    // NOT seed that node without seed_match 'substring'.
    expect(
      resolveScopeNodes({ type: 'concept', seed_nodes: ['auth'] }, NODES, EDGES)
    ).toBeNull();
  });
});

describe('edge endpoint accessors', () => {
  it('reads the three serialized field spellings', () => {
    expect(edgeSrc({ source_id: 'a', target_id: 'b' })).toBe('a');
    expect(edgeTgt({ source_id: 'a', target_id: 'b' })).toBe('b');
    expect(edgeSrc({ from_node_id: 'a', to_node_id: 'b' })).toBe('a');
    expect(edgeTgt({ from_node_id: 'a', to_node_id: 'b' })).toBe('b');
    expect(edgeSrc({ from: 'a', to: 'b' })).toBe('a');
    expect(edgeTgt({ from: 'a', to: 'b' })).toBe('b');
  });
});
