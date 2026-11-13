import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import ExplorerCanvas from '../lib/ExplorerCanvas.svelte';

// task-015 Flow Canvas — component-level behaviors for the flow-layout
// particle system (ui-layout.md §4 Flow Layout, human-system-interface.md
// §3a): playback controls (Step, 10x, Test selector), freeze-on-scrub,
// test-case filtering + legend, click-node-during-animation pause, and the
// fading particle trail. Pure span→particle logic is covered by
// flow-particles.test.js; these tests pin the wiring and the UI contract.

let mockCtx;
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
  rafQueue = [];
  HTMLCanvasElement.prototype.getContext = vi.fn(() => mockCtx);
  global.ResizeObserver = class ResizeObserver { observe() {} disconnect() {} unobserve() {} };
  // Queued rAF (real-browser semantics): callbacks run only when flushed,
  // with caller-controlled timestamps so playback advancement is
  // deterministic in the trail test.
  global.requestAnimationFrame = vi.fn((cb) => { rafQueue.push(cb); return rafQueue.length; });
  global.cancelAnimationFrame = vi.fn();
});
afterEach(() => { vi.restoreAllMocks(); });

// Run queued animation frames, optionally with explicit timestamps.
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

// Three-node call graph under one package:
//   fn1 ──calls──▶ fn2
//   fn1 ──calls──▶ fn3
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
    { span_id: 's2', parent_span_id: 's1', operation_name: 'create_user', start_time: 1000, duration_us: 4000, status: 'ok', graph_node_id: 'fn2' },
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

// ── Control bar: Step, 10x speed, Test selector, legend ──────────────────

describe('ExplorerCanvas flow — playback control bar', () => {
  it('renders the Step button, a 10x speed option, and the test selector with root span names', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData: TRACE },
    });
    // ⏭ Step (HSI §3a step-through)
    const stepBtn = container.querySelector('.trace-pb-step');
    expect(stepBtn).toBeTruthy();
    expect(stepBtn.title).toContain('Step');
    // 10x speed (HSI §3a: 1x/10x/step-through)
    const speed = container.querySelector('.trace-pb-speed');
    expect(speed).toBeTruthy();
    const opt10 = [...speed.querySelectorAll('option')].find((o) => o.value === '10');
    expect(opt10?.textContent).toBe('10x');
    // Test selector (ui-layout.md §4: dropdown of root span names)
    const testSel = container.querySelector('.trace-pb-test');
    expect(testSel).toBeTruthy();
    const opts = [...testSel.querySelectorAll('option')].map((o) => o.textContent);
    expect(opts).toContain('Test: all');
    expect(opts).toContain('test_create');
    expect(opts).toContain('test_read');
  });

  it('renders the test-case legend with a distinct hue per case', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData: TRACE },
    });
    const items = container.querySelectorAll('.trace-pb-legend-item');
    expect(items.length).toBe(2);
    expect(items[0].textContent).toContain('test_create');
    expect(items[1].textContent).toContain('test_read');
    const dots = [...container.querySelectorAll('.trace-pb-legend-dot')];
    expect(dots[0].getAttribute('style')).not.toBe(dots[1].getAttribute('style'));
  });

  it('hides the test selector and legend for a single test case', () => {
    const single = { spans: TRACE.spans.filter((s) => ['s1', 's2', 's3'].includes(s.span_id)), root_spans: ['s1'] };
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData: single },
    });
    expect(container.querySelector('.trace-pb-test')).toBeFalsy();
    expect(container.querySelector('.trace-pb-legend')).toBeFalsy();
  });
});

// ── Freeze on scrub (ui-layout.md §4 Scrub bar) ──────────────────────────

describe('ExplorerCanvas flow — time scrubber', () => {
  it('dragging the scrubber while paused repositions particles to that timestamp', async () => {
    // Pre-fix defect: particles were only recomputed inside tickParticles,
    // which returns early when paused — dragging the scrubber left the
    // particles at their last-played positions (freeze-on-scrub broken).
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData: TRACE },
    });
    await flushFrames();
    expect(activeSpansText(container)).toBe('0 active spans');

    // t = 3000µs: s2 (progress 0.5) and s3 (progress 1/3) are active.
    await setScrubber(container, 300);
    expect(activeSpansText(container)).toBe('2 active spans');

    // t = 7000µs: only s5 (second test case) is active.
    await setScrubber(container, 700);
    expect(activeSpansText(container)).toBe('1 active spans');

    // t = 9500µs: everything finished.
    await setScrubber(container, 950);
    expect(activeSpansText(container)).toBe('0 active spans');
  });
});

// ── Test-case filter (ui-layout.md §4 Test selector + legend) ────────────

describe('ExplorerCanvas flow — test-case filter', () => {
  it('the test selector filters which root spans animate', async () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData: TRACE },
    });
    await flushFrames();
    await setScrubber(container, 700); // t=7000: only s5 (case test_read)
    expect(activeSpansText(container)).toBe('1 active spans');

    const testSel = container.querySelector('.trace-pb-test');
    await fireEvent.change(testSel, { target: { value: 's1' } }); // test_create
    await new Promise((r) => setTimeout(r, 0));
    expect(activeSpansText(container)).toBe('0 active spans');

    await fireEvent.change(testSel, { target: { value: 's4' } }); // test_read
    await new Promise((r) => setTimeout(r, 0));
    expect(activeSpansText(container)).toBe('1 active spans');

    await fireEvent.change(testSel, { target: { value: 'all' } });
    await new Promise((r) => setTimeout(r, 0));
    expect(activeSpansText(container)).toBe('1 active spans');
  });

  it('legend items toggle the same filter', async () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData: TRACE },
    });
    await flushFrames();
    await setScrubber(container, 300); // t=3000: s2 + s3, both case test_create
    expect(activeSpansText(container)).toBe('2 active spans');

    const items = container.querySelectorAll('.trace-pb-legend-item');
    await fireEvent.click(items[1]); // test_read → no particles at t=3000
    await new Promise((r) => setTimeout(r, 0));
    expect(activeSpansText(container)).toBe('0 active spans');
    let active = container.querySelector('.trace-pb-legend-active');
    expect(active?.textContent).toContain('test_read');

    await fireEvent.click(container.querySelector('.trace-pb-legend-active')); // toggle off → all cases
    await new Promise((r) => setTimeout(r, 0));
    expect(activeSpansText(container)).toBe('2 active spans');
    expect(container.querySelector('.trace-pb-legend-active')).toBeFalsy();
  });
});

// ── Step-through (HSI §3a: span by span) ─────────────────────────────────

describe('ExplorerCanvas flow — step-through', () => {
  it('Step advances to the next span start and pauses playback', async () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData: TRACE },
    });
    await flushFrames();

    // Start playback, then step: stepping must pause (play glyph restored).
    const playBtn = container.querySelector('.trace-pb-btn');
    await fireEvent.click(playBtn);
    await new Promise((r) => setTimeout(r, 0));
    expect(playBtn.querySelectorAll('svg rect').length).toBe(2); // pause glyph = playing

    await fireEvent.click(container.querySelector('.trace-pb-step'));
    await new Promise((r) => setTimeout(r, 0));
    expect(playBtn.querySelectorAll('svg rect').length).toBe(0); // play glyph = paused
    // First span start after t≈0 is s2 at 1000µs → exactly one active span.
    expect(activeSpansText(container)).toBe('1 active spans');

    // Next step → s3 at 2000µs: s2 (progress 0.25) + s3 (progress 0).
    await fireEvent.click(container.querySelector('.trace-pb-step'));
    await new Promise((r) => setTimeout(r, 0));
    expect(activeSpansText(container)).toBe('2 active spans');

    // Next step → s5 at 6000µs (s2/s3 finished).
    await fireEvent.click(container.querySelector('.trace-pb-step'));
    await new Promise((r) => setTimeout(r, 0));
    expect(activeSpansText(container)).toBe('1 active spans');

    // Past the last span start → pinned at the end, nothing active.
    await fireEvent.click(container.querySelector('.trace-pb-step'));
    await new Promise((r) => setTimeout(r, 0));
    expect(activeSpansText(container)).toBe('0 active spans');
  });

  it('Step respects the active test-case filter', async () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData: TRACE },
    });
    await flushFrames();
    await fireEvent.change(container.querySelector('.trace-pb-test'), { target: { value: 's4' } });
    await new Promise((r) => setTimeout(r, 0));

    // Case test_read spans start at 0 (root) and 6000 (s5): the first step
    // from t=0 lands on 6000µs, skipping the other case's spans entirely.
    await fireEvent.click(container.querySelector('.trace-pb-step'));
    await new Promise((r) => setTimeout(r, 0));
    expect(activeSpansText(container)).toBe('1 active spans');
  });
});

// ── Click node during animation (ui-layout.md §4) ────────────────────────

describe('ExplorerCanvas flow — click node during animation', () => {
  // Module + callee, both hit-testable around the canvas center in jsdom
  // (world (0,0) → screen (450,300); the scope-drill fixture pattern).
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

  it('pauses playback and opens the detail panel with the nearest span data', async () => {
    const onNodeDetail = vi.fn();
    const { container } = render(ExplorerCanvas, {
      props: { nodes: [MOD, FN], edges: EDGES_B, lens: 'evaluative', traceData: TRACE_B, onNodeDetail },
    });
    await flushFrames([16]);

    // Start playback — a frame advances the scrubber past 0 so the
    // fn-b particle is in flight.
    const playBtn = container.querySelector('.trace-pb-btn');
    await fireEvent.click(playBtn);
    await flushFrames([66]);
    expect(activeSpansText(container)).toBe('1 active spans');

    // Click a node (canvas center).
    const canvas = container.querySelector('canvas.treemap-canvas');
    canvas.dispatchEvent(new MouseEvent('mousedown', { clientX: 450, clientY: 300, bubbles: true }));
    canvas.dispatchEvent(new MouseEvent('click', { clientX: 450, clientY: 300, bubbles: true }));
    await new Promise((r) => setTimeout(r, 0));

    // Playback paused: the toolbar button flipped back to the play glyph.
    expect(playBtn.querySelectorAll('svg rect').length).toBe(0);
    // The detail panel received the span data for the particle at/nearest
    // the clicked node (ui-layout.md §4 click-node payload).
    const withSpan = onNodeDetail.mock.calls.map((c) => c[0]).find((n) => n?._activeSpan);
    expect(withSpan).toBeTruthy();
    expect(withSpan._activeSpan.span_id).toBe('s9');
    expect(withSpan._activeSpan.operation_name).toBe('handle');
  });
});

// ── Particle trail (ui-layout.md §4: 200ms fade, 20px length) ────────────

describe('ExplorerCanvas flow — particle trail rendering', () => {
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
    // Capture stroke parameters at call time (the ctx is mutated between
    // calls, so the mock must snapshot on invocation). The mock's
    // save/restore are no-ops, so lineCap set by the trail leaks to later
    // strokes — discriminate by the particle color instead.
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

    const { container } = render(ExplorerCanvas, {
      props: { nodes: [MOD, FN], edges: EDGES_B, lens: 'evaluative', traceData: TRACE_B },
    });
    await flushFrames([16]);
    await fireEvent.click(container.querySelector('.trace-pb-btn'));
    // Advance playback with controlled timestamps (50ms apart → the
    // particle moves along the edge each frame, leaving a trail).
    await flushFrames([66, 116, 166, 216, 266]);

    // The particle body rendered in the spec success color (#3b82f6).
    expect(particleFills.length).toBeGreaterThan(0);
    // A fading trail was stroked in the particle color, dimmer than the
    // particle body (alpha scales down along the trail).
    expect(trailStrokes.length).toBeGreaterThan(0);
    for (const t of trailStrokes) {
      expect(t.cap).toBe('round');
      expect(t.alpha).toBeLessThanOrEqual(0.6);
      expect(t.alpha).toBeGreaterThan(0);
    }
  });
});

// ── View-query scoping of the flow layout (task-184, system-explorer.md
//    §5 predefined flows): the active ViewQuery's concept scope filters the
//    node set FlowCanvas renders — the same "scope filters the node set"
//    contract the registry renderers implement.
describe('ExplorerCanvas flow — view query scopes the flow layout', () => {
  it('renders only the concept-scoped nodes in flow layout, with edges into unmatched nodes dropped', async () => {
    const { container } = render(ExplorerCanvas, {
      props: {
        nodes: NODES,
        edges: EDGES,
        layout: 'flow',
        activeQuery: {
          scope: { type: 'concept', seed_nodes: ['fn1', 'fn2'], expand_edges: ['calls'], expand_depth: 0 },
        },
      },
    });
    const nodeIds = [...container.querySelectorAll('g[data-flow-node]')].map(
      (g) => g.getAttribute('data-flow-node'));
    // fn3 and the package node are outside the scoped path.
    expect(nodeIds.sort()).toEqual(['fn1', 'fn2']);
    // The fn1→fn3 edge is dropped with its unmatched endpoint; fn1→fn2
    // (both endpoints scoped) still renders.
    const edgeTargets = [...container.querySelectorAll('path[data-flow-edge]')]
      .map((p) => `${p.dataset.src}->${p.dataset.tgt}`);
    expect(edgeTargets).toEqual(['fn1->fn2']);
  });

  it('shows the full graph when no query is active (unscoped flow view)', () => {
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, layout: 'flow' },
    });
    expect(container.querySelectorAll('g[data-flow-node]').length).toBe(4);
  });
});
