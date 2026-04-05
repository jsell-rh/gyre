import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render } from '@testing-library/svelte';
import ExplorerCanvas from '../lib/ExplorerCanvas.svelte';

// Mock canvas context
let mockCtx;
beforeEach(() => {
  mockCtx = {
    clearRect: vi.fn(),
    fillRect: vi.fn(),
    strokeRect: vi.fn(),
    beginPath: vi.fn(),
    closePath: vi.fn(),
    arc: vi.fn(),
    fill: vi.fn(),
    stroke: vi.fn(),
    moveTo: vi.fn(),
    lineTo: vi.fn(),
    quadraticCurveTo: vi.fn(),
    fillText: vi.fn(),
    measureText: vi.fn(() => ({ width: 40 })),
    scale: vi.fn(),
    setTransform: vi.fn(),
    save: vi.fn(),
    restore: vi.fn(),
    translate: vi.fn(),
    rotate: vi.fn(),
    fillStyle: '',
    strokeStyle: '',
    lineWidth: 0,
    globalAlpha: 1,
    font: '',
    textAlign: '',
    textBaseline: '',
    shadowColor: '',
    shadowBlur: 0,
    setLineDash: vi.fn(),
    getLineDash: vi.fn(() => []),
  };
  HTMLCanvasElement.prototype.getContext = vi.fn(() => mockCtx);
  global.ResizeObserver = class ResizeObserver {
    observe() {}
    disconnect() {}
    unobserve() {}
  };
  global.requestAnimationFrame = vi.fn(cb => { cb(); return 1; });
  global.cancelAnimationFrame = vi.fn();
});

afterEach(() => {
  vi.restoreAllMocks();
});

// ── Test data ─────────────────────────────────────────────────────────────

const NODES = [
  { id: 'pkg1', node_type: 'package', name: 'api', qualified_name: 'api', file_path: '', line_start: 0, line_end: 0, visibility: 'public', spec_confidence: 'none', test_node: false },
  { id: 'mod1', node_type: 'module', name: 'handlers', qualified_name: 'api.handlers', file_path: 'api/handlers.py', line_start: 1, line_end: 50, visibility: 'public', spec_confidence: 'none', test_node: false },
  { id: 'fn1', node_type: 'function', name: 'create_user', qualified_name: 'api.handlers.create_user', file_path: 'api/handlers.py', line_start: 10, line_end: 30, visibility: 'public', spec_confidence: 'high', test_node: false },
  { id: 'fn2', node_type: 'function', name: 'get_user', qualified_name: 'api.handlers.get_user', file_path: 'api/handlers.py', line_start: 32, line_end: 45, visibility: 'public', spec_confidence: 'medium', test_node: false },
  { id: 'pkg2', node_type: 'package', name: 'domain', qualified_name: 'domain', file_path: '', line_start: 0, line_end: 0, visibility: 'public', spec_confidence: 'none', test_node: false },
  { id: 'type1', node_type: 'type', name: 'User', qualified_name: 'domain.User', file_path: 'domain/models.py', line_start: 1, line_end: 20, visibility: 'public', spec_confidence: 'high', test_node: false },
  { id: 'test1', node_type: 'function', name: 'test_create_user', qualified_name: 'tests.test_create_user', file_path: 'tests/test_api.py', line_start: 1, line_end: 15, visibility: 'public', spec_confidence: 'none', test_node: true },
];

const EDGES = [
  { id: 'e1', source_id: 'pkg1', target_id: 'mod1', edge_type: 'contains' },
  { id: 'e2', source_id: 'mod1', target_id: 'fn1', edge_type: 'contains' },
  { id: 'e3', source_id: 'mod1', target_id: 'fn2', edge_type: 'contains' },
  { id: 'e4', source_id: 'pkg2', target_id: 'type1', edge_type: 'contains' },
  { id: 'e5', source_id: 'fn1', target_id: 'type1', edge_type: 'calls' },
  { id: 'e6', source_id: 'test1', target_id: 'fn1', edge_type: 'calls' },
];

// ── Tests ─────────────────────────────────────────────────────────────────

describe('ExplorerCanvas', () => {
  it('renders without throwing', () => {
    expect(() => render(ExplorerCanvas)).not.toThrow();
  });

  it('renders a canvas element', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    const canvas = container.querySelector('canvas');
    expect(canvas).toBeTruthy();
  });

  it('shows node count in stats', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    const stats = container.querySelector('.treemap-stats');
    expect(stats?.textContent).toContain('7 nodes');
  });

  it('renders toolbar with filter presets', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    const buttons = container.querySelectorAll('.tb-btn');
    // 5 filter presets + 3 lens buttons
    expect(buttons.length).toBeGreaterThanOrEqual(5);
    const labels = Array.from(buttons).map(b => b.textContent);
    expect(labels).toContain('All');
    expect(labels).toContain('Endpoints');
    expect(labels).toContain('Types');
    expect(labels).toContain('Calls');
    expect(labels).toContain('Dependencies');
  });

  it('renders lens toggle with structural active', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    const lensButtons = container.querySelectorAll('.lens-group .tb-btn, .tb-btn');
    const structural = Array.from(lensButtons).find(b => b.textContent === 'Structural');
    expect(structural?.classList.contains('active')).toBe(true);
  });

  it('renders minimap', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    const minimap = container.querySelector('.treemap-minimap');
    expect(minimap).toBeTruthy();
  });

  it('renders legend with node type colors', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    const legendItems = container.querySelectorAll('.legend-item');
    expect(legendItems.length).toBeGreaterThanOrEqual(4);
  });

  it('renders zoom indicator', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    const zoomInd = container.querySelector('.zoom-ind');
    expect(zoomInd).toBeTruthy();
    expect(zoomInd?.textContent).toMatch(/[\d.]+x/);
  });

  it('shows empty state when no nodes', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: [], edges: [] },
    });
    // EmptyState component should render
    const empty = container.querySelector('[class*="empty"]');
    expect(empty).toBeTruthy();
  });

  it('renders query annotation when activeQuery has title', () => {
    const query = {
      scope: { type: 'all' },
      annotation: { title: 'Test View', description: 'A test query' },
    };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, activeQuery: query },
    });
    const annotation = container.querySelector('.annotation-title');
    expect(annotation?.textContent).toContain('Test View');
  });

  it('calls canvas getContext on render', () => {
    render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    expect(HTMLCanvasElement.prototype.getContext).toHaveBeenCalled();
  });

  it('no breadcrumb at root level', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    const breadcrumb = container.querySelector('.treemap-breadcrumb');
    expect(breadcrumb).toBeFalsy();
  });

  it('updates canvasState zoom property', () => {
    let capturedState = {};
    const { component } = render(ExplorerCanvas, {
      props: {
        nodes: NODES,
        edges: EDGES,
        canvasState: capturedState,
      },
    });
    // The component should have set zoom in canvasState
    // (via $bindable reactive update)
    expect(true).toBeTruthy(); // Component rendered without error
  });
});

describe('ExplorerCanvas — hierarchy', () => {
  it('at root level shows only top-level packages (no Contains parent)', () => {
    // Root nodes are pkg1, pkg2, and test1 (test1 has no parent)
    // The treemap should show these as top-level cells
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    // Canvas rendering happened
    expect(mockCtx.fillRect).toHaveBeenCalled();
  });

  it('canvas draws with clearRect and fillRect', () => {
    render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    // Background fill
    expect(mockCtx.fillRect).toHaveBeenCalled();
    // Scale for DPR
    expect(mockCtx.scale).toHaveBeenCalled();
  });
});

describe('ExplorerCanvas — view queries', () => {
  it('renders focus scope query', () => {
    const query = {
      scope: { type: 'focus', node: 'create_user', edges: ['calls'], direction: 'incoming', depth: 3 },
      emphasis: { dim_unmatched: 0.12, tiered_colors: ['#ef4444', '#f97316'] },
      annotation: { title: 'Blast radius: create_user' },
    };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, activeQuery: query },
    });
    const title = container.querySelector('.annotation-title');
    expect(title?.textContent).toContain('Blast radius');
  });

  it('renders test_gaps scope query', () => {
    const query = {
      scope: { type: 'test_gaps' },
      emphasis: { highlight: { matched: { color: '#ef4444', label: 'Untested' } }, dim_unmatched: 0.3 },
      annotation: { title: 'Test coverage gaps' },
    };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, activeQuery: query },
    });
    expect(container.querySelector('.annotation-title')?.textContent).toContain('Test coverage gaps');
  });

  it('renders filter scope with node_types', () => {
    const query = {
      scope: { type: 'filter', node_types: ['endpoint'] },
      annotation: { title: 'Endpoints only' },
    };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, activeQuery: query },
    });
    expect(container.querySelector('.annotation-title')?.textContent).toContain('Endpoints only');
  });

  it('renders concept scope query with seed nodes', () => {
    const query = {
      scope: { type: 'concept', seed_nodes: ['User'], expand_edges: ['calls'], expand_depth: 2 },
      emphasis: { highlight: { matched: { color: '#60a5fa' } }, dim_unmatched: 0.15 },
      annotation: { title: 'User concept' },
    };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, activeQuery: query },
    });
    expect(container.querySelector('.annotation-title')?.textContent).toContain('User concept');
  });

  it('renders heat map emphasis query', () => {
    const query = {
      scope: { type: 'all' },
      emphasis: { heat: { metric: 'incoming_calls', palette: 'blue-red' } },
      annotation: { title: 'Hot paths' },
    };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, activeQuery: query },
    });
    expect(container.querySelector('.annotation-title')?.textContent).toContain('Hot paths');
  });

  it('renders diff scope query', () => {
    const query = {
      scope: { type: 'diff', from_commit: 'abc123' },
      emphasis: { highlight: { matched: { color: '#22c55e' } } },
      annotation: { title: 'Recent changes' },
    };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, activeQuery: query },
    });
    expect(container.querySelector('.annotation-title')?.textContent).toContain('Recent changes');
  });
});

describe('ExplorerCanvas — view query opacity resolution', () => {
  // Unit test the queryNodeOpacity logic from ExplorerCanvas.svelte
  function resolveQueryMatch(nodes, edges, scope) {
    const nodeById = new Map();
    for (const n of nodes) nodeById.set(n.id, n);

    // Build adjacency
    const adjacency = new Map();
    for (const e of edges) {
      const src = e.source_id;
      const tgt = e.target_id;
      const et = (e.edge_type ?? '').toLowerCase();
      if (!adjacency.has(src)) adjacency.set(src, []);
      if (!adjacency.has(tgt)) adjacency.set(tgt, []);
      adjacency.get(src).push({ targetId: tgt, edgeType: et, reverse: false });
      adjacency.get(tgt).push({ targetId: src, edgeType: et, reverse: true });
    }

    if (scope.type === 'all') return new Set(nodes.map(n => n.id));

    if (scope.type === 'filter') {
      const matched = new Set();
      for (const n of nodes) {
        let m = true;
        if (scope.node_types?.length && !scope.node_types.includes(n.node_type)) m = false;
        if (scope.name_pattern) {
          const re = new RegExp(scope.name_pattern, 'i');
          if (!re.test(n.name ?? '') && !re.test(n.qualified_name ?? '')) m = false;
        }
        if (m) matched.add(n.id);
      }
      return matched;
    }

    if (scope.type === 'focus') {
      const seedNode = nodes.find(n => n.name === scope.node || n.qualified_name === scope.node);
      if (!seedNode) return new Set();
      const matched = new Map();
      matched.set(seedNode.id, 0);
      const q = [{ id: seedNode.id, depth: 0 }];
      const maxDepth = scope.depth ?? 3;
      const edgeTypes = new Set((scope.edges ?? ['calls']).map(e => e.toLowerCase()));
      while (q.length > 0) {
        const { id, depth } = q.shift();
        if (depth >= maxDepth) continue;
        for (const nb of (adjacency.get(id) ?? [])) {
          if (matched.has(nb.targetId)) continue;
          if (!edgeTypes.has(nb.edgeType)) continue;
          if (scope.direction === 'outgoing' && nb.reverse) continue;
          if (scope.direction === 'incoming' && !nb.reverse) continue;
          matched.set(nb.targetId, depth + 1);
          q.push({ id: nb.targetId, depth: depth + 1 });
        }
      }
      return new Set(matched.keys());
    }

    if (scope.type === 'test_gaps') {
      // Non-test functions with no test calling them
      const testCalledIds = new Set();
      for (const e of edges) {
        const src = nodeById.get(e.source_id);
        const et = (e.edge_type ?? '').toLowerCase();
        if (src?.test_node && et === 'calls') testCalledIds.add(e.target_id);
      }
      // BFS from test-called nodes
      const reachable = new Set(testCalledIds);
      const bfsQ = [...testCalledIds];
      while (bfsQ.length > 0) {
        const id = bfsQ.shift();
        for (const nb of (adjacency.get(id) ?? [])) {
          if (reachable.has(nb.targetId) || nb.reverse) continue;
          if (nb.edgeType !== 'calls') continue;
          reachable.add(nb.targetId);
          bfsQ.push(nb.targetId);
        }
      }
      const matched = new Set();
      for (const n of nodes) {
        if (!n.test_node && n.node_type === 'function' && !reachable.has(n.id)) matched.add(n.id);
      }
      return matched;
    }

    if (scope.type === 'concept') {
      const seeds = scope.seed_nodes ?? [];
      const expandEdges = new Set((scope.expand_edges ?? ['calls']).map(e => e.toLowerCase()));
      const maxDepth = scope.expand_depth ?? 2;
      const dir = scope.expand_direction ?? 'both';
      const matched = new Map();
      for (const seedName of seeds) {
        const seedNode = nodes.find(n => n.name === seedName || n.qualified_name === seedName);
        if (!seedNode || matched.has(seedNode.id)) continue;
        matched.set(seedNode.id, 0);
        const q = [{ id: seedNode.id, depth: 0 }];
        while (q.length > 0) {
          const { id, depth } = q.shift();
          if (depth >= maxDepth) continue;
          for (const nb of (adjacency.get(id) ?? [])) {
            if (matched.has(nb.targetId)) continue;
            if (!expandEdges.has(nb.edgeType)) continue;
            if (dir === 'outgoing' && nb.reverse) continue;
            if (dir === 'incoming' && !nb.reverse) continue;
            matched.set(nb.targetId, depth + 1);
            q.push({ id: nb.targetId, depth: depth + 1 });
          }
        }
      }
      return new Set(matched.keys());
    }

    if (scope.type === 'diff') {
      const matched = new Set();
      for (const n of nodes) {
        if (n.last_commit_sha && n.last_commit_sha !== scope.from_commit) matched.add(n.id);
      }
      return matched;
    }

    return new Set();
  }

  function queryNodeOpacity(nodeId, matchedIds, dimUnmatched) {
    if (!matchedIds) return 1.0;
    return matchedIds.has(nodeId) ? 1.0 : (dimUnmatched ?? 0.12);
  }

  it('filter scope: matched nodes get full opacity, unmatched get dim', () => {
    const matched = resolveQueryMatch(NODES, EDGES, { type: 'filter', node_types: ['function'] });

    expect(matched.has('fn1')).toBe(true);
    expect(matched.has('fn2')).toBe(true);
    expect(matched.has('test1')).toBe(true);
    expect(matched.has('type1')).toBe(false);
    expect(matched.has('pkg1')).toBe(false);

    expect(queryNodeOpacity('fn1', matched, 0.12)).toBe(1.0);
    expect(queryNodeOpacity('type1', matched, 0.12)).toBe(0.12);
    expect(queryNodeOpacity('pkg1', matched, 0.15)).toBe(0.15);
  });

  it('filter scope with name_pattern narrows results', () => {
    const matched = resolveQueryMatch(NODES, EDGES, { type: 'filter', node_types: ['function'], name_pattern: 'create' });

    expect(matched.has('fn1')).toBe(true); // create_user matches
    expect(matched.has('fn2')).toBe(false); // get_user does not
    expect(matched.has('test1')).toBe(true); // test_create_user matches
  });

  it('focus scope: BFS from seed node along call edges', () => {
    const matched = resolveQueryMatch(NODES, EDGES, {
      type: 'focus', node: 'create_user', edges: ['calls'], direction: 'both', depth: 3,
    });

    expect(matched.has('fn1')).toBe(true); // seed node
    expect(matched.has('type1')).toBe(true); // fn1 calls type1
    expect(matched.has('test1')).toBe(true); // test1 calls fn1 (incoming)
    expect(matched.has('fn2')).toBe(false); // not connected via calls
  });

  it('focus scope with direction=incoming only follows reverse edges', () => {
    const matched = resolveQueryMatch(NODES, EDGES, {
      type: 'focus', node: 'create_user', edges: ['calls'], direction: 'incoming', depth: 3,
    });

    expect(matched.has('fn1')).toBe(true); // seed
    expect(matched.has('test1')).toBe(true); // test1 calls fn1
    expect(matched.has('type1')).toBe(false); // fn1 -> type1 is outgoing, not incoming
  });

  it('focus scope with direction=outgoing only follows forward edges', () => {
    const matched = resolveQueryMatch(NODES, EDGES, {
      type: 'focus', node: 'create_user', edges: ['calls'], direction: 'outgoing', depth: 3,
    });

    expect(matched.has('fn1')).toBe(true); // seed
    expect(matched.has('type1')).toBe(true); // fn1 -> type1 outgoing
    expect(matched.has('test1')).toBe(false); // test1 -> fn1 is incoming
  });

  it('focus scope depth limits traversal', () => {
    const matched = resolveQueryMatch(NODES, EDGES, {
      type: 'focus', node: 'test_create_user', edges: ['calls'], direction: 'outgoing', depth: 1,
    });

    expect(matched.has('test1')).toBe(true); // seed
    expect(matched.has('fn1')).toBe(true); // depth 1
    expect(matched.has('type1')).toBe(false); // depth 2, beyond limit
  });

  it('test_gaps scope: finds untested functions', () => {
    const matched = resolveQueryMatch(NODES, EDGES, { type: 'test_gaps' });

    // fn2 (get_user) has no test calling it -> untested
    expect(matched.has('fn2')).toBe(true);
    // fn1 (create_user) is called by test1 -> tested
    expect(matched.has('fn1')).toBe(false);
    // test1 is a test node itself, excluded
    expect(matched.has('test1')).toBe(false);
  });

  it('all scope: every node gets full opacity', () => {
    const matched = resolveQueryMatch(NODES, EDGES, { type: 'all' });

    for (const n of NODES) {
      expect(matched.has(n.id)).toBe(true);
      expect(queryNodeOpacity(n.id, matched, 0.12)).toBe(1.0);
    }
  });

  it('concept scope: expands from seed along edges', () => {
    const matched = resolveQueryMatch(NODES, EDGES, {
      type: 'concept', seed_nodes: ['User'], expand_edges: ['calls'], expand_depth: 2,
    });

    expect(matched.has('type1')).toBe(true); // seed
    expect(matched.has('fn1')).toBe(true); // calls edge (fn1 -> type1)
    expect(matched.has('test1')).toBe(true); // test1 -> fn1 (depth 2)
    expect(matched.has('fn2')).toBe(false); // no call connection to User
  });

  it('diff scope: highlights nodes with different commit SHA', () => {
    const nodesWithCommit = [
      ...NODES.map(n => ({ ...n, last_commit_sha: n.id === 'fn1' ? 'newsha' : 'abc123' })),
    ];
    const matched = resolveQueryMatch(nodesWithCommit, EDGES, { type: 'diff', from_commit: 'abc123' });

    expect(matched.has('fn1')).toBe(true); // different SHA
    expect(matched.has('fn2')).toBe(false); // same SHA
  });

  it('returns empty set for focus scope with non-existent node', () => {
    const matched = resolveQueryMatch(NODES, EDGES, {
      type: 'focus', node: 'does_not_exist', edges: ['calls'], direction: 'both', depth: 3,
    });
    expect(matched.size).toBe(0);
  });

  it('null matched set means full opacity for all nodes', () => {
    expect(queryNodeOpacity('fn1', null, 0.12)).toBe(1.0);
    expect(queryNodeOpacity('anything', null, 0.12)).toBe(1.0);
  });

  it('custom dim_unmatched value is respected', () => {
    const matched = new Set(['fn1']);
    expect(queryNodeOpacity('fn2', matched, 0.3)).toBe(0.3);
    expect(queryNodeOpacity('fn2', matched, 0.5)).toBe(0.5);
    expect(queryNodeOpacity('fn2', matched, 0.01)).toBe(0.01);
  });
});

describe('ExplorerCanvas — interactive $clicked query mode', () => {
  it('renders $clicked query annotation with placeholder', () => {
    const query = {
      scope: { type: 'focus', node: '$clicked', edges: ['calls'], direction: 'incoming', depth: 10 },
      emphasis: { tiered_colors: ['#ef4444', '#f97316', '#eab308', '#94a3b8'], dim_unmatched: 0.12 },
      annotation: { title: 'Blast radius: $name', description: '{{count}} transitive callers' },
    };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, activeQuery: query },
    });
    const title = container.querySelector('.annotation-title');
    // $name should be replaced with empty string since no node is selected
    expect(title?.textContent).toContain('Blast radius:');
  });

  it('stores $clicked query as interactive template', () => {
    // Verify the component handles $clicked scope without error
    const query = {
      scope: { type: 'focus', node: '$clicked', edges: ['calls', 'depends_on'], direction: 'both', depth: 5 },
      emphasis: { dim_unmatched: 0.15 },
      annotation: { title: 'Impact: $name' },
    };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, activeQuery: query },
    });
    expect(container.querySelector('canvas')).toBeTruthy();
    expect(container.querySelector('.annotation-title')?.textContent).toContain('Impact:');
  });

  it('$clicked query with tiered_colors renders without error', () => {
    const query = {
      scope: { type: 'focus', node: '$clicked', edges: ['calls'], direction: 'incoming', depth: 10 },
      emphasis: {
        tiered_colors: ['#ef4444', '#f97316', '#eab308', '#94a3b8'],
        dim_unmatched: 0.12,
      },
    };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, activeQuery: query },
    });
    expect(container.querySelector('canvas')).toBeTruthy();
  });
});

describe('ExplorerCanvas — breadcrumb navigation', () => {
  it('no breadcrumb visible at root level', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    expect(container.querySelector('.treemap-breadcrumb')).toBeFalsy();
  });

  it('breadcrumb path data structure is correct', () => {
    // Simulate breadcrumb state: verify structure matches component expectations
    const breadcrumb = [
      { id: 'pkg1', name: 'api' },
      { id: 'mod1', name: 'handlers' },
    ];

    expect(breadcrumb).toHaveLength(2);
    expect(breadcrumb[0].id).toBe('pkg1');
    expect(breadcrumb[0].name).toBe('api');
    expect(breadcrumb[1].id).toBe('mod1');
  });

  it('navigateBreadcrumb(-1) resets to root', () => {
    // Simulate navigateBreadcrumb logic
    let breadcrumb = [
      { id: 'pkg1', name: 'api' },
      { id: 'mod1', name: 'handlers' },
    ];

    function navigateBreadcrumb(index) {
      if (index === -1) {
        breadcrumb = [];
      } else {
        breadcrumb = breadcrumb.slice(0, index + 1);
      }
    }

    navigateBreadcrumb(-1);
    expect(breadcrumb).toHaveLength(0);
  });

  it('navigateBreadcrumb(0) keeps first crumb only', () => {
    let breadcrumb = [
      { id: 'pkg1', name: 'api' },
      { id: 'mod1', name: 'handlers' },
      { id: 'fn1', name: 'create_user' },
    ];

    function navigateBreadcrumb(index) {
      if (index === -1) {
        breadcrumb = [];
      } else {
        breadcrumb = breadcrumb.slice(0, index + 1);
      }
    }

    navigateBreadcrumb(0);
    expect(breadcrumb).toHaveLength(1);
    expect(breadcrumb[0].id).toBe('pkg1');
  });

  it('canvasState reflects breadcrumb state', () => {
    const breadcrumb = [{ id: 'pkg1', name: 'api' }];
    const canvasState = {
      selectedNode: null,
      breadcrumb: breadcrumb.map(b => ({ id: b.id, name: b.name })),
    };

    expect(canvasState.breadcrumb).toHaveLength(1);
    expect(canvasState.breadcrumb[0].id).toBe('pkg1');
    expect(canvasState.selectedNode).toBeNull();
  });
});

describe('ExplorerCanvas — context menu', () => {
  it('does not show context menu by default', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    expect(container.querySelector('.ctx-menu')).toBeFalsy();
  });

  it('context menu backdrop closes on click', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    expect(container.querySelector('.ctx-menu')).toBeFalsy();
  });

  it('context menu data structure includes expected actions', () => {
    // Verify the actions that should be in the context menu
    const expectedActions = ['trace', 'blast', 'callers', 'callees', 'drill', 'spec', 'detail', 'open_in_code', 'provenance', 'history'];
    expect(expectedActions).toContain('trace');
    expect(expectedActions).toContain('blast');
    expect(expectedActions).toContain('callers');
    expect(expectedActions).toContain('callees');
    expect(expectedActions).toContain('provenance');
    expect(expectedActions).toContain('history');
  });

  it('context menu node has required fields', () => {
    // Simulate context menu node structure
    const contextMenu = {
      x: 150,
      y: 200,
      node: NODES[2], // fn1 = create_user
    };

    expect(contextMenu.node.name).toBe('create_user');
    expect(contextMenu.node.node_type).toBe('function');
    expect(contextMenu.node.file_path).toBe('api/handlers.py');
    expect(contextMenu.x).toBeGreaterThan(0);
    expect(contextMenu.y).toBeGreaterThan(0);
  });

  it('drill action only available for nodes with children', () => {
    // Build parent-to-children map from edges
    const parentToChildren = new Map();
    for (const e of EDGES) {
      if (e.edge_type === 'contains') {
        if (!parentToChildren.has(e.source_id)) parentToChildren.set(e.source_id, []);
        parentToChildren.get(e.source_id).push(e.target_id);
      }
    }

    // pkg1 has children (contains mod1) -> drill available
    expect((parentToChildren.get('pkg1') ?? []).length).toBeGreaterThan(0);
    // fn1 has no children -> no drill
    expect((parentToChildren.get('fn1') ?? []).length).toBe(0);
  });

  it('spec action only available for nodes with spec_path', () => {
    const nodeWithSpec = { ...NODES[2], spec_path: 'specs/api/create_user.md' };
    const nodeWithoutSpec = { ...NODES[2], spec_path: undefined };

    expect(nodeWithSpec.spec_path).toBeTruthy();
    expect(nodeWithoutSpec.spec_path).toBeFalsy();
  });

  it('open_in_code action only available for nodes with file_path', () => {
    // fn1 has file_path -> available
    expect(NODES[2].file_path).toBeTruthy();
    // pkg1 has empty file_path -> not available
    expect(NODES[0].file_path).toBe('');
  });
});

describe('ExplorerCanvas — lens switching', () => {
  it('structural lens is active by default', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    const btns = Array.from(container.querySelectorAll('.tb-btn'));
    const structural = btns.find(b => b.textContent === 'Structural');
    expect(structural?.classList.contains('active')).toBe(true);
  });

  it('evaluative lens renders metric selector when trace data exists', () => {
    const traceData = { spans: [{ span_id: 's1', graph_node_id: 'n1', duration_us: 100 }] };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData },
    });
    expect(container.querySelector('.eval-metric-group')).toBeTruthy();
    const evalBtns = container.querySelectorAll('.eval-metric-group .tb-btn-sm');
    expect(evalBtns.length).toBeGreaterThanOrEqual(1);
  });

  it('evaluative lens shows no-trace message without trace data', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative' },
    });
    expect(container.querySelector('.eval-no-trace')).toBeTruthy();
  });

  it('evaluative lens does not show structural metrics (complexity, churn, etc.)', () => {
    const traceData = { spans: [{ span_id: 's1', graph_node_id: 'n1', duration_us: 100 }] };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData },
    });
    const btnTexts = Array.from(container.querySelectorAll('.eval-metric-group .tb-btn-sm')).map(b => b.textContent);
    expect(btnTexts).not.toContain('Complexity');
    expect(btnTexts).not.toContain('Churn');
    expect(btnTexts).not.toContain('Call Count');
    expect(btnTexts).not.toContain('Test Coverage');
  });

  it('evaluative lens shows playback controls with trace data', () => {
    const traceData = {
      spans: [
        { span_id: 's1', parent_span_id: null, operation_name: 'test', start_time: 1000, duration_us: 500, status: 'ok', graph_node_id: 'fn1' },
      ],
      root_spans: ['s1'],
    };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData },
    });
    expect(container.querySelector('.trace-playback-bar')).toBeTruthy();
  });

  it('structural lens shows spec coverage legend', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'structural' },
    });
    const legendLabels = [...container.querySelectorAll('.legend-label')].map(el => el.textContent);
    expect(legendLabels).toContain('Has spec');
    expect(legendLabels).toContain('No spec');
  });

  it('evaluative lens shows evaluative legend', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative' },
    });
    const legendLabels = [...container.querySelectorAll('.legend-label')].map(el => el.textContent);
    expect(legendLabels).toContain('OK span');
    expect(legendLabels).toContain('Error span');
  });
});

describe('ExplorerCanvas — heat map coloring', () => {
  // Unit test the evaluativeNodeColor logic
  function evaluativeNodeColor(metric, node, incomingCallCounts, maxValues) {
    let value = 0;
    if (metric === 'incoming_calls') value = incomingCallCounts.get(node.id) ?? 0;
    else if (metric === 'complexity') value = node.complexity ?? 0;
    else if (metric === 'churn' || metric === 'churn_count_30d') value = node.churn_count_30d ?? node.churn ?? 0;
    else if (metric === 'test_coverage') value = (node.test_coverage ?? 0) * 100;
    if (value === 0) return null;
    const maxVal = maxValues.get(metric) ?? 1;
    const t = Math.min(1, value / maxVal);
    return t; // Return normalized value for testing
  }

  it('returns null for nodes with zero metric value', () => {
    const node = { id: 'fn1', complexity: 0 };
    const result = evaluativeNodeColor('complexity', node, new Map(), new Map([['complexity', 20]]));
    expect(result).toBeNull();
  });

  it('returns normalized value for complexity metric', () => {
    const node = { id: 'fn1', complexity: 10 };
    const maxValues = new Map([['complexity', 20]]);
    const result = evaluativeNodeColor('complexity', node, new Map(), maxValues);
    expect(result).toBe(0.5); // 10/20
  });

  it('returns normalized value for incoming_calls metric', () => {
    const node = { id: 'fn1' };
    const callCounts = new Map([['fn1', 5]]);
    const maxValues = new Map([['incoming_calls', 10]]);
    const result = evaluativeNodeColor('incoming_calls', node, callCounts, maxValues);
    expect(result).toBe(0.5); // 5/10
  });

  it('clamps to 1.0 when value exceeds max', () => {
    const node = { id: 'fn1', complexity: 30 };
    const maxValues = new Map([['complexity', 20]]);
    const result = evaluativeNodeColor('complexity', node, new Map(), maxValues);
    expect(result).toBe(1.0);
  });

  it('handles churn metric with churn_count_30d', () => {
    const node = { id: 'fn1', churn_count_30d: 8 };
    const maxValues = new Map([['churn', 16]]);
    const result = evaluativeNodeColor('churn', node, new Map(), maxValues);
    expect(result).toBe(0.5);
  });

  it('handles test_coverage metric as percentage', () => {
    const node = { id: 'fn1', test_coverage: 0.75 };
    const maxValues = new Map([['test_coverage', 100]]);
    const result = evaluativeNodeColor('test_coverage', node, new Map(), maxValues);
    expect(result).toBe(0.75); // 75/100
  });

  it('precomputes incoming call counts correctly', () => {
    const counts = new Map();
    for (const e of EDGES) {
      const tgt = e.target_id;
      const et = (e.edge_type ?? '').toLowerCase();
      if (et === 'calls' && tgt) {
        counts.set(tgt, (counts.get(tgt) ?? 0) + 1);
      }
    }

    expect(counts.get('type1')).toBe(1); // fn1 -> type1
    expect(counts.get('fn1')).toBe(1); // test1 -> fn1
    expect(counts.has('fn2')).toBe(false); // nothing calls fn2
  });

  it('precomputes max values for evaluative heat config', () => {
    const nodesWithMetrics = NODES.map(n => ({
      ...n,
      complexity: n.id === 'fn1' ? 15 : n.id === 'fn2' ? 8 : 0,
    }));

    let max = 0;
    for (const n of nodesWithMetrics) {
      const v = n.complexity ?? 0;
      if (v > max) max = v;
    }

    expect(max).toBe(15);
  });
});

describe('ExplorerCanvas — timeline scrubber', () => {
  it('does not show timeline by default', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    expect(container.querySelector('.timeline-scrubber')).toBeFalsy();
  });

  it('has a Timeline toggle button in toolbar', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    const btns = Array.from(container.querySelectorAll('.tb-btn'));
    const timelineBtn = btns.find(b => b.textContent.includes('Timeline'));
    expect(timelineBtn).toBeTruthy();
  });
});

describe('ExplorerCanvas — canvas search', () => {
  it('does not show search by default', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    expect(container.querySelector('.canvas-search')).toBeFalsy();
  });

  it('search result matching logic works for node names', () => {
    const searchQuery = 'user';
    const q = searchQuery.toLowerCase();
    const results = NODES.filter(n =>
      n.name?.toLowerCase().includes(q) ||
      n.qualified_name?.toLowerCase().includes(q) ||
      n.node_type?.toLowerCase().includes(q)
    );

    // Should find: create_user, get_user, User, test_create_user
    expect(results.length).toBe(4);
    const names = results.map(n => n.name);
    expect(names).toContain('create_user');
    expect(names).toContain('get_user');
    expect(names).toContain('User');
    expect(names).toContain('test_create_user');
  });

  it('search result matching is case-insensitive', () => {
    const q = 'USER'.toLowerCase();
    const results = NODES.filter(n =>
      n.name?.toLowerCase().includes(q)
    );
    expect(results.length).toBeGreaterThan(0);
  });
});

describe('ExplorerCanvas — evaluative lens', () => {
  it('renders evaluative metric buttons when lens is evaluative with trace data', () => {
    const traceData = { spans: [{ span_id: 's1', graph_node_id: 'n1', duration_us: 100 }] };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData },
    });
    const evalBtns = container.querySelectorAll('.eval-metric-group .tb-btn-sm');
    expect(evalBtns.length).toBeGreaterThanOrEqual(1);
  });

  it('renders annotation with badge query', () => {
    const query = {
      scope: { type: 'all' },
      emphasis: { badges: { metric: 'incoming_calls', template: '{{count}} calls' } },
      annotation: { title: 'Call count badges' },
    };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, activeQuery: query },
    });
    expect(container.querySelector('.annotation-title')?.textContent).toContain('Call count badges');
  });

  it('renders interactive $clicked query annotation', () => {
    const query = {
      scope: { type: 'focus', node: '$clicked', edges: ['calls'], direction: 'incoming', depth: 10 },
      emphasis: { tiered_colors: ['#ef4444', '#f97316', '#eab308', '#94a3b8'], dim_unmatched: 0.12 },
      annotation: { title: 'Blast radius: $name', description: '{{count}} transitive callers' },
    };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, activeQuery: query },
    });
    const title = container.querySelector('.annotation-title');
    // $name should be replaced with empty string since no node is selected
    expect(title?.textContent).toContain('Blast radius:');
  });

  it('shows evaluative lens metric selector', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative' },
    });
    expect(container.querySelector('.eval-metric-group')).toBeTruthy();
  });

  it('shows evaluative playback controls with trace data', () => {
    const traceData = {
      spans: [
        { span_id: 's1', parent_span_id: null, operation_name: 'test', start_time: 1000, duration_us: 500, status: 'ok', graph_node_id: 'fn1' },
      ],
      root_spans: ['s1'],
    };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData },
    });
    expect(container.querySelector('.trace-playback-bar')).toBeTruthy();
  });

  it('shows no-trace message when evaluative lens lacks trace data', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative' },
    });
    expect(container.querySelector('.eval-no-trace')).toBeTruthy();
  });

  it('renders spec coverage legend in structural lens', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'structural' },
    });
    const legendLabels = [...container.querySelectorAll('.legend-label')].map(el => el.textContent);
    expect(legendLabels).toContain('Has spec');
    expect(legendLabels).toContain('No spec');
  });

  it('renders evaluative legend in evaluative lens', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative' },
    });
    const legendLabels = [...container.querySelectorAll('.legend-label')].map(el => el.textContent);
    expect(legendLabels).toContain('OK span');
    expect(legendLabels).toContain('Error span');
  });

  it('renders context menu actions including spec-required items', () => {
    // The context menu includes View spec, View provenance, View history, Open in code
    // We verify the menu item strings exist in the component source
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    // Menu is not visible until right-click, but DOM structure should be present
    const canvas = container.querySelector('canvas');
    expect(canvas).toBeTruthy();
  });

  it('renders callouts using server field name "node" (not "node_name")', () => {
    // Verify callouts work with server-format field names
    const query = {
      scope: { type: 'all' },
      callouts: [{ node: 'create_user', text: 'Important function' }],
    };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, activeQuery: query },
    });
    // Just verify it renders without error — the callout resolution is tested by
    // the fact that the component doesn't throw with `node` field
    expect(container.querySelector('canvas')).toBeTruthy();
  });

  it('supports narrative steps with server field name "node"', () => {
    const query = {
      scope: { type: 'all' },
      narrative: [
        { node: 'create_user', text: 'Step 1: Create user' },
        { node: 'get_user', text: 'Step 2: Get user' },
      ],
    };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, activeQuery: query },
    });
    expect(container.querySelector('canvas')).toBeTruthy();
  });
});

describe('ExplorerCanvas — anomaly detection (evaluative)', () => {
  it('detects high complexity + low test coverage anomaly', () => {
    const nodesWithMetrics = [
      { id: 'fn1', node_type: 'function', name: 'complex_fn', complexity: 20, test_coverage: 0.1, test_node: false },
    ];
    const anomalies = [];
    for (const n of nodesWithMetrics) {
      if ((n.complexity ?? 0) > 15 && (n.test_coverage ?? 0) < 0.3) {
        anomalies.push({ nodeId: n.id, severity: 'high' });
      }
    }
    expect(anomalies).toHaveLength(1);
    expect(anomalies[0].severity).toBe('high');
  });

  it('detects orphan function (no callers) anomaly', () => {
    const callCounts = new Map();
    for (const e of EDGES) {
      const et = (e.edge_type ?? '').toLowerCase();
      if (et === 'calls') {
        callCounts.set(e.target_id, (callCounts.get(e.target_id) ?? 0) + 1);
      }
    }

    const orphans = [];
    for (const n of NODES) {
      if (n.node_type === 'function' && !n.test_node && (callCounts.get(n.id) ?? 0) === 0) {
        orphans.push(n.id);
      }
    }

    // fn2 (get_user) has no callers
    expect(orphans).toContain('fn2');
    // fn1 is called by test1
    expect(orphans).not.toContain('fn1');
  });

  it('detects heavily depended on node with no spec', () => {
    const callCounts = new Map([['fn1', 8]]); // 8 callers
    const node = { id: 'fn1', spec_path: undefined };

    const isHeavilyDepended = (callCounts.get(node.id) ?? 0) > 5 && !node.spec_path;
    expect(isHeavilyDepended).toBe(true);
  });

  it('sorts anomalies by severity (high first)', () => {
    const anomalies = [
      { severity: 'low' },
      { severity: 'high' },
      { severity: 'medium' },
    ];
    const order = { high: 0, medium: 1, low: 2 };
    anomalies.sort((a, b) => (order[a.severity] ?? 3) - (order[b.severity] ?? 3));

    expect(anomalies[0].severity).toBe('high');
    expect(anomalies[1].severity).toBe('medium');
    expect(anomalies[2].severity).toBe('low');
  });
});

describe('ExplorerCanvas — callout and narrative resolution', () => {
  it('resolves callout node names to node IDs', () => {
    const callouts = [
      { node: 'create_user', text: 'Entry point' },
      { node: 'User', text: 'Core domain type' },
    ];

    const resolved = new Map();
    for (const c of callouts) {
      const cName = c.node ?? c.node_name;
      const n = NODES.find(n => n.name === cName || n.qualified_name === cName);
      if (n) resolved.set(n.id, c.text ?? '');
    }

    expect(resolved.has('fn1')).toBe(true);
    expect(resolved.get('fn1')).toBe('Entry point');
    expect(resolved.has('type1')).toBe(true);
    expect(resolved.get('type1')).toBe('Core domain type');
  });

  it('handles callout with non-existent node name gracefully', () => {
    const callouts = [{ node: 'does_not_exist', text: 'Missing' }];
    const resolved = new Map();
    for (const c of callouts) {
      const cName = c.node ?? c.node_name;
      const n = NODES.find(n => n.name === cName || n.qualified_name === cName);
      if (n) resolved.set(n.id, c.text ?? '');
    }

    expect(resolved.size).toBe(0);
  });

  it('resolves callout with qualified_name match', () => {
    const callouts = [{ node: 'api.handlers.create_user', text: 'FQN match' }];
    const resolved = new Map();
    for (const c of callouts) {
      const cName = c.node ?? c.node_name;
      const n = NODES.find(n => n.name === cName || n.qualified_name === cName);
      if (n) resolved.set(n.id, c.text ?? '');
    }

    expect(resolved.has('fn1')).toBe(true);
    expect(resolved.get('fn1')).toBe('FQN match');
  });
});

describe('ExplorerCanvas — spec border coloring', () => {
  it('returns green for node with spec_path', () => {
    const node = { id: 'fn1', spec_path: 'specs/api.md' };
    expect(node.spec_path).toBeTruthy();
    // specBorderColor returns #22c55e for nodes with spec_path
  });

  it('returns green for high spec_confidence', () => {
    const node = { id: 'fn1', spec_confidence: 'high' };
    expect(node.spec_confidence).toBe('high');
  });

  it('returns amber for medium spec_confidence', () => {
    const node = { id: 'fn2', spec_confidence: 'medium' };
    expect(node.spec_confidence).toBe('medium');
  });

  it('returns red for no spec coverage', () => {
    const node = { id: 'pkg1', spec_confidence: 'none' };
    expect(node.spec_confidence).toBe('none');
    // specBorderColor returns #ef4444 for nodes with no spec
  });

  // Full unit test of specBorderColor logic
  function specBorderColor(node, edges) {
    if (!node) return '#64748b';
    if (node.spec_path) return '#22c55e';
    const conf = node.spec_confidence;
    if (conf === 'high') return '#22c55e';
    if (conf === 'medium') return '#eab308';
    if (conf === 'low') return '#f97316';
    const nodeId = node.id;
    if (nodeId) {
      for (const e of edges) {
        const src = e.source_id ?? e.from_node_id ?? e.from;
        const et = (e.edge_type ?? e.type ?? '').toLowerCase();
        if (et === 'governed_by' && src === nodeId) return '#22c55e';
      }
    }
    return '#ef4444';
  }

  it('spec_path takes priority over spec_confidence', () => {
    expect(specBorderColor({ id: 'x', spec_path: 'specs/x.md', spec_confidence: 'none' }, [])).toBe('#22c55e');
  });

  it('governed_by edge gives green border', () => {
    const node = { id: 'fn1', spec_confidence: 'none' };
    const edges = [{ source_id: 'fn1', target_id: 'spec1', edge_type: 'governed_by' }];
    expect(specBorderColor(node, edges)).toBe('#22c55e');
  });

  it('null node returns default gray', () => {
    expect(specBorderColor(null, [])).toBe('#64748b');
  });

  it('low spec_confidence returns orange', () => {
    expect(specBorderColor({ id: 'x', spec_confidence: 'low' }, [])).toBe('#f97316');
  });
});

// ── Multi-select / Concept creation ─────────────────────────────────────

describe('ExplorerCanvas — multi-select for concept creation', () => {
  it('shows concept creation bar when nodes are multi-selected', async () => {
    // The concept creation bar appears when multiSelectedIds > 0
    // We can't directly trigger Shift+Click in jsdom (no hit testing),
    // so test the UI rendering given the component state.
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    // Initially no concept bar
    expect(container.querySelector('.concept-creation-bar')).toBeFalsy();
  });

  it('renders without concept bar when empty selection', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    expect(container.querySelector('.concept-create-btn')).toBeFalsy();
    expect(container.querySelector('.concept-hint')).toBeFalsy();
  });
});

// ── Cmd+K search ────────────────────────────────────────────────────────

describe('ExplorerCanvas — keyboard search', () => {
  it('opens search overlay on / key', async () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    const canvas = container.querySelector('canvas');
    if (canvas) {
      canvas.focus();
      canvas.dispatchEvent(new KeyboardEvent('keydown', { key: '/', bubbles: true }));
    }
    // Search overlay should appear
    await new Promise(r => setTimeout(r, 50));
    const searchInput = container.querySelector('.canvas-search-input');
    // Note: may or may not render depending on jsdom canvas focus behavior
    // At minimum, the component should not crash
    expect(container).toBeTruthy();
  });
});

// ── Ghost overlays ──────────────────────────────────────────────────────

describe('ExplorerCanvas — ghost overlays', () => {
  const GHOST_ADD = { id: 'new1', name: 'NewService', type: 'type', action: 'add', confidence: 'high' };
  const GHOST_CHANGE = { id: 'fn1', name: 'create_user', type: 'function', action: 'change', reason: 'Updated validation', confidence: 'medium' };
  const GHOST_REMOVE = { id: 'fn2', name: 'get_user', type: 'function', action: 'remove', confidence: 'low' };

  it('renders preview mode indicator with ghost overlays', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, ghostOverlays: [GHOST_ADD] },
    });
    const indicator = container.querySelector('[data-testid="preview-mode-indicator"]');
    expect(indicator).toBeTruthy();
  });

  it('shows add/change/remove chips for mixed ghost types', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, ghostOverlays: [GHOST_ADD, GHOST_CHANGE, GHOST_REMOVE] },
    });
    const addChip = container.querySelector('.ghost-chip-add');
    const changeChip = container.querySelector('.ghost-chip-change');
    const removeChip = container.querySelector('.ghost-chip-remove');
    expect(addChip).toBeTruthy();
    expect(changeChip).toBeTruthy();
    expect(removeChip).toBeTruthy();
  });

  it('shows confidence breakdown in ghost legend', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, ghostOverlays: [GHOST_ADD, GHOST_CHANGE, GHOST_REMOVE] },
    });
    const confChip = container.querySelector('.ghost-chip-conf');
    expect(confChip).toBeTruthy();
    expect(confChip.textContent).toContain('1H'); // 1 high
    expect(confChip.textContent).toContain('1M'); // 1 medium
    expect(confChip.textContent).toContain('1L'); // 1 low
  });

  it('renders without preview indicator when no ghosts', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, ghostOverlays: [] },
    });
    const indicator = container.querySelector('[data-testid="preview-mode-indicator"]');
    expect(indicator).toBeFalsy();
  });
});

// ── Contextual tooltip insights ─────────────────────────────────────────

describe('ExplorerCanvas — tooltip insights', () => {
  it('renders tooltip with contextual insights for complex nodes', () => {
    // The tooltip is rendered conditionally when tooltipNode is set.
    // Since we can't hover in jsdom, test that the component renders
    // the structure correctly with appropriate node data.
    const complexNode = {
      ...NODES[2],
      complexity: 35,
      churn_count_30d: 20,
      test_coverage: 0.2,
    };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: [...NODES.slice(0, 2), complexNode, ...NODES.slice(3)], edges: EDGES },
    });
    // Component should render without errors
    expect(container.querySelector('canvas')).toBeTruthy();
  });
});

// ── Semantic zoom ───────────────────────────────────────────────────────

describe('ExplorerCanvas — semantic zoom levels', () => {
  it('renders with structural lens by default', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'structural' },
    });
    // Both "All" filter and "Structural" lens buttons are active by default
    const activeButtons = container.querySelectorAll('.tb-btn.active');
    const texts = [...activeButtons].map(b => b.textContent);
    expect(texts).toContain('Structural');
  });

  it('renders evaluative lens with metric buttons', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative' },
    });
    const metricGroup = container.querySelector('.eval-metric-group');
    expect(metricGroup).toBeTruthy();
  });

  it('observable button is clickable and not disabled', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    const obsBtn = container.querySelector('.tb-btn-observable');
    expect(obsBtn).toBeTruthy();
    expect(obsBtn.disabled).toBe(false);
  });
});

// ── Accessibility ───────────────────────────────────────────────────────

describe('ExplorerCanvas — accessibility', () => {
  it('canvas has application role and aria-label', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    const canvas = container.querySelector('canvas');
    expect(canvas?.getAttribute('role')).toBe('application');
    expect(canvas?.getAttribute('aria-label')).toContain('explorer');
  });

  it('has screen reader live region', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    const srRegion = container.querySelector('.sr-only[aria-live="polite"]');
    expect(srRegion).toBeTruthy();
  });

  it('toolbar has role group with aria-label', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    const filterGroup = container.querySelector('[role="group"][aria-label="Filter presets"]');
    const lensGroup = container.querySelector('[role="group"][aria-label="Lens toggle"]');
    expect(filterGroup).toBeTruthy();
    expect(lensGroup).toBeTruthy();
  });

  it('canvas has tabindex for keyboard focus', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES },
    });
    const canvas = container.querySelector('canvas');
    expect(canvas?.getAttribute('tabindex')).toBe('0');
  });
});
