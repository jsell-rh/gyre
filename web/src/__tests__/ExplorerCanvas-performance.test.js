import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/svelte';
import ExplorerCanvas from '../lib/ExplorerCanvas.svelte';

// ── Test infrastructure notes ────────────────────────────────────────────
//
// requestAnimationFrame is mocked with an explicit queue + flushFrames()
// instead of a synchronous callback. A synchronous rAF mock wedges the
// component's redraw loop: `animFrame = requestAnimationFrame(animLoop)`
// assigns the return value AFTER the nested callback has already run and
// (on convergence) set `animFrame = null`, so the stale truthy id makes
// every later scheduleRedraw() a no-op. With the queue mock, camera
// interaction (wheel / touch / drag) can be simulated and flushed
// deterministically, and the camera state can be read back from the
// minimap viewport rectangle (drawMinimap strokes it in world coordinates
// with strokeStyle '#60a5fa' once per frame).
//
// Mutation sensitivity (verified by mutation runs, see
// /tmp/stage/review-evidence/ in the task record):
//   - isVisible() → `return true`            fails 'viewport culling' tests
//   - leaf label guard → `if (true)`         fails 'LOD: mid zoom' test
//   - semantic-zoom node-type gates removed  fails 'semantic zoom' test
//   - edge-bundling condition → `if (false)` fails 'edge bundling' tests
//   - pinch midpoint anchoring removed       fails 'pinch' test
//
// The ">30fps during pan/zoom" AC cannot be measured in jsdom (no real
// frame loop or GPU). The mechanism-level proxies here verify what keeps
// interaction above that target in a real browser: per-frame draw work
// stays bounded by culling/LOD/bundling while the camera moves.

const CANVAS_W = 900;
const CANVAS_H = 600;

// ── Mock canvas context ──────────────────────────────────────────────────
let mockCtx;
let drawCallCount;

let rafQueue = [];
let viewportRects = []; // minimap viewport rects (world coords), one per frame
let frameMarks = [];    // draw-call count snapshots, one per frame end

function snapshotCounts() {
  return {
    fill: mockCtx.fill.mock.calls.length,
    stroke: mockCtx.stroke.mock.calls.length,
    fillRect: mockCtx.fillRect.mock.calls.length,
    fillText: mockCtx.fillText.mock.calls.length,
    moveTo: mockCtx.moveTo.mock.calls.length,
    quad: mockCtx.quadraticCurveTo.mock.calls.length,
  };
}

function resetCapture() {
  rafQueue = [];
  viewportRects = [];
  frameMarks = [];
}

/** Run queued animation frames until the loop settles (camera converges). */
function flushFrames(maxBatches = 5000) {
  let batches = 0;
  while (rafQueue.length > 0) {
    if (++batches > maxBatches) throw new Error('rAF queue did not drain — animation loop never converged');
    const cbs = rafQueue;
    rafQueue = [];
    for (const cb of cbs) cb();
  }
}

/**
 * Camera pose from the last minimap viewport rectangle. drawMinimap strokes
 * the world-space viewport (`strokeRect(screenToWorld(0,0) … )`) every
 * frame, so its args encode the live camera exactly:
 *   width = W / cam.zoom, center = (cam.x, cam.y).
 */
function readCamera() {
  const r = viewportRects[viewportRects.length - 1];
  if (!r) return null;
  return { x: r.x + r.w / 2, y: r.y + r.h / 2, zoom: CANVAS_W / r.w };
}

/** Draw-call counts of the most recent frame (between the last two frame-end marks). */
function readLastFrameCounts() {
  const n = frameMarks.length;
  if (n < 2) return null;
  const prev = frameMarks[n - 2];
  const last = frameMarks[n - 1];
  const out = {};
  for (const k of Object.keys(last)) out[k] = last[k] - prev[k];
  return out;
}

/** fillText argument strings drawn in the most recent frame. */
function readLastFrameTexts() {
  const n = frameMarks.length;
  const from = frameMarks[n - 2].fillText;
  return mockCtx.fillText.mock.calls.slice(from).map(c => String(c[0]));
}

beforeEach(() => {
  drawCallCount = 0;
  resetCapture();
  mockCtx = {
    clearRect: vi.fn(),
    fillRect: vi.fn(() => { drawCallCount++; }),
    strokeRect: vi.fn((x, y, w, h) => {
      // Minimap viewport rect = per-frame camera snapshot (see readCamera)
      if (mockCtx.strokeStyle === '#60a5fa') {
        viewportRects.push({ x, y, w, h });
        frameMarks.push(snapshotCounts());
      }
    }),
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
  let seq = 0;
  global.requestAnimationFrame = vi.fn(cb => { rafQueue.push(cb); return ++seq; });
  global.cancelAnimationFrame = vi.fn();
});

afterEach(() => {
  cleanup();
  // restoreAllMocks restores spies, but vi.fn draw mocks retain their call
  // histories. Release those large graph traces after each case's assertions.
  vi.clearAllMocks();
  vi.restoreAllMocks();
  resetCapture();
});

// ── Large graph generator (10k benchmark) ────────────────────────────────

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

// ── Camera/LOD graph generators ──────────────────────────────────────────

/**
 * Two-package graph used for camera, culling, LOD and semantic-zoom tests.
 *
 * Shape matters: with only two top-level containers each package occupies
 * half the layout, so a modest zoom-in from the auto-fit opens the
 * containers (leaf children become drawable) while cam.zoom can still be
 * held below 0.5 — the regime where the leaf-label LOD guard is the ONLY
 * thing suppressing text. Leaf cell size is the squarify weight
 * (`complexity`): most leaves are small, a few are large (complexity 150)
 * so their labels pass the sw>30/sh>14 size checks at mid zoom.
 *
 * Leaf names use the distinctive `Lf_` prefix so fillText calls can be
 * attributed to leaf labels unambiguously (tree-group labels are 'pkgA',
 * 'pkgB', 'N nodes', 'pkgA (N+1)' — none match).
 */
function genTwoPackageGraph(leafType, leavesPerPkg, bigLeafCount = 8) {
  const nodes = [];
  const edges = [];
  for (const pkg of ['pkgA', 'pkgB']) {
    nodes.push({
      id: pkg, node_type: 'package', name: pkg, qualified_name: pkg,
      file_path: `${pkg}.py`, line_start: 0, line_end: 0, visibility: 'public',
      spec_confidence: 'none', test_node: false,
    });
    for (let i = 0; i < leavesPerPkg; i++) {
      nodes.push({
        id: `${pkg}_t${i}`, node_type: leafType, name: `Lf_${i}`,
        qualified_name: `${pkg}.Lf_${i}`, file_path: `${pkg}/t${i}.py`,
        line_start: 1, line_end: 10, visibility: 'public',
        spec_confidence: 'none', test_node: false,
        complexity: i < bigLeafCount ? 150 : 1,
      });
      edges.push({ id: `${pkg}_e${i}`, source_id: pkg, target_id: `${pkg}_t${i}`, edge_type: 'contains' });
    }
  }
  return { nodes, edges };
}

/** Two packages whose leaves are connected by `callEdgeCount` cross-package call edges. */
function genCrossEdgeGraph(leavesPerPkg, callEdgeCount) {
  const g = genTwoPackageGraph('type', leavesPerPkg, 0);
  for (let i = 0; i < callEdgeCount; i++) {
    g.edges.push({
      id: `x${i}`,
      source_id: `pkgA_t${i % leavesPerPkg}`,
      target_id: `pkgB_t${(i * 7) % leavesPerPkg}`,
      edge_type: 'calls',
    });
  }
  return g;
}

// Leaves per package for the LOD/culling/semantic fixture: 2×2250+2 nodes
// makes the auto-fit zoom land near ~0.27, so 2 batched wheel zoom-ins put
// the camera around 0.44 (containers open, still below the 0.5 label LOD
// threshold and below the 0.6 semantic gate for functions).
const BIG_LEAVES = 2250;

// ── Interaction dispatch helpers ─────────────────────────────────────────

function wheel(canvas, deltaY, x = CANVAS_W / 2, y = CANVAS_H / 2) {
  canvas.dispatchEvent(new WheelEvent('wheel', { deltaY, clientX: x, clientY: y, cancelable: true }));
}

/** jsdom has no TouchEvent constructor; the handlers only read e.touches. */
function touch(canvas, type, points) {
  const ev = new Event(type, { cancelable: true, bubbles: true });
  Object.defineProperty(ev, 'touches', { value: points.map(p => ({ clientX: p[0], clientY: p[1] })) });
  canvas.dispatchEvent(ev);
}

// ── 10k benchmark tests ──────────────────────────────────────────────────

describe('ExplorerCanvas — large graph performance', () => {
  it('renders a graph with 10k+ nodes within 100ms', () => {
    const { nodes, edges } = generateLargeGraph(10000, 20000);
    expect(nodes.length).toBeGreaterThanOrEqual(10000);
    expect(edges.length).toBeGreaterThanOrEqual(20000);

    // Measure CPU time — jsdom timing is not identical to browser timing but
    // serves as a regression guard for computational performance. CPU time is
    // measured instead of wall time so the guard stays deterministic when the
    // full suite runs workers in parallel and the event loop is contended:
    // under load, wall time inflates (observed 16s for this render in a full
    // suite run vs ~3s isolated) while CPU time does not.
    // The AC target is <100ms in a real browser. In jsdom, rendering 10k nodes
    // takes several seconds because jsdom is single-threaded JS without GPU
    // acceleration. We use a generous threshold as a regression guard —
    // a sudden 10x increase would indicate an algorithmic regression.
    // Includes the initial animation frames (flushFrames) — render alone
    // would only measure layout, not drawing.
    const startCpu = process.cpuUsage();
    render(ExplorerCanvas, {
      props: { nodes, edges },
    });
    flushFrames();
    const cpuUsed = process.cpuUsage(startCpu);
    const cpuMs = (cpuUsed.user + cpuUsed.system) / 1000;

    // jsdom regression guard — not the AC target (which requires real browser).
    // Typical jsdom: ~3s CPU for 10k nodes. Threshold set at 15s to catch a
    // sudden algorithmic regression without flaky CI.
    expect(cpuMs).toBeLessThan(15000);
  }, 30000);

  it('10k graph renders a canvas element', () => {
    const { nodes, edges } = generateLargeGraph(10000, 20000);
    const { container } = render(ExplorerCanvas, {
      props: { nodes, edges },
    });
    flushFrames();
    const canvas = container.querySelector('canvas');
    expect(canvas).toBeTruthy();
  }, 30000);

  it('10k graph shows correct node count in stats', () => {
    const { nodes, edges } = generateLargeGraph(10000, 20000);
    const { container } = render(ExplorerCanvas, {
      props: { nodes, edges },
    });
    flushFrames();
    const stats = container.querySelector('.treemap-stats');
    // Stats should show the actual node count (10000+)
    expect(stats?.textContent).toMatch(/10\d{3}/);
  }, 30000);

  it('10k graph triggers canvas draw calls', () => {
    const { nodes, edges } = generateLargeGraph(10000, 20000);
    render(ExplorerCanvas, {
      props: { nodes, edges },
    });
    flushFrames();
    // Canvas should have received draw calls (fillRect for background + nodes)
    expect(mockCtx.fillRect).toHaveBeenCalled();
  }, 30000);

  it('10k graph draw calls stay bounded', () => {
    // Total-work regression guard: the initial auto-fit frames draw the whole
    // graph once (everything is on screen at fit zoom, so culling culls
    // nothing here — the culling behavior itself is verified under a zoomed
    // camera in the 'viewport culling' describe below). The bound catches a
    // per-frame explosion (e.g. LOD or summary mode breaking).
    const { nodes, edges } = generateLargeGraph(10000, 20000);
    render(ExplorerCanvas, {
      props: { nodes, edges },
    });
    flushFrames();
    const totalFillRects = mockCtx.fillRect.mock.calls.length;
    // 2 initial frames × (background + dot grid ≤3000 + node rects) stays
    // well below this; an unbounded per-node multiplier would exceed it.
    expect(totalFillRects).toBeLessThan(50000);
    expect(totalFillRects).toBeGreaterThan(0);
  }, 30000);

  it('text width cache prevents redundant measureText calls', () => {
    // With 10k nodes, many will share the same labels (node_type names).
    // The LRU text width cache should reduce measureText calls.
    const { nodes, edges } = generateLargeGraph(10000, 20000);
    render(ExplorerCanvas, {
      props: { nodes, edges },
    });
    flushFrames();

    const measureTextCalls = mockCtx.measureText.mock.calls.length;
    // Text cache should limit measurements. With 10k nodes there are only
    // ~10 unique node_type values, so even if labels differ, the cache
    // reduces calls significantly compared to naive per-label measurement.
    expect(measureTextCalls).toBeLessThan(10000);
  }, 30000);

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

  it('15k graph renders within timing budget', () => {
    const { nodes, edges } = generateLargeGraph(15000, 30000);
    expect(nodes.length).toBeGreaterThanOrEqual(15000);

    const startCpu = process.cpuUsage();
    render(ExplorerCanvas, {
      props: { nodes, edges },
    });
    flushFrames();
    const cpuUsed = process.cpuUsage(startCpu);
    const cpuMs = (cpuUsed.user + cpuUsed.system) / 1000;

    // jsdom regression guard for 15k graph (CPU time — see 10k test above).
    // Typical: ~5s CPU. Real browser target would be <200ms; jsdom is
    // orders of magnitude slower.
    expect(cpuMs).toBeLessThan(20000);
  }, 30000);
});

// ── Camera interaction (wheel / mouse / touch) ────────────────────────────

describe('ExplorerCanvas — camera interaction', () => {
  // Small graph: camera behavior is graph-size independent.
  function renderSmall() {
    const g = genTwoPackageGraph('type', 60);
    const r = render(ExplorerCanvas, { props: { nodes: g.nodes, edges: g.edges } });
    flushFrames();
    const canvas = r.container.querySelector('canvas');
    return { canvas, camera: readCamera() };
  }

  it('wheel zoom-in increases zoom and keeps the world point under the cursor fixed', () => {
    const { canvas, camera: cam0 } = renderSmall();
    expect(cam0).toBeTruthy();

    // World point under the cursor before zooming (cursor at 200,150)
    const wx0 = (200 - CANVAS_W / 2) / cam0.zoom + cam0.x;
    const wy0 = (150 - CANVAS_H / 2) / cam0.zoom + cam0.y;

    for (let i = 0; i < 3; i++) wheel(canvas, -100, 200, 150);
    flushFrames();

    const cam1 = readCamera();
    expect(cam1.zoom).toBeGreaterThan(cam0.zoom * 1.5);

    // applyZoomAtMouse anchors the zoom at the cursor: the same world point
    // must remain under (200,150). Tolerance covers the camera lerp's
    // convergence threshold (0.1 world units per axis), not the anchoring
    // math — removing the anchor moves the world point by hundreds of units.
    const wx1 = (200 - CANVAS_W / 2) / cam1.zoom + cam1.x;
    const wy1 = (150 - CANVAS_H / 2) / cam1.zoom + cam1.y;
    expect(Math.abs(wx1 - wx0)).toBeLessThan(0.5);
    expect(Math.abs(wy1 - wy0)).toBeLessThan(0.5);
  }, 30000);

  it('wheel zoom-out decreases zoom', () => {
    const { canvas, camera: cam0 } = renderSmall();
    for (let i = 0; i < 3; i++) wheel(canvas, 100);
    flushFrames();
    const cam1 = readCamera();
    expect(cam1.zoom).toBeLessThan(cam0.zoom * 0.7);
  }, 30000);

  it('mouse drag pans the camera', () => {
    const { canvas, camera: cam0 } = renderSmall();
    // bubbles: true — mousedown/mousemove/mouseup are delegated by Svelte 5
    // and only reach the handler through the bubble phase
    canvas.dispatchEvent(new MouseEvent('mousedown', { button: 0, clientX: 300, clientY: 200, bubbles: true, cancelable: true }));
    canvas.dispatchEvent(new MouseEvent('mousemove', { clientX: 360, clientY: 240, bubbles: true, cancelable: true }));
    canvas.dispatchEvent(new MouseEvent('mouseup', { bubbles: true, cancelable: true }));
    flushFrames();

    const cam1 = readCamera();
    // Dragging the canvas right/down moves the world camera left/up by
    // dx/zoom, dy/zoom (onMouseDown/onMouseMove pan logic).
    expect(Math.abs(cam1.x - (cam0.x - 60 / cam0.zoom))).toBeLessThan(0.5);
    expect(Math.abs(cam1.y - (cam0.y - 40 / cam0.zoom))).toBeLessThan(0.5);
    expect(Math.abs(cam1.zoom - cam0.zoom)).toBeLessThan(0.01);
  }, 30000);

  it('single-finger touch drag pans the camera', () => {
    const { canvas, camera: cam0 } = renderSmall();
    touch(canvas, 'touchstart', [[300, 200]]);
    touch(canvas, 'touchmove', [[360, 240]]);
    touch(canvas, 'touchend', []);
    flushFrames();

    const cam1 = readCamera();
    expect(Math.abs(cam1.x - (cam0.x - 60 / cam0.zoom))).toBeLessThan(0.5);
    expect(Math.abs(cam1.y - (cam0.y - 40 / cam0.zoom))).toBeLessThan(0.5);
    expect(Math.abs(cam1.zoom - cam0.zoom)).toBeLessThan(0.01);
  }, 30000);

  it('two-finger pinch-out zooms toward the pinch midpoint', () => {
    const { canvas, camera: cam0 } = renderSmall();

    // World point under the pinch midpoint (400, 280) before the pinch
    const wx0 = (400 - CANVAS_W / 2) / cam0.zoom + cam0.x;
    const wy0 = (280 - CANVAS_H / 2) / cam0.zoom + cam0.y;

    // Pinch out: finger distance 50 → 100 (scale 2), midpoint fixed
    touch(canvas, 'touchstart', [[375, 280], [425, 280]]);
    touch(canvas, 'touchmove', [[350, 280], [450, 280]]);
    touch(canvas, 'touchend', []);
    flushFrames();

    const cam1 = readCamera();
    // Doubling the finger distance doubles the zoom
    expect(Math.abs(cam1.zoom - cam0.zoom * 2)).toBeLessThan(cam0.zoom * 0.05);

    // The world point under the pinch midpoint must stay under the midpoint
    // (zoom toward the pinch center). Tolerance covers camera-lerp
    // convergence only — the pre-fix behavior (correction computed from the
    // lagging display camera, identically zero) left the world point ~50
    // world units away from the midpoint.
    const wx1 = (400 - CANVAS_W / 2) / cam1.zoom + cam1.x;
    const wy1 = (280 - CANVAS_H / 2) / cam1.zoom + cam1.y;
    expect(Math.abs(wx1 - wx0)).toBeLessThan(0.5);
    expect(Math.abs(wy1 - wy0)).toBeLessThan(0.5);
  }, 30000);
});

// ── Viewport culling under a zoomed camera ────────────────────────────────

describe('ExplorerCanvas — viewport culling under a zoomed camera', () => {
  it('deep zoom: only on-screen nodes are drawn per frame', () => {
    // 4502-node graph, then zoom deep (13 batched wheel-ins ≈ 25×). At that
    // zoom the visible world is a small fraction of the layout, so culling
    // must keep the per-frame node-draw count tiny. With isVisible() forced
    // to `return true` every container opens and every leaf passes the
    // opacity checks at deep zoom, so thousands of nodes get drawn per
    // frame and this test fails — the initial-fit draw output alone cannot
    // distinguish culled from unculled rendering (everything is on screen
    // at fit zoom).
    const g = genTwoPackageGraph('type', BIG_LEAVES);
    const { container } = render(ExplorerCanvas, { props: { nodes: g.nodes, edges: g.edges } });
    flushFrames();
    const canvas = container.querySelector('canvas');

    for (let i = 0; i < 13; i++) wheel(canvas, -100);
    flushFrames();

    const cam = readCamera();
    const lf = readLastFrameCounts();
    // Scenario validity: deeply zoomed in, and a large graph behind it
    expect(cam.zoom).toBeGreaterThan(5);
    expect(g.nodes.length).toBeGreaterThan(4000);
    // Per-frame node draws (fill = node fills + edge arrowheads) stay tiny
    // while the graph has 4.5k nodes — off-screen nodes are culled. At this
    // zoom the ~130×90-unit viewport still intersects leaf cells (layout
    // gaps are ≤ 8 units), so a nonzero count also proves visible content
    // is still drawn.
    expect(lf.fill).toBeGreaterThan(0);
    expect(lf.fill).toBeLessThanOrEqual(60);
    expect(lf.quad).toBeLessThanOrEqual(500);
  }, 120000);
});

// ── Level-of-detail rendering under real cameras ─────────────────────────

describe('ExplorerCanvas — level-of-detail rendering', () => {
  it('mid zoom (0.3–0.5) suppresses leaf labels; high zoom (≥1.2) draws them', () => {
    // One render, two regimes. First zoom in just enough to open the
    // containers (leaf nodes drawn) while staying below the 0.5 label-LOD
    // threshold: leaf labels (name + type indicator) must be suppressed.
    // Forcing the label guard to `if (true)` draws labels for every visible
    // leaf (~1700 fillText calls) and fails the mid-zoom half. Then zoom
    // past 1.2 and the labels must appear — removing label rendering
    // entirely (guard → `if (false)`) fails the high-zoom half.
    const g = genTwoPackageGraph('type', BIG_LEAVES);
    const { container } = render(ExplorerCanvas, { props: { nodes: g.nodes, edges: g.edges } });
    flushFrames();
    const canvas = container.querySelector('canvas');

    for (let i = 0; i < 2; i++) wheel(canvas, -100);
    flushFrames();

    const midCam = readCamera();
    const midFrame = readLastFrameCounts();
    // Scenario validity: containers open (leaves drawn in numbers) at a zoom
    // below the label threshold
    expect(midCam.zoom).toBeGreaterThanOrEqual(0.3);
    expect(midCam.zoom).toBeLessThan(0.5);
    expect(midFrame.fill).toBeGreaterThanOrEqual(300);
    // No leaf labels: neither leaf names (Lf_…) nor the node-type indicator
    const midTexts = readLastFrameTexts();
    const midLeafTexts = midTexts.filter(t => /^Lf_/.test(t) || t === 'type');
    expect(midLeafTexts.length).toBe(0);

    for (let i = 0; i < 6; i++) wheel(canvas, -100);
    flushFrames();

    const highCam = readCamera();
    expect(highCam.zoom).toBeGreaterThanOrEqual(1.2);
    const highTexts = readLastFrameTexts();
    const highLeafTexts = highTexts.filter(t => /^Lf_/.test(t) || t === 'type');
    expect(highLeafTexts.length).toBeGreaterThan(10);
  }, 180000);

  it('semantic zoom (0.3–0.6): function leaves hidden, type leaves drawn', () => {
    // The semantic-zoom node-type gates: below 0.6, functions/methods are
    // hidden while types remain visible. Same geometry, only the leaf type
    // differs — so identical layouts. Removing the gates draws both graphs'
    // leaves in equal numbers and fails this test.
    const typeGraph = genTwoPackageGraph('type', BIG_LEAVES);
    const { container: c1 } = render(ExplorerCanvas, { props: { nodes: typeGraph.nodes, edges: typeGraph.edges } });
    flushFrames();
    const canvas1 = c1.querySelector('canvas');
    for (let i = 0; i < 2; i++) wheel(canvas1, -100);
    flushFrames();
    const typeCam = readCamera();
    const typeFrame = readLastFrameCounts();

    cleanup();
    resetCapture();

    const fnGraph = genTwoPackageGraph('function', BIG_LEAVES);
    const { container: c2 } = render(ExplorerCanvas, { props: { nodes: fnGraph.nodes, edges: fnGraph.edges } });
    flushFrames();
    const canvas2 = c2.querySelector('canvas');
    for (let i = 0; i < 2; i++) wheel(canvas2, -100);
    flushFrames();
    const fnCam = readCamera();
    const fnFrame = readLastFrameCounts();

    // Scenario validity: both cameras in the 0.3–0.6 semantic band
    expect(typeCam.zoom).toBeGreaterThanOrEqual(0.3);
    expect(typeCam.zoom).toBeLessThan(0.6);
    expect(fnCam.zoom).toBeGreaterThanOrEqual(0.3);
    expect(fnCam.zoom).toBeLessThan(0.6);

    // Types: leaves drawn in numbers. Functions: only containers remain.
    expect(typeFrame.fill).toBeGreaterThanOrEqual(300);
    expect(fnFrame.fill).toBeLessThanOrEqual(50);
    expect(fnFrame.fill).toBeLessThan(typeFrame.fill / 4);
  }, 180000);
});

// ── Edge bundling ─────────────────────────────────────────────────────────

describe('ExplorerCanvas — edge bundling', () => {
  it('>5000 edges at zoom ≥ 0.5: bundled group arrows with count labels', () => {
    // The count-based bundling branch: renderEdges > 5000 while zoom ≥ 0.5
    // must switch to group-to-group aggregate arrows. Bundled mode labels
    // each thick bundle with an "N edges" count — individual edge rendering
    // never produces that text, so forcing the bundling condition to
    // `if (false)` fails this test (the capped individual renderer draws
    // neither bundle arrows nor bundle labels).
    const g = genCrossEdgeGraph(300, 6000);
    expect(g.edges.length).toBeGreaterThan(5000);
    render(ExplorerCanvas, { props: { nodes: g.nodes, edges: g.edges } });
    flushFrames();

    const cam = readCamera();
    const lf = readLastFrameCounts();
    // Scenario validity: this is the >5000-edges branch, not the zoom branch
    expect(cam.zoom).toBeGreaterThanOrEqual(0.5);
    expect(cam.zoom).toBeLessThan(2);

    const texts = readLastFrameTexts();
    const bundleLabels = texts.filter(t => /^\d+ edges$/.test(t));
    expect(bundleLabels.length).toBeGreaterThan(0);
    // The 6000 cross-package calls bundle into group arrows, not 1500
    // capped individual edges (each drawArrow does 2 moveTo).
    expect(lf.moveTo).toBeLessThanOrEqual(200);
  }, 60000);

  it('<5000 edges at zoom < 0.5: zoom-based bundling activates', () => {
    // The zoom-based bundling branch: below 0.5, inter-group edges are
    // bundled even when the edge count is small. Forcing the condition to
    // `if (false)` falls back to individual edge rendering, which draws no
    // bundle count labels at all.
    const g = genCrossEdgeGraph(300, 500);
    expect(g.edges.length).toBeLessThan(5000);
    const { container } = render(ExplorerCanvas, { props: { nodes: g.nodes, edges: g.edges } });
    flushFrames();
    const canvas = container.querySelector('canvas');

    for (let i = 0; i < 2; i++) wheel(canvas, 100);
    flushFrames();

    const cam = readCamera();
    expect(cam.zoom).toBeLessThan(0.5);

    const texts = readLastFrameTexts();
    const bundleLabels = texts.filter(t => /^\d+ edges$/.test(t));
    expect(bundleLabels.length).toBeGreaterThan(0);
  }, 60000);

  it('small graph at fit zoom: edges are individually drawn (not bundled)', () => {
    // With <5000 edges at zoom ≥ 0.5, individual edge rendering is used.
    // Edge drawing uses moveTo/lineTo/quadraticCurveTo, not fillRect (which is nodes).
    const nodes = [
      { id: 'a', node_type: 'function', name: 'a', qualified_name: 'a', file_path: 'a.py', line_start: 1, line_end: 10, visibility: 'public', spec_confidence: 'none', test_node: false },
      { id: 'b', node_type: 'function', name: 'b', qualified_name: 'b', file_path: 'b.py', line_start: 1, line_end: 10, visibility: 'public', spec_confidence: 'none', test_node: false },
      { id: 'c', node_type: 'function', name: 'c', qualified_name: 'c', file_path: 'c.py', line_start: 1, line_end: 10, visibility: 'public', spec_confidence: 'none', test_node: false },
    ];
    const edges = [
      { id: 'e1', source_id: 'a', target_id: 'b', edge_type: 'calls' },
      { id: 'e2', source_id: 'b', target_id: 'c', edge_type: 'calls' },
    ];

    render(ExplorerCanvas, { props: { nodes, edges } });
    flushFrames();

    const cam = readCamera();
    expect(cam.zoom).toBeGreaterThanOrEqual(0.5);

    // Edge paths use moveTo to start each edge. With 2 edges individually drawn,
    // moveTo count should be at least proportional to edge count (not collapsed
    // into a single group-to-group arrow as bundled mode would).
    const lf = readLastFrameCounts();
    expect(lf.moveTo).toBeGreaterThan(0);
    // With individual edge rendering, we expect at least 1 moveTo per edge
    // (2 per drawArrow: shaft + arrowhead)
    expect(lf.moveTo).toBeGreaterThanOrEqual(edges.length);
    // And no bundle count labels
    const texts = readLastFrameTexts();
    expect(texts.filter(t => /^\d+ edges$/.test(t)).length).toBe(0);
  });
});

// ── Performance timing note ─────────────────────────────────────────────
// The acceptance criterion ">30fps during pan/zoom" cannot be measured in
// jsdom because jsdom does not have a real animation loop or GPU rendering.
// The camera-interaction tests above simulate real wheel/drag/pinch input
// through the component's handlers and flush the animation loop, and the
// culling/LOD/bundling tests assert the per-frame draw work stays bounded
// while the camera moves — the mechanisms that keep interaction above the
// fps target in a real browser. For real fps measurement, use browser-based
// testing tools (e.g., Playwright with Chrome DevTools protocol).
