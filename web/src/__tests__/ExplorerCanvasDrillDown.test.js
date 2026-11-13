/**
 * ExplorerCanvasDrillDown.test.js — explorer-canvas.md §5 Progressive Drill-Down
 *
 * Covers the spec's drill-down contract end-to-end against the real component
 * (real layout pipeline, real hit-testing, real breadcrumb state):
 *
 *   Level 0: all packages → double-click "greeting-service"
 *   Level 1: modules inside it (lib, greeting) → double-click "greeting"
 *   Level 2: types inside (GreetingService, User) → double-click "GreetingService"
 *   Level 3: methods/fields (greet, config) → click "greet" → detail panel
 *
 * Breadcrumb: e2e-repo > greeting-service > greeting > GreetingService, each
 * segment navigates back up. The zoom transition must be animated (camera
 * interpolation + fade-out of unrelated nodes), not a jump-cut. The lens
 * toggle must work at every drill level.
 *
 * These tests intentionally drive the component through real mouse events and
 * discover node positions via the hover tooltip (the same hit-testing path a
 * user's cursor takes) instead of reaching into component internals.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render } from '@testing-library/svelte';
import ExplorerCanvas from '../lib/ExplorerCanvas.svelte';

// ── jsdom canvas mocks ──────────────────────────────────────────────────
// The 2d context is recorded when instrumentation is active so tests can
// assert on what was actually drawn (camera interpolation, fade alphas).
let mockCtx;
let recorder; // null when not recording

function startRecording() {
  recorder = { frames: [] };
}
function stopRecording() {
  const r = recorder;
  recorder = null;
  return r;
}

beforeEach(() => {
  mockCtx = {
    clearRect: vi.fn(), fillRect: vi.fn(), strokeRect: vi.fn(),
    beginPath: vi.fn(), closePath: vi.fn(), arc: vi.fn(),
    fill: vi.fn(), stroke: vi.fn(), moveTo: vi.fn(), lineTo: vi.fn(),
    quadraticCurveTo: vi.fn(), fillText: vi.fn(),
    measureText: vi.fn(() => ({ width: 40 })),
    scale: vi.fn(), setTransform: vi.fn(), save: vi.fn(), restore: vi.fn(),
    translate: vi.fn(), rotate: vi.fn(),
    fillStyle: '', strokeStyle: '', lineWidth: 0, globalAlpha: 1,
    font: '', textAlign: '', textBaseline: '',
    shadowColor: '', shadowBlur: 0,
    setLineDash: vi.fn(), getLineDash: vi.fn(() => []),
    roundRect: vi.fn(),
  };
  // Instrument fillRect/strokeRect:
  //  - fillRect(0,0,900,600) is the main-canvas background → starts a frame
  //  - fillRect(0,0,180,110) is the minimap background → main-canvas segment done
  //  - fillRect with 0.8×0.8 is a dot-grid dot → camera pose observable
  //  - larger rects are node cells → record draw alpha (globalAlpha)
  mockCtx.fillRect.mockImplementation((x, y, w, h) => {
    if (!recorder) return;
    if (w === 900 && h === 600) {
      recorder.cur = { firstDot: null, nodeAlphas: [], mainDone: false };
      recorder.frames.push(recorder.cur);
      return;
    }
    if (!recorder.cur) return;
    if (w === 180 && h === 110) {
      recorder.cur.mainDone = true; // minimap draws pollute alphas — stop
      return;
    }
    if (w === 0.8 && h === 0.8) {
      if (recorder.cur.firstDot === null) recorder.cur.firstDot = { x, y };
    } else if (!recorder.cur.mainDone && w > 2 && h > 2) {
      recorder.cur.nodeAlphas.push(mockCtx.globalAlpha);
    }
  });
  mockCtx.strokeRect.mockImplementation((x, y, w, h) => {
    if (!recorder || !recorder.cur || recorder.cur.mainDone) return;
    if (w > 2 && h > 2) recorder.cur.nodeAlphas.push(mockCtx.globalAlpha);
  });
  // Node cells are drawn path-based (roundRect helper + fill/stroke), so
  // record draw opacity at fill()/stroke() time as well.
  mockCtx.fill.mockImplementation(() => {
    if (!recorder || !recorder.cur || recorder.cur.mainDone) return;
    recorder.cur.nodeAlphas.push(mockCtx.globalAlpha);
  });
  mockCtx.stroke.mockImplementation(() => {
    if (!recorder || !recorder.cur || recorder.cur.mainDone) return;
    recorder.cur.nodeAlphas.push(mockCtx.globalAlpha);
  });
  HTMLCanvasElement.prototype.getContext = vi.fn(() => mockCtx);
  global.ResizeObserver = class ResizeObserver {
    observe() {} disconnect() {} unobserve() {}
  };
  global.requestAnimationFrame = cb => { return setTimeout(() => cb(performance.now()), 16); };
  global.cancelAnimationFrame = id => { clearTimeout(id); };
});

afterEach(() => {
  vi.restoreAllMocks();
});

// ── Spec §5 fixture: repo > package > module > type > method/field ──────
const NODES = [
  { id: 'pkg-gs', node_type: 'package', name: 'greeting-service', qualified_name: 'greeting-service', file_path: '', line_start: 0, line_end: 0, visibility: 'public', spec_confidence: 'none', test_node: false },
  { id: 'mod-lib', node_type: 'module', name: 'lib', qualified_name: 'greeting-service.lib', file_path: 'lib.rs', line_start: 1, line_end: 10, visibility: 'public', spec_confidence: 'none', test_node: false },
  { id: 'mod-greet', node_type: 'module', name: 'greeting', qualified_name: 'greeting-service.greeting', file_path: 'greeting.rs', line_start: 1, line_end: 200, visibility: 'public', spec_confidence: 'none', test_node: false },
  { id: 'type-svc', node_type: 'type', name: 'GreetingService', qualified_name: 'greeting-service.greeting.GreetingService', file_path: 'greeting/service.rs', line_start: 1, line_end: 80, visibility: 'public', spec_confidence: 'high', test_node: false },
  { id: 'fn-greet', node_type: 'function', name: 'greet', qualified_name: 'greeting-service.greeting.GreetingService.greet', file_path: 'greeting/service.rs', line_start: 10, line_end: 30, visibility: 'public', spec_confidence: 'high', test_node: false },
  { id: 'field-config', node_type: 'field', name: 'config', qualified_name: 'greeting-service.greeting.GreetingService.config', file_path: 'greeting/service.rs', line_start: 5, line_end: 5, visibility: 'public', spec_confidence: 'none', test_node: false },
  { id: 'type-user', node_type: 'type', name: 'User', qualified_name: 'greeting-service.greeting.User', file_path: 'greeting/models.rs', line_start: 1, line_end: 20, visibility: 'public', spec_confidence: 'none', test_node: false },
  { id: 'pkg-other', node_type: 'package', name: 'other-service', qualified_name: 'other-service', file_path: '', line_start: 0, line_end: 0, visibility: 'public', spec_confidence: 'none', test_node: false },
  { id: 'fn-other', node_type: 'function', name: 'other_fn', qualified_name: 'other-service.other_fn', file_path: 'other/lib.rs', line_start: 1, line_end: 5, visibility: 'public', spec_confidence: 'none', test_node: false },
];

const EDGES = [
  { id: 'c1', source_id: 'pkg-gs', target_id: 'mod-lib', edge_type: 'contains' },
  { id: 'c2', source_id: 'pkg-gs', target_id: 'mod-greet', edge_type: 'contains' },
  { id: 'c3', source_id: 'mod-greet', target_id: 'type-svc', edge_type: 'contains' },
  { id: 'c4', source_id: 'mod-greet', target_id: 'type-user', edge_type: 'contains' },
  { id: 'c5', source_id: 'type-svc', target_id: 'fn-greet', edge_type: 'contains' },
  { id: 'c6', source_id: 'type-svc', target_id: 'field-config', edge_type: 'contains' },
  { id: 'c7', source_id: 'pkg-other', target_id: 'fn-other', edge_type: 'contains' },
];

function sleep(ms) { return new Promise(r => setTimeout(r, ms)); }

/**
 * Sweep the canvas with mousemove events and discover node positions via the
 * hover tooltip — the same hit-testing path a user's cursor takes. When
 * `wanted` is provided, the sweep stops early once every wanted name has been
 * found; otherwise it sweeps the full grid (needed for absence assertions).
 */
async function scanNodePositions(container, wanted = null, step = 20) {
  const canvas = container.querySelector('canvas.treemap-canvas');
  const found = new Map();
  const wantedLeft = wanted ? new Set(wanted) : null;
  for (let x = 20; x < 900; x += step) {
    for (let y = 20; y < 600; y += step) {
      canvas.dispatchEvent(new MouseEvent('mousemove', { clientX: x, clientY: y, bubbles: true }));
      await new Promise(r => setTimeout(r, 0));
      const tip = container.querySelector('.tooltip-name');
      if (tip) {
        const name = tip.textContent;
        if (!found.has(name)) {
          found.set(name, { x, y });
          if (wantedLeft) {
            wantedLeft.delete(name);
            if (wantedLeft.size === 0) return found;
          }
        }
      }
    }
  }
  return found;
}

function dblclick(canvas, x, y) {
  canvas.dispatchEvent(new MouseEvent('dblclick', { clientX: x, clientY: y, bubbles: true }));
}

function click(canvas, x, y) {
  canvas.dispatchEvent(new MouseEvent('mousedown', { clientX: x, clientY: y, bubbles: true }));
  canvas.dispatchEvent(new MouseEvent('mouseup', { clientX: x, clientY: y, bubbles: true }));
  canvas.dispatchEvent(new MouseEvent('click', { clientX: x, clientY: y, bubbles: true }));
}

function crumbTexts(container) {
  return Array.from(container.querySelectorAll('.breadcrumb-item')).map(c => c.textContent.trim());
}

// Phase 1 (zoom into node) + phase 2 (settle on new level) each take
// DRILL_TRANSITION_MS; wait past both before asserting scope state.
const DRILL_SETTLE_MS = 800;

describe('ExplorerCanvas — §5 progressive drill-down', () => {
  it('double-click drills the Contains hierarchy level by level; breadcrumb segments navigate back up; leaf double-click opens the detail panel', async () => {
    const onNodeDetail = vi.fn();
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, onNodeDetail, rootLabel: 'e2e-repo' },
    });
    await sleep(200);
    const canvas = container.querySelector('canvas.treemap-canvas');

    // ── Level 0: all packages are on the canvas ──────────────────────
    const l0 = await scanNodePositions(container, ['greeting-service', 'other-service']);
    expect(l0.has('greeting-service')).toBe(true);
    expect(l0.has('other-service')).toBe(true);

    // ── Level 0 → 1: double-click the package ────────────────────────
    dblclick(canvas, l0.get('greeting-service').x, l0.get('greeting-service').y);
    await sleep(DRILL_SETTLE_MS);
    expect(crumbTexts(container)).toEqual(['e2e-repo', 'greeting-service']);
    // Only the package's Contains children (its modules) remain; the sibling
    // package and everything outside the drilled subtree are filtered out.
    // Full sweep: absence of other-service is the Contains-filter assertion.
    const l1 = await scanNodePositions(container);
    expect(l1.has('greeting')).toBe(true);
    expect(l1.has('lib')).toBe(true);
    expect(l1.has('other-service')).toBe(false);
    expect(l1.has('User')).toBe(false);

    // Lens toggle works at this drill level (structural ↔ evaluative) —
    // checkbox group (ui-navigation.md §3): Evaluative is a toggle overlay.
    const lensChecks = Array.from(container.querySelectorAll('.lens-group input[type="checkbox"]'));
    const evaluative = lensChecks.find((i) => i.closest('label').textContent.includes('Evaluative'));
    const structural = lensChecks.find((i) => i.closest('label').textContent.includes('Structural'));
    expect(evaluative && structural).toBeTruthy();
    await evaluative.click();
    expect(evaluative.checked).toBe(true);
    expect(container.querySelector('.eval-no-trace')).toBeTruthy(); // no trace data → message, not a crash
    await evaluative.click();
    expect(evaluative.checked).toBe(false);

    // ── Level 1 → 2: double-click the module ─────────────────────────
    dblclick(canvas, l1.get('greeting').x, l1.get('greeting').y);
    await sleep(DRILL_SETTLE_MS);
    expect(crumbTexts(container)).toEqual(['e2e-repo', 'greeting-service', 'greeting']);
    const l2 = await scanNodePositions(container, ['GreetingService', 'User']);
    expect(l2.has('GreetingService')).toBe(true);
    expect(l2.has('User')).toBe(true);

    // ── Level 2 → 3: double-click the type ───────────────────────────
    dblclick(canvas, l2.get('GreetingService').x, l2.get('GreetingService').y);
    await sleep(DRILL_SETTLE_MS);
    expect(crumbTexts(container)).toEqual(['e2e-repo', 'greeting-service', 'greeting', 'GreetingService']);
    const l3 = await scanNodePositions(container, ['greet', 'config']);
    expect(l3.has('greet')).toBe(true);
    expect(l3.has('config')).toBe(true);

    // ── Leaf nodes do not drill: click → detail panel ────────────────
    onNodeDetail.mockClear();
    click(canvas, l3.get('greet').x, l3.get('greet').y);
    await sleep(50);
    expect(onNodeDetail).toHaveBeenCalledTimes(1);
    expect(onNodeDetail).toHaveBeenCalledWith(expect.objectContaining({ id: 'fn-greet', node_type: 'function' }));

    onNodeDetail.mockClear();
    dblclick(canvas, l3.get('greet').x, l3.get('greet').y);
    await sleep(DRILL_SETTLE_MS);
    // Double-click on a leaf opens the detail panel, it does not drill:
    expect(onNodeDetail).toHaveBeenCalledWith(expect.objectContaining({ id: 'fn-greet' }));
    expect(crumbTexts(container)).toEqual(['e2e-repo', 'greeting-service', 'greeting', 'GreetingService']);

    // ── Breadcrumb segments navigate back up (reverse transition) ─────
    const segments = container.querySelectorAll('.breadcrumb-item');
    // [root, greeting-service, greeting, GreetingService] — click the module
    segments[2].click();
    await sleep(DRILL_SETTLE_MS);
    expect(crumbTexts(container)).toEqual(['e2e-repo', 'greeting-service', 'greeting']);
    const backAtModule = await scanNodePositions(container, ['GreetingService', 'User']);
    expect(backAtModule.has('GreetingService')).toBe(true);
    expect(backAtModule.has('User')).toBe(true);

    segments[1].click();
    await sleep(DRILL_SETTLE_MS);
    expect(crumbTexts(container)).toEqual(['e2e-repo', 'greeting-service']);
    const backAtPackage = await scanNodePositions(container, ['greeting', 'lib']);
    expect(backAtPackage.has('greeting')).toBe(true);

    container.querySelector('.breadcrumb-item.root').click();
    await sleep(DRILL_SETTLE_MS);
    expect(crumbTexts(container)).toEqual([]);
    const backAtRoot = await scanNodePositions(container, ['greeting-service', 'other-service']);
    expect(backAtRoot.has('greeting-service')).toBe(true);
    expect(backAtRoot.has('other-service')).toBe(true);
  }, 120_000);

  it('drill transition is animated: camera interpolates frames and unrelated nodes fade out and recover (not a jump-cut)', async () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, rootLabel: 'e2e-repo' },
    });
    await sleep(300);
    const canvas = container.querySelector('canvas.treemap-canvas');
    const pkg = (await scanNodePositions(container, ['greeting-service'])).get('greeting-service');
    expect(pkg).toBeTruthy();

    // Dispatch a few mousemoves so redraw frames run while recording.
    async function wiggle(ms) {
      const t0 = Date.now();
      let i = 0;
      while (Date.now() - t0 < ms) {
        canvas.dispatchEvent(new MouseEvent('mousemove', { clientX: 450 + (i % 3) * 10, clientY: 300 + (i % 2) * 10, bubbles: true }));
        i++;
        await sleep(30);
      }
    }

    // Baseline: at rest, node draws are at their normal opacity (≥ 0.4 at
    // this layout: top-level groups 1.0, nested groups ~0.4).
    startRecording();
    await wiggle(300);
    const baseline = stopRecording();
    const baselineAlphas = baseline.frames.flatMap(f => f.nodeAlphas);
    expect(baselineAlphas.length).toBeGreaterThan(0);
    // Resting opacity floor at this layout (small sub-groups ~0.2, tops 1.0)
    expect(Math.min(...baselineAlphas)).toBeGreaterThanOrEqual(0.15);

    // During the drill: camera must move through intermediate poses (the
    // first dot-grid position differs across frames — one distinct value
    // means either a jump-cut or a NaN camera, which makes every rect
    // "contain" every point), and unrelated nodes must fade out (draw
    // opacity falls far below any resting opacity).
    startRecording();
    dblclick(canvas, pkg.x, pkg.y);
    await sleep(700);
    const during = stopRecording();
    expect(during.frames.length).toBeGreaterThan(4);
    const dotPos = during.frames.map(f => f.firstDot).filter(Boolean);
    const distinctDots = new Set(dotPos.map(d => `${d.x},${d.y}`));
    expect(distinctDots.size).toBeGreaterThanOrEqual(3);
    const duringAlphas = during.frames.flatMap(f => f.nodeAlphas);
    expect(duringAlphas.length).toBeGreaterThan(0);
    // Fade-out: by the end of the window, unrelated nodes draw far below
    // any resting opacity (drillFadeAlpha floors at 0.12).
    expect(Math.min(...duringAlphas)).toBeLessThan(0.1);

    // After the drill settles, opacity recovers (fade back in).
    await sleep(2300);
    startRecording();
    await wiggle(300);
    const after = stopRecording();
    const afterAlphas = after.frames.flatMap(f => f.nodeAlphas);
    expect(afterAlphas.length).toBeGreaterThan(0);
    // Recovered: the bulk of draws is bright again (top-level group at full
    // opacity) and clearly brighter than the dimmed during-window bulk.
    const pct = (arr, p) => arr.slice().sort((a, b) => a - b)[Math.floor(p * (arr.length - 1))];
    expect(Math.max(...afterAlphas)).toBeGreaterThanOrEqual(0.9);
    expect(pct(afterAlphas, 0.75)).toBeGreaterThan(pct(duringAlphas, 0.75));
  }, 60_000);

  it('lens toggle works at a drilled level: evaluative overlay shows span stats scoped to current-level nodes (deep-linked)', async () => {
    const traceData = {
      spans: [
        { span_id: 's1', parent_span_id: null, operation_name: 'greet', start_time: 0, duration_us: 2000, status: 'ok', graph_node_id: 'fn-greet' },
        { span_id: 's2', parent_span_id: null, operation_name: 'greet', start_time: 10, duration_us: 6000, status: 'error', graph_node_id: 'fn-greet' },
        { span_id: 's3', parent_span_id: null, operation_name: 'config', start_time: 20, duration_us: 500, status: 'ok', graph_node_id: 'field-config' },
        // Span on a node OUTSIDE this drill level — must not surface here.
        { span_id: 's4', parent_span_id: null, operation_name: 'user', start_time: 30, duration_us: 99999, status: 'ok', graph_node_id: 'type-user' },
      ],
      root_spans: ['s1', 's2', 's3', 's4'],
    };
    const { container } = render(ExplorerCanvas, {
      props: {
        nodes: NODES,
        edges: EDGES,
        lens: 'evaluative',
        traceData,
        rootLabel: 'e2e-repo',
        // Deep-link straight to Level 3 (GreetingService) — the same state a
        // drill leaves in canvasState, restorable from a URL.
        canvasState: { selectedNode: null, zoom: 1, visibleGroups: [], breadcrumb: [{ id: 'type-svc', name: 'GreetingService' }] },
      },
    });
    await sleep(300);

    // The drilled level restored: only GreetingService's members are present.
    expect(crumbTexts(container)).toEqual(['e2e-repo', 'GreetingService']);
    const l3 = await scanNodePositions(container);
    expect(l3.has('greet')).toBe(true);
    expect(l3.has('config')).toBe(true);
    expect(l3.has('User')).toBe(false); // outside the drill level
    expect(l3.has('GreetingService')).toBe(false); // scope root is replaced by its children

    // Evaluative lens UI is active at this level.
    expect(container.querySelector('.eval-metric-group')).toBeTruthy();
    expect(container.querySelector('.trace-playback-bar')).toBeTruthy();

    // Hovering a current-level node shows ITS span stats (evaluative overlay
    // scoped to spans touching nodes at this level): greet p50 = 4.0ms with a
    // 50% error rate; config p50 = 500µs with none.
    async function hoverAssert(x, y, checks) {
      const canvas = container.querySelector('canvas.treemap-canvas');
      canvas.dispatchEvent(new MouseEvent('mousemove', { clientX: x, clientY: y, bubbles: true }));
      await sleep(40);
      const stats = container.querySelector('.tooltip-span-stats');
      expect(stats).toBeTruthy();
      for (const [re, should] of Object.entries(checks)) {
        if (should) expect(stats.textContent).toMatch(new RegExp(re));
        else expect(stats.textContent).not.toMatch(new RegExp(re));
      }
    }
    await hoverAssert(l3.get('greet').x, l3.get('greet').y, {
      'p50: 4\\.0ms': true,
      'Error rate: 50%': true,
      '99\\.9ms': false, // User's span must not leak into this level
    });
    await hoverAssert(l3.get('config').x, l3.get('config').y, {
      'p50: 500µs': true,
      'Error rate': false,
    });
  }, 60_000);
});
