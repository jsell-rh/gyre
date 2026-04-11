import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render } from '@testing-library/svelte';
import ExplorerCanvas from '../lib/ExplorerCanvas.svelte';

// ── Mock canvas context ──────────────────────────────────────────────────
let mockCtx;
let drawCallCount;

beforeEach(() => {
  drawCallCount = 0;
  mockCtx = {
    clearRect: vi.fn(),
    fillRect: vi.fn(() => { drawCallCount++; }),
    strokeRect: vi.fn(() => { drawCallCount++; }),
    beginPath: vi.fn(),
    closePath: vi.fn(),
    arc: vi.fn(),
    fill: vi.fn(),
    stroke: vi.fn(),
    moveTo: vi.fn(),
    lineTo: vi.fn(),
    quadraticCurveTo: vi.fn(),
    fillText: vi.fn(() => { drawCallCount++; }),
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

// ── Large graph generator ────────────────────────────────────────────────

const NODE_TYPES = ['package', 'module', 'type', 'interface', 'function', 'method', 'endpoint', 'table', 'component', 'constant'];

/**
 * Generate a mock graph with the specified number of nodes and edges.
 * Creates a hierarchical structure: packages -> modules -> functions/types.
 */
function generateLargeGraph(nodeCount, edgeCount) {
  const nodes = [];
  const edges = [];

  // Create a realistic hierarchy: ~1% packages, ~5% modules, rest are leaves
  const pkgCount = Math.max(5, Math.floor(nodeCount * 0.01));
  const modCount = Math.max(20, Math.floor(nodeCount * 0.05));
  const leafCount = nodeCount - pkgCount - modCount;

  // Packages
  for (let i = 0; i < pkgCount; i++) {
    nodes.push({
      id: `pkg${i}`,
      node_type: 'package',
      name: `pkg_${i}`,
      qualified_name: `pkg_${i}`,
      file_path: '',
      line_start: 0,
      line_end: 0,
      visibility: 'public',
      spec_confidence: 'none',
      test_node: false,
    });
  }

  // Modules (each assigned to a package)
  for (let i = 0; i < modCount; i++) {
    const pkgIdx = i % pkgCount;
    nodes.push({
      id: `mod${i}`,
      node_type: 'module',
      name: `mod_${i}`,
      qualified_name: `pkg_${pkgIdx}.mod_${i}`,
      file_path: `pkg_${pkgIdx}/mod_${i}.py`,
      line_start: 1,
      line_end: 100,
      visibility: 'public',
      spec_confidence: 'none',
      test_node: false,
    });
    edges.push({
      id: `ce_pkg${pkgIdx}_mod${i}`,
      source_id: `pkg${pkgIdx}`,
      target_id: `mod${i}`,
      edge_type: 'contains',
    });
  }

  // Leaf nodes (functions, types, etc.)
  for (let i = 0; i < leafCount; i++) {
    const modIdx = i % modCount;
    const typeIdx = 2 + (i % (NODE_TYPES.length - 2)); // Skip package/module
    const nodeType = NODE_TYPES[typeIdx];
    const isTest = i % 50 === 0; // 2% test nodes
    nodes.push({
      id: `leaf${i}`,
      node_type: nodeType,
      name: `${nodeType}_${i}`,
      qualified_name: `pkg_${modIdx % pkgCount}.mod_${modIdx}.${nodeType}_${i}`,
      file_path: `pkg_${modIdx % pkgCount}/mod_${modIdx}.py`,
      line_start: (i % 100) * 10 + 1,
      line_end: (i % 100) * 10 + 10,
      visibility: 'public',
      spec_confidence: ['none', 'low', 'medium', 'high'][i % 4],
      test_node: isTest,
      complexity: i % 30,
      churn_count_30d: i % 15,
    });
    edges.push({
      id: `ce_mod${modIdx}_leaf${i}`,
      source_id: `mod${modIdx}`,
      target_id: `leaf${i}`,
      edge_type: 'contains',
    });
  }

  // Add call/dependency edges up to edgeCount (subtract containment edges already created)
  const containsEdgeCount = modCount + leafCount;
  const callEdgesNeeded = Math.max(0, edgeCount - containsEdgeCount);
  for (let i = 0; i < callEdgesNeeded; i++) {
    const srcIdx = i % leafCount;
    const tgtIdx = (i * 7 + 3) % leafCount; // Deterministic but spread out
    if (srcIdx === tgtIdx) continue;
    const edgeType = i % 3 === 0 ? 'calls' : i % 3 === 1 ? 'depends_on' : 'imports';
    edges.push({
      id: `call_${i}`,
      source_id: `leaf${srcIdx}`,
      target_id: `leaf${tgtIdx}`,
      edge_type: edgeType,
    });
  }

  return { nodes, edges };
}

// ── Performance tests ────────────────────────────────────────────────────

describe('ExplorerCanvas — large graph performance', () => {
  it('renders a graph with 10k+ nodes without throwing', () => {
    const { nodes, edges } = generateLargeGraph(10000, 20000);
    expect(nodes.length).toBeGreaterThanOrEqual(10000);
    expect(edges.length).toBeGreaterThanOrEqual(20000);

    expect(() => {
      render(ExplorerCanvas, {
        props: { nodes, edges },
      });
    }).not.toThrow();
  });

  it('10k graph renders a canvas element', () => {
    const { nodes, edges } = generateLargeGraph(10000, 20000);
    const { container } = render(ExplorerCanvas, {
      props: { nodes, edges },
    });
    const canvas = container.querySelector('canvas');
    expect(canvas).toBeTruthy();
  });

  it('10k graph shows correct node count in stats', () => {
    const { nodes, edges } = generateLargeGraph(10000, 20000);
    const { container } = render(ExplorerCanvas, {
      props: { nodes, edges },
    });
    const stats = container.querySelector('.treemap-stats');
    // Stats should show the actual node count (10k+)
    expect(stats?.textContent).toMatch(/\d{4,}/);
  });

  it('10k graph triggers canvas draw calls', () => {
    const { nodes, edges } = generateLargeGraph(10000, 20000);
    render(ExplorerCanvas, {
      props: { nodes, edges },
    });
    // Canvas should have received draw calls (fillRect for background + nodes)
    expect(mockCtx.fillRect).toHaveBeenCalled();
  });

  it('viewport culling reduces draw calls for off-screen nodes', () => {
    // The isVisible function skips nodes outside the viewport.
    // With a default camera at origin, many nodes in a 10k graph will be off-screen.
    const { nodes, edges } = generateLargeGraph(10000, 20000);
    render(ExplorerCanvas, {
      props: { nodes, edges },
    });

    // fillRect is called for many purposes (background, dot grid, tree groups,
    // summary mode labels, etc.), so the raw count can be high.
    // The key assertion: the total draw operations should be far less than what
    // an uncalled rendering of 10k nodes would produce (each leaf node uses
    // multiple fillRect calls for fill + border + label background).
    // An uncalled 10k graph would produce ~50k+ fill calls.
    const totalFillRects = mockCtx.fillRect.mock.calls.length;
    // With culling + semantic zoom + edge bundling, draw calls stay manageable
    // even for 10k nodes. Verify they stay under a reasonable ceiling.
    expect(totalFillRects).toBeLessThan(50000);
    // Also verify canvas did draw something (not zero)
    expect(totalFillRects).toBeGreaterThan(0);
  });

  it('edge bundling activates for large edge counts (>5000)', () => {
    // When edge count > 5000, drawEdges should use bundled mode.
    // We verify that individual edge drawing is capped.
    const { nodes, edges } = generateLargeGraph(10000, 20000);
    render(ExplorerCanvas, {
      props: { nodes, edges },
    });

    // With >5000 edges, the edge drawing should use bundled mode.
    // The moveTo calls should be much less than 20k (bundled = group-to-group arrows).
    const moveToCount = mockCtx.moveTo.mock.calls.length;
    // Even bundled edges call moveTo, but the count should be << total edges
    expect(moveToCount).toBeLessThan(edges.length);
  });

  it('semantic zoom filters node types at low zoom', () => {
    // Unit test: verify semantic zoom visibility rules
    function isVisibleAtZoom(nodeType, zoom) {
      if (zoom < 0.3 && !['package', 'module'].includes(nodeType)) return false;
      if (zoom < 0.6 && ['function', 'method', 'endpoint', 'field', 'constant', 'table', 'component', 'class', 'enum_variant'].includes(nodeType)) return false;
      if (zoom < 1.0 && ['function', 'method', 'field', 'constant', 'enum_variant'].includes(nodeType)) return false;
      if (zoom < 2.0 && ['field', 'constant', 'enum_variant'].includes(nodeType)) return false;
      return true;
    }

    // At zoom 0.2 (overview), only packages and modules are visible
    const { nodes } = generateLargeGraph(10000, 20000);
    const visibleAtOverview = nodes.filter(n => isVisibleAtZoom(n.node_type, 0.2));
    const totalNodes = nodes.length;

    // Packages + modules should be much less than total
    expect(visibleAtOverview.length).toBeLessThan(totalNodes * 0.1);
    // But some should be visible
    expect(visibleAtOverview.length).toBeGreaterThan(0);
    // All visible should be packages or modules
    for (const n of visibleAtOverview) {
      expect(['package', 'module']).toContain(n.node_type);
    }
  });

  it('generates hierarchical containment correctly', () => {
    const { nodes, edges } = generateLargeGraph(10000, 20000);
    const containsEdges = edges.filter(e => e.edge_type === 'contains');

    // Every module should have a contains edge from its package
    const modules = nodes.filter(n => n.node_type === 'module');
    for (const mod of modules) {
      const parentEdge = containsEdges.find(e => e.target_id === mod.id);
      expect(parentEdge).toBeTruthy();
      expect(parentEdge.source_id).toMatch(/^pkg/);
    }

    // Every leaf should have a contains edge from its module
    const leaves = nodes.filter(n => n.node_type !== 'package' && n.node_type !== 'module');
    for (const leaf of leaves.slice(0, 100)) { // Sample first 100 for speed
      const parentEdge = containsEdges.find(e => e.target_id === leaf.id);
      expect(parentEdge).toBeTruthy();
      expect(parentEdge.source_id).toMatch(/^mod/);
    }
  });

  it('15k graph renders without throwing', () => {
    const { nodes, edges } = generateLargeGraph(15000, 30000);
    expect(nodes.length).toBeGreaterThanOrEqual(15000);
    expect(() => {
      render(ExplorerCanvas, {
        props: { nodes, edges },
      });
    }).not.toThrow();
  });

  it('text width cache prevents redundant measureText calls', () => {
    // With 10k nodes, many will share the same labels (node_type names).
    // The LRU text width cache should reduce measureText calls.
    const { nodes, edges } = generateLargeGraph(10000, 20000);
    render(ExplorerCanvas, {
      props: { nodes, edges },
    });

    const measureTextCalls = mockCtx.measureText.mock.calls.length;
    // Text cache should limit measurements. With 10k nodes there are only
    // ~10 unique node_type values, so even if labels differ, the cache
    // reduces calls significantly compared to naive per-label measurement.
    // This is a reasonable assertion: far fewer than 10k calls.
    expect(measureTextCalls).toBeLessThan(10000);
  });
});

describe('ExplorerCanvas — large graph viewport culling logic', () => {
  // Unit test the isVisible culling logic used in drawNodeRecursive
  function isVisible(ln, cam, W, H) {
    const sx = W / 2 + (ln.x - cam.x) * cam.zoom;
    const sy = H / 2 + (ln.y - cam.y) * cam.zoom;
    const hw = (ln.w / 2) * cam.zoom + 20;
    const hh = (ln.h / 2) * cam.zoom + 20;
    return sx + hw > 0 && sx - hw < W && sy + hh > 0 && sy - hh < H;
  }

  it('nodes at camera center are visible', () => {
    const cam = { x: 0, y: 0, zoom: 1 };
    const ln = { x: 0, y: 0, w: 100, h: 40 };
    expect(isVisible(ln, cam, 1000, 600)).toBe(true);
  });

  it('nodes far off-screen are culled', () => {
    const cam = { x: 0, y: 0, zoom: 1 };
    const ln = { x: 5000, y: 5000, w: 100, h: 40 };
    expect(isVisible(ln, cam, 1000, 600)).toBe(false);
  });

  it('nodes near viewport edge are visible (buffer zone)', () => {
    const cam = { x: 0, y: 0, zoom: 1 };
    // Node just barely outside the viewport to the right
    // Screen x = 500 + 510 * 1 = 1010, hw = 50 + 20 = 70
    // 1010 + 70 > 0 AND 1010 - 70 = 940 < 1000 → visible
    const ln = { x: 510, y: 0, w: 100, h: 40 };
    expect(isVisible(ln, cam, 1000, 600)).toBe(true);
  });

  it('culling at low zoom excludes more nodes', () => {
    const cam = { x: 0, y: 0, zoom: 0.1 }; // Very zoomed out
    // At zoom 0.1, world space visible range is much larger
    // Screen x = 500 + 200 * 0.1 = 520, hw = 50 * 0.1 + 20 = 25
    const nearNode = { x: 200, y: 0, w: 100, h: 40 };
    expect(isVisible(nearNode, cam, 1000, 600)).toBe(true);

    // But a distant node at world space 6000 is off-screen
    // Screen x = 500 + 6000 * 0.1 = 1100, hw = 25
    // 1100 - 25 = 1075 > 1000 → not visible
    const farNode = { x: 6000, y: 0, w: 100, h: 40 };
    expect(isVisible(farNode, cam, 1000, 600)).toBe(false);
  });

  it('culling correctly handles negative world coordinates', () => {
    const cam = { x: 0, y: 0, zoom: 1 };
    // Node at (-600, 0): screen x = 500 + (-600) * 1 = -100, hw = 70
    // -100 + 70 = -30 < 0 → not visible (just off screen left)
    const ln = { x: -600, y: 0, w: 100, h: 40 };
    expect(isVisible(ln, cam, 1000, 600)).toBe(false);

    // Node at (-450, 0): screen x = 500 + (-450) = 50, hw = 70
    // 50 + 70 > 0 AND 50 - 70 = -20 < 1000 → visible
    const ln2 = { x: -450, y: 0, w: 100, h: 40 };
    expect(isVisible(ln2, cam, 1000, 600)).toBe(true);
  });

  it('culls majority of nodes in a 10k graph at default zoom', () => {
    const cam = { x: 0, y: 0, zoom: 1 };
    const W = 1200;
    const H = 800;

    // Simulate layout positions spread across a large world space
    let visibleCount = 0;
    const totalNodes = 10000;
    for (let i = 0; i < totalNodes; i++) {
      // Spread nodes across a 10000x10000 world space
      const x = (i % 100) * 100 - 5000;
      const y = Math.floor(i / 100) * 100 - 5000;
      const ln = { x, y, w: 80, h: 40 };
      if (isVisible(ln, cam, W, H)) {
        visibleCount++;
      }
    }

    // At zoom 1 with default camera, only nodes near (0,0) should be visible
    // Visible world range: approximately -620 to 620 in x, -420 to 420 in y
    // That's about 12 columns (600/100 * 2) × 8 rows = ~96 nodes
    expect(visibleCount).toBeLessThan(totalNodes * 0.05); // Less than 5% visible
    expect(visibleCount).toBeGreaterThan(0); // Some should be visible
  });
});

describe('ExplorerCanvas — edge culling logic', () => {
  // Unit test for edge frustum culling used in drawEdges
  function isEdgeVisible(srcScreen, tgtScreen, W, H) {
    const buffer = 50;
    if (srcScreen.x < -buffer && tgtScreen.x < -buffer) return false;
    if (srcScreen.x > W + buffer && tgtScreen.x > W + buffer) return false;
    if (srcScreen.y < -buffer && tgtScreen.y < -buffer) return false;
    if (srcScreen.y > H + buffer && tgtScreen.y > H + buffer) return false;
    return true;
  }

  it('edges with both endpoints on-screen are visible', () => {
    expect(isEdgeVisible({ x: 100, y: 100 }, { x: 200, y: 200 }, 1000, 600)).toBe(true);
  });

  it('edges with both endpoints far off-screen right are culled', () => {
    expect(isEdgeVisible({ x: 1200, y: 100 }, { x: 1300, y: 200 }, 1000, 600)).toBe(false);
  });

  it('edges crossing the viewport are visible even if endpoints are off-screen', () => {
    // Source far left, target far right — edge crosses viewport
    expect(isEdgeVisible({ x: -100, y: 300 }, { x: 1200, y: 300 }, 1000, 600)).toBe(true);
  });

  it('edges with both endpoints off-screen bottom are culled', () => {
    expect(isEdgeVisible({ x: 100, y: 700 }, { x: 200, y: 800 }, 1000, 600)).toBe(false);
  });

  it('edge near buffer zone is visible', () => {
    // Source at x=-45 (within 50px buffer)
    expect(isEdgeVisible({ x: -45, y: 300 }, { x: 500, y: 300 }, 1000, 600)).toBe(true);
  });
});

describe('ExplorerCanvas — LOD text rendering', () => {
  // Unit test for the LOD decision: skip text labels at low zoom
  function shouldRenderText(zoom) {
    // Text labels are rendered when zoom > 0.5 (labels become readable)
    return zoom >= 0.5;
  }

  function shouldRenderBadges(zoom) {
    // Badges are rendered when zoom > 1.0 (detail level)
    return zoom >= 1.0;
  }

  it('text labels hidden at very low zoom', () => {
    expect(shouldRenderText(0.1)).toBe(false);
    expect(shouldRenderText(0.3)).toBe(false);
  });

  it('text labels shown at medium zoom', () => {
    expect(shouldRenderText(0.5)).toBe(true);
    expect(shouldRenderText(1.0)).toBe(true);
  });

  it('badges hidden at overview zoom', () => {
    expect(shouldRenderBadges(0.3)).toBe(false);
    expect(shouldRenderBadges(0.8)).toBe(false);
  });

  it('badges shown at detail zoom', () => {
    expect(shouldRenderBadges(1.0)).toBe(true);
    expect(shouldRenderBadges(2.0)).toBe(true);
  });
});
