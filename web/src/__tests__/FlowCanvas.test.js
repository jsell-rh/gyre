/**
 * task-015 FlowCanvas — component tests for the flow-layout renderer
 * (ui-layout.md §4 Flow Layout, human-system-interface.md §3a):
 *
 * - SVG node/edge layer (accessible, clickable) with a Canvas 2D / WebGL2
 *   particle overlay on top — and ExplorerCanvas delegating to it for
 *   layout: "flow" views
 * - playback controls (play/pause/step/speed/test selector), freeze-on-scrub
 * - node badges (span count, error rate, mean duration)
 * - click node / click particle during animation (pause + detail payload)
 * - particle hover tooltip (I/O summary), trail rendering
 * - >100 concurrent particles switching the overlay to WebGL2
 * - double-click drill scoping
 *
 * Pure span→particle logic is covered by flow-particles.test.js; the
 * layout by flow-layout.test.js; the 2D/WebGL painter by
 * particle-layer.test.js. These tests pin the component wiring.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import FlowCanvas from '../lib/FlowCanvas.svelte';
import ExplorerCanvas from '../lib/ExplorerCanvas.svelte';

let mockCtx;
let glMock;
let rafQueue = [];
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
    font: '', textAlign: '', textBaseline: '', shadowColor: '', shadowBlur: 0,
    setLineDash: vi.fn(), getLineDash: vi.fn(() => []),
    roundRect: vi.fn(), clip: vi.fn(), ellipse: vi.fn(),
    strokeText: vi.fn(), createLinearGradient: vi.fn(() => ({ addColorStop: vi.fn() })),
    filter: '', lineCap: '',
  };
  glMock = {
    VERTEX_SHADER: 1, FRAGMENT_SHADER: 2, ARRAY_BUFFER: 3, DYNAMIC_DRAW: 4,
    FLOAT: 5, COLOR_BUFFER_BIT: 16, BLEND: 17, SRC_ALPHA: 18,
    ONE_MINUS_SRC_ALPHA: 19, LINES: 20, POINTS: 21,
    createShader: vi.fn(() => ({})), shaderSource: vi.fn(), compileShader: vi.fn(),
    createProgram: vi.fn(() => ({})), attachShader: vi.fn(), linkProgram: vi.fn(),
    useProgram: vi.fn(), getUniformLocation: vi.fn(() => ({})),
    uniform2f: vi.fn(), uniform1f: vi.fn(),
    getAttribLocation: vi.fn(() => 0), createBuffer: vi.fn(() => ({})),
    bindBuffer: vi.fn(), bufferData: vi.fn(), enableVertexAttribArray: vi.fn(),
    vertexAttribPointer: vi.fn(), enable: vi.fn(), blendFunc: vi.fn(),
    viewport: vi.fn(), clearColor: vi.fn(), clear: vi.fn(), drawArrays: vi.fn(),
    getExtension: vi.fn(() => undefined),
  };
  rafQueue = [];
  HTMLCanvasElement.prototype.getContext = vi.fn((type) =>
    type === 'webgl2' ? glMock : mockCtx);
  global.ResizeObserver = class ResizeObserver { observe() {} disconnect() {} unobserve() {} };
  global.requestAnimationFrame = vi.fn((cb) => { rafQueue.push(cb); return rafQueue.length; });
  global.cancelAnimationFrame = vi.fn();
});
afterEach(() => { vi.restoreAllMocks(); });

async function flushFrames(times = [], maxFrames = 500) {
  let ran = 0;
  const drain = (ts) => {
    while (rafQueue.length > 0 && ran < maxFrames) {
      const batch = rafQueue.splice(0);
      for (const cb of batch) { ran++; cb(ts); }
    }
  };
  if (times.length === 0) {
    drain(performance.now());
  } else {
    for (const ts of times) drain(ts);
  }
  await new Promise((r) => setTimeout(r, 0));
}

// ── Fixtures ──────────────────────────────────────────────────────────────

const DN = (id, name, node_type = 'function', extra = {}) => ({
  id, node_type, name, qualified_name: `api.${name}`,
  file_path: 'api/handlers.py', line_start: 0, line_end: 0,
  visibility: 'public', spec_confidence: 'none', test_node: false,
  ...extra,
});

const NODES = [
  DN('pkg1', 'api', 'package'),
  DN('fn1', 'create_user'),
  DN('fn2', 'get_user'),
  DN('fn3', 'delete_user'),
];
const EDGES = [
  { id: 'e1', source_id: 'pkg1', target_id: 'fn1', edge_type: 'contains' },
  { id: 'e2', source_id: 'pkg1', target_id: 'fn2', edge_type: 'contains' },
  { id: 'e3', source_id: 'pkg1', target_id: 'fn3', edge_type: 'contains' },
  { id: 'e4', source_id: 'fn1', target_id: 'fn2', edge_type: 'calls' },
  { id: 'e5', source_id: 'fn1', target_id: 'fn3', edge_type: 'calls' },
];

// Two test cases (root spans) over that graph. Trace timeline [0, 10000]µs:
//   s1 'test_create' (fn1) → s2 fn1→fn2 [1000,5000], s3 fn1→fn3 [2000,5000]
//   s4 'test_read'   (fn1) → s5 fn1→fn2 [6000,9000]
const TRACE = {
  spans: [
    { span_id: 's1', parent_span_id: null, operation_name: 'test_create', start_time: 0, duration_us: 10000, status: 'ok', graph_node_id: 'fn1' },
    { span_id: 's2', parent_span_id: 's1', operation_name: 'create_user', start_time: 1000, duration_us: 4000, status: 'ok', graph_node_id: 'fn2', input_summary: 'user payload', output_summary: 'user id' },
    { span_id: 's3', parent_span_id: 's1', operation_name: 'validate', start_time: 2000, duration_us: 3000, status: 'error', graph_node_id: 'fn3' },
    { span_id: 's4', parent_span_id: null, operation_name: 'test_read', start_time: 0, duration_us: 10000, status: 'ok', graph_node_id: 'fn1' },
    { span_id: 's5', parent_span_id: 's4', operation_name: 'get_user', start_time: 6000, duration_us: 3000, status: 'ok', graph_node_id: 'fn2' },
  ],
  root_spans: ['s1', 's4'],
};

const activeSpansText = (container) =>
  container.querySelector('.trace-pb-particles')?.textContent ?? '';

const setScrubber = async (container, permille) => {
  const input = container.querySelector('.trace-pb-scrubber');
  expect(input).toBeTruthy();
  await fireEvent.input(input, { target: { value: String(permille) } });
  await new Promise((r) => setTimeout(r, 0));
};

// ── SVG layer + particle overlay architecture ────────────────────────────

describe('FlowCanvas — SVG layer with particle overlay', () => {
  it('renders nodes/edges as accessible SVG with a Canvas particle overlay on top', () => {
    const { container } = render(FlowCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData: TRACE },
    });
    const svg = container.querySelector('svg.flow-svg');
    expect(svg).toBeTruthy();
    // Nodes: role=button, keyboard focusable, labelled.
    const nodeGs = container.querySelectorAll('g[data-flow-node][role="button"]');
    expect(nodeGs.length).toBe(4);
    const labels = [...nodeGs].map((g) => g.getAttribute('aria-label'));
    expect(labels.some((l) => l.includes('create_user'))).toBe(true);
    expect(nodeGs[0].hasAttribute('tabindex')).toBe(true);
    // Edges drawn as SVG paths.
    expect(container.querySelectorAll('path[data-flow-edge]').length).toBe(5);
    // Particle overlay: a canvas above the SVG, particles only.
    const overlay = container.querySelector('.flow-particles');
    expect(overlay).toBeTruthy();
    const overlayCanvas = overlay.querySelector('canvas');
    expect(overlayCanvas).toBeTruthy();
    expect(overlayCanvas.getAttribute('data-particle-mode')).toBe('2d');
    // The overlay is transparent to pointer events — interactions reach the
    // SVG node/edge layer beneath (particles are not interactive per spec).
    expect(overlayCanvas.style.pointerEvents).toBe('none');
  });

  it('renders node badges with span count, error rate and mean duration', async () => {
    const { container } = render(FlowCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData: TRACE },
    });
    await flushFrames();
    // fn2: two spans (s2 ok 4000µs, s5 ok 3000µs) → 2 spans, 0% err,
    // mean 3500µs = 3.5ms.
    const fn2 = container.querySelector('g[data-flow-node="fn2"]');
    expect(fn2).toBeTruthy();
    const badgeText = [...fn2.querySelectorAll('.flow-badge-text')].map((t) => t.textContent);
    expect(badgeText).toContain('2 spans');
    expect(badgeText).toContain('0% err');
    expect(badgeText).toContain('3.5ms');
    // fn3: one error span → 100% err, 3000µs mean.
    const fn3 = container.querySelector('g[data-flow-node="fn3"]');
    const fn3Badges = [...fn3.querySelectorAll('.flow-badge-text')].map((t) => t.textContent);
    expect(fn3Badges).toContain('1 spans');
    expect(fn3Badges).toContain('100% err');
    expect(fn3Badges).toContain('3.0ms');
    // Structural lens shows no badges (pure structure).
    const structural = render(FlowCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'structural', traceData: TRACE },
    });
    expect(structural.container.querySelector('.flow-badge')).toBeFalsy();
  });

  it('ExplorerCanvas delegates flow-layout rendering to FlowCanvas', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, layout: 'flow', lens: 'evaluative', traceData: TRACE },
    });
    // FlowCanvas's SVG layer rendered, the 2D graph canvas did not.
    expect(container.querySelector('svg.flow-svg')).toBeTruthy();
    expect(container.querySelector('canvas.treemap-canvas')).toBeNull();
    // Exactly one playback control bar (FlowCanvas's) — ExplorerCanvas's
    // own bar is guarded off in flow layout.
    expect(container.querySelectorAll('.trace-playback-bar').length).toBe(1);
  });
});

// ── Playback (freeze on scrub, filter, step) ─────────────────────────────

describe('FlowCanvas — playback', () => {
  it('dragging the scrubber while paused repositions particles to that timestamp', async () => {
    const { container } = render(FlowCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData: TRACE },
    });
    await flushFrames();
    expect(activeSpansText(container)).toBe('0 active spans');
    await setScrubber(container, 300); // t=3000µs: s2 (0.5) + s3 (1/3)
    expect(activeSpansText(container)).toBe('2 active spans');
    await setScrubber(container, 700); // t=7000µs: only s5
    expect(activeSpansText(container)).toBe('1 active spans');
    await setScrubber(container, 950);
    expect(activeSpansText(container)).toBe('0 active spans');
  });

  it('the test selector and legend filter which root spans animate', async () => {
    const { container } = render(FlowCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData: TRACE },
    });
    await flushFrames();
    await setScrubber(container, 700); // t=7000: only s5 (test_read)
    expect(activeSpansText(container)).toBe('1 active spans');

    const testSel = container.querySelector('.trace-pb-test');
    expect(testSel).toBeTruthy();
    await fireEvent.change(testSel, { target: { value: 's1' } }); // test_create
    await new Promise((r) => setTimeout(r, 0));
    expect(activeSpansText(container)).toBe('0 active spans');

    // Legend items carry the hue-shifted per-case swatch.
    const dots = [...container.querySelectorAll('.trace-pb-legend-dot')];
    expect(dots.length).toBe(2);
    expect(dots[0].getAttribute('style')).not.toBe(dots[1].getAttribute('style'));

    await fireEvent.change(testSel, { target: { value: 'all' } });
    await new Promise((r) => setTimeout(r, 0));
    expect(activeSpansText(container)).toBe('1 active spans');
  });

  it('Step advances to the next span start and pauses playback', async () => {
    const { container } = render(FlowCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData: TRACE },
    });
    await flushFrames();
    const playBtn = container.querySelector('.trace-pb-btn');
    await fireEvent.click(playBtn);
    await new Promise((r) => setTimeout(r, 0));
    expect(playBtn.querySelectorAll('svg rect').length).toBe(2); // pause glyph

    await fireEvent.click(container.querySelector('.trace-pb-step'));
    await new Promise((r) => setTimeout(r, 0));
    expect(playBtn.querySelectorAll('svg rect').length).toBe(0); // play glyph
    expect(activeSpansText(container)).toBe('1 active spans'); // s2 @1000µs

    await fireEvent.click(container.querySelector('.trace-pb-step'));
    await new Promise((r) => setTimeout(r, 0));
    expect(activeSpansText(container)).toBe('2 active spans'); // + s3 @2000µs
  });

  it('a playbackRequest nonce starts playback automatically', async () => {
    const { container } = render(FlowCanvas, {
      props: {
        nodes: NODES, edges: EDGES, lens: 'evaluative', traceData: TRACE,
        playbackRequest: { nonce: 1 },
      },
    });
    // Two separate flushes: each gets a fresh frame budget (the rAF loop
    // re-schedules while playing, so a single flushFrames call spends its
    // whole budget on same-timestamp frames).
    await flushFrames([16]);
    await flushFrames([66]);
    const playBtn = container.querySelector('.trace-pb-btn');
    expect(playBtn.querySelectorAll('svg rect').length).toBe(2); // playing
    expect(activeSpansText(container)).toBe('1 active spans');
  });
});

// ── Interaction during animation ─────────────────────────────────────────

describe('FlowCanvas — interaction during animation', () => {
  const MOD = { id: 'mod-a', node_type: 'module', name: 'svc', qualified_name: 'svc', file_path: 'svc.rs', line_start: 1, line_end: 10, visibility: 'public', spec_confidence: 'none', test_node: false };
  const FN = { id: 'fn-b', node_type: 'function', name: 'handler', qualified_name: 'svc.handler', file_path: 'svc.rs', line_start: 12, line_end: 20, visibility: 'public', spec_confidence: 'none', test_node: false };
  const EDGES_B = [
    { id: 'c1', source_id: 'mod-a', target_id: 'fn-b', edge_type: 'contains' },
    { id: 'c2', source_id: 'mod-a', target_id: 'fn-b', edge_type: 'calls' },
  ];
  const TRACE_B = {
    spans: [
      { span_id: 'r1', parent_span_id: null, operation_name: 'test_request', start_time: 0, duration_us: 10000, status: 'ok', graph_node_id: 'mod-a' },
      { span_id: 's9', parent_span_id: 'r1', operation_name: 'handle', start_time: 0, duration_us: 10000, status: 'ok', graph_node_id: 'fn-b', input_summary: 'GET /users', output_summary: '200 OK' },
    ],
    root_spans: ['r1'],
  };

  it('clicking a node during animation pauses playback and opens the detail panel with the nearest span', async () => {
    const onNodeDetail = vi.fn();
    const { container } = render(FlowCanvas, {
      props: { nodes: [MOD, FN], edges: EDGES_B, lens: 'evaluative', traceData: TRACE_B, onNodeDetail },
    });
    await flushFrames([16]);
    const playBtn = container.querySelector('.trace-pb-btn');
    await fireEvent.click(playBtn);
    await flushFrames([66]);
    expect(activeSpansText(container)).toBe('1 active spans');

    const fnG = container.querySelector('g[data-flow-node="fn-b"]');
    await fireEvent.click(fnG);
    await new Promise((r) => setTimeout(r, 0));

    // Playback paused.
    expect(playBtn.querySelectorAll('svg rect').length).toBe(0);
    // Detail payload carries the span at/nearest the clicked node.
    const arg = onNodeDetail.mock.calls[0][0];
    expect(arg.id).toBe('fn-b');
    expect(arg._activeSpan.span_id).toBe('s9');
    expect(arg._activeSpan.operation_name).toBe('handle');
  });

  it('hovering a particle shows the span I/O tooltip; clicking it pauses and shows the span detail', async () => {
    const onNodeDetail = vi.fn();
    const { container } = render(FlowCanvas, {
      props: { nodes: [MOD, FN], edges: EDGES_B, lens: 'evaluative', traceData: TRACE_B, onNodeDetail },
    });
    await flushFrames([16]);
    // Play one frame so the particle is in flight and drawn.
    await fireEvent.click(container.querySelector('.trace-pb-btn'));
    await flushFrames([66]);
    expect(activeSpansText(container)).toBe('1 active spans');

    // The drawn particle body position (mockCtx.arc captured it).
    const arcs = mockCtx.arc.mock.calls;
    expect(arcs.length).toBeGreaterThan(0);
    const [px, py] = arcs[arcs.length - 1];
    const svg = container.querySelector('svg.flow-svg');

    // Hover: I/O summary tooltip (HSI §3a).
    await fireEvent.mouseMove(svg, { clientX: px, clientY: py });
    await new Promise((r) => setTimeout(r, 0));
    const tip = container.querySelector('.flow-tooltip');
    expect(tip).toBeTruthy();
    expect(tip.textContent).toContain('handle');
    expect(tip.textContent).toContain('GET /users');
    expect(tip.textContent).toContain('200 OK');

    // Click (mousedown first so the pan-drag guard sees a zero-length drag).
    await fireEvent.mouseDown(svg, { clientX: px, clientY: py });
    await fireEvent.click(svg, { clientX: px, clientY: py });
    await new Promise((r) => setTimeout(r, 0));
    const arg = onNodeDetail.mock.calls[0][0];
    expect(arg.node_type).toBe('span');
    expect(arg.span_id).toBe('s9');
    expect(arg.input_summary).toBe('GET /users');
    // Clicking a particle paused playback.
    expect(container.querySelector('.trace-pb-btn').querySelectorAll('svg rect').length).toBe(0);
  });

  it('double-click drills into a container node and the scope bar navigates back', async () => {
    const { container } = render(FlowCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'structural' },
    });
    expect(container.querySelectorAll('g[data-flow-node]').length).toBe(4);
    const pkg = container.querySelector('g[data-flow-node="pkg1"]');
    await fireEvent.dblClick(pkg);
    await new Promise((r) => setTimeout(r, 0));
    // Scoped to pkg1's contains-children.
    expect(container.querySelectorAll('g[data-flow-node]').length).toBe(3);
    const bar = container.querySelector('.flow-scope-bar');
    expect(bar).toBeTruthy();
    expect(bar.textContent).toContain('api');
    // Navigate back to the root scope.
    await fireEvent.click(bar.querySelector('.flow-scope-crumb'));
    await new Promise((r) => setTimeout(r, 0));
    expect(container.querySelectorAll('g[data-flow-node]').length).toBe(4);
    expect(container.querySelector('.flow-scope-bar')).toBeFalsy();
  });

  it('clicking an edge during animation lists the spans that traversed it', async () => {
    const onNodeDetail = vi.fn();
    const { container } = render(FlowCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData: TRACE, onNodeDetail },
    });
    await flushFrames();
    // fn1 → fn2 was traversed by s2 and s5.
    const edge = container.querySelector('path[data-flow-edge="calls:fn1->fn2"]');
    expect(edge).toBeTruthy();
    await fireEvent.mouseDown(edge);
    await fireEvent.click(edge);
    await new Promise((r) => setTimeout(r, 0));
    const arg = onNodeDetail.mock.calls[0][0];
    expect(arg.node_type).toBe('edge');
    expect(arg.traversing_spans.map((s) => s.span_id).sort()).toEqual(['s2', 's5']);
  });
});

// ── Trail + WebGL switch ─────────────────────────────────────────────────

describe('FlowCanvas — trail and WebGL overlay', () => {
  const MOD = { id: 'mod-a', node_type: 'module', name: 'svc', qualified_name: 'svc', file_path: 'svc.rs', line_start: 1, line_end: 10, visibility: 'public', spec_confidence: 'none', test_node: false };
  const FN = { id: 'fn-b', node_type: 'function', name: 'handler', qualified_name: 'svc.handler', file_path: 'svc.rs', line_start: 12, line_end: 20, visibility: 'public', spec_confidence: 'none', test_node: false };
  const EDGES_B = [
    { id: 'c1', source_id: 'mod-a', target_id: 'fn-b', edge_type: 'contains' },
    { id: 'c2', source_id: 'mod-a', target_id: 'fn-b', edge_type: 'calls' },
  ];
  const TRACE_B = {
    spans: [
      { span_id: 'r1', parent_span_id: null, operation_name: 'test_request', start_time: 0, duration_us: 10000, status: 'ok', graph_node_id: 'mod-a' },
      { span_id: 's9', parent_span_id: 'r1', operation_name: 'handle', start_time: 0, duration_us: 10000, status: 'ok', graph_node_id: 'fn-b' },
    ],
    root_spans: ['r1'],
  };

  it('draws a fading trail behind moving particles, in the particle color', async () => {
    const trailStrokes = [];
    const particleFills = [];
    mockCtx.stroke = vi.fn(() => {
      if (mockCtx.strokeStyle === '#3b82f6') {
        trailStrokes.push({ alpha: mockCtx.globalAlpha, cap: mockCtx.lineCap });
      }
    });
    mockCtx.fill = vi.fn(() => {
      if (mockCtx.fillStyle === '#3b82f6') {
        particleFills.push(mockCtx.globalAlpha);
      }
    });
    const { container } = render(FlowCanvas, {
      props: { nodes: [MOD, FN], edges: EDGES_B, lens: 'evaluative', traceData: TRACE_B },
    });
    await flushFrames([16]);
    await fireEvent.click(container.querySelector('.trace-pb-btn'));
    await flushFrames([66, 116, 166, 216, 266]);
    expect(particleFills.length).toBeGreaterThan(0);
    expect(trailStrokes.length).toBeGreaterThan(0);
    for (const t of trailStrokes) {
      expect(t.cap).toBe('round');
      expect(t.alpha).toBeGreaterThan(0);
      expect(t.alpha).toBeLessThanOrEqual(0.6);
    }
  });

  it('switches the particle overlay to WebGL2 above 100 concurrent particles', async () => {
    // 150 spans concurrently in flight on the mod-a → fn-b edge.
    const spans = [
      { span_id: 'root', parent_span_id: null, operation_name: 'test_fanout', start_time: 0, duration_us: 10000, status: 'ok', graph_node_id: 'mod-a' },
    ];
    for (let i = 0; i < 150; i++) {
      spans.push({
        span_id: `fan-${i}`, parent_span_id: 'root', operation_name: `op_${i}`,
        start_time: 0, duration_us: 5000, status: i % 10 === 0 ? 'error' : 'ok',
        graph_node_id: 'fn-b',
      });
    }
    const trace = { spans, root_spans: ['root'] };
    const { container } = render(FlowCanvas, {
      props: { nodes: [MOD, FN], edges: EDGES_B, lens: 'evaluative', traceData: trace },
    });
    await flushFrames();
    expect(activeSpansText(container)).toBe('150 active spans');
    const overlayCanvas = container.querySelector('.flow-particles canvas');
    expect(overlayCanvas.getAttribute('data-particle-mode')).toBe('webgl');
    expect(glMock.createShader).toHaveBeenCalled();
    expect(glMock.drawArrays).toHaveBeenCalled();
    // Back below the threshold → Canvas 2D again. (Fan spans end at
    // 5000µs; t=9900µs is past every span's end.)
    await setScrubber(container, 990);
    expect(activeSpansText(container)).toBe('0 active spans');
    // (0 drawables render through the 2D path on a fresh 2D canvas.)
    const canvasNow = container.querySelector('.flow-particles canvas');
    expect(canvasNow.getAttribute('data-particle-mode')).toBe('2d');
  });
});
