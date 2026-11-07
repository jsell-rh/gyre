import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import ExplorerCanvas from '../lib/ExplorerCanvas.svelte';

// explorer-canvas.md §4 Lens Definitions — evaluative lens behaviors.
//
// These tests pin the two task-066 repairs:
// 1. Node badges must render ALL THREE spec metrics (span count, error
//    rate, mean duration) as chips, independent of which metric drives
//    the heat coloring. The pre-fix code drew only the selected metric.
// 2. The render loop must keep redrawing under a synchronous
//    requestAnimationFrame mock (jsdom, embedded webviews with sync rAF
//    polyfills). The pre-fix code tracked pending frames by the rAF
//    handle, which a sync rAF leaves stale, so every later
//    scheduleRedraw() bailed and the canvas froze.

let mockCtx;
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
    filter: '',
  };
  HTMLCanvasElement.prototype.getContext = vi.fn(() => mockCtx);
  global.ResizeObserver = class ResizeObserver { observe() {} disconnect() {} unobserve() {} };
  // Synchronous rAF: the callback runs before requestAnimationFrame returns.
  global.requestAnimationFrame = vi.fn(cb => { cb(16); return 1; });
  global.cancelAnimationFrame = vi.fn();
});
afterEach(() => { vi.restoreAllMocks(); });

// ── Test data ─────────────────────────────────────────────────────────────

const NODES = [
  { id: 'pkg1', node_type: 'package', name: 'api', qualified_name: 'api', file_path: '', line_start: 0, line_end: 0, visibility: 'public', spec_confidence: 'none', test_node: false },
  { id: 'fn1', node_type: 'function', name: 'create_user', qualified_name: 'api.create_user', file_path: 'api/handlers.py', line_start: 10, line_end: 30, visibility: 'public', spec_confidence: 'high', test_node: false },
  { id: 'fn2', node_type: 'function', name: 'get_user', qualified_name: 'api.get_user', file_path: 'api/handlers.py', line_start: 32, line_end: 45, visibility: 'public', spec_confidence: 'medium', test_node: false },
];

const EDGES = [
  { id: 'e1', source_id: 'pkg1', target_id: 'fn1', edge_type: 'contains' },
  { id: 'e2', source_id: 'pkg1', target_id: 'fn2', edge_type: 'contains' },
  { id: 'e3', source_id: 'fn1', target_id: 'fn2', edge_type: 'calls' },
];

// fn1: one span, 1000µs mean, no errors. fn2: two spans (400µs + 600µs →
// 500µs mean), one error (→ 50% error rate).
const TRACE = {
  spans: [
    { span_id: 's1', parent_span_id: null, operation_name: 'root', start_time: 0, duration_us: 1000, status: 'ok', graph_node_id: 'fn1' },
    { span_id: 's2', parent_span_id: 's1', operation_name: 'child', start_time: 100, duration_us: 400, status: 'ok', graph_node_id: 'fn2' },
    { span_id: 's3', parent_span_id: 's1', operation_name: 'child2', start_time: 200, duration_us: 600, status: 'error', graph_node_id: 'fn2' },
  ],
  root_spans: ['s1'],
};

const drawnTexts = () => mockCtx.fillText.mock.calls.map(c => String(c[0]));

// ── Evaluative node badges (§4: span count, error rate, mean duration) ────

describe('ExplorerCanvas — evaluative lens node badges', () => {
  it('renders all three metric chips on every traced node', () => {
    render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData: TRACE },
    });
    const texts = drawnTexts();
    // fn1: 1 span, 0% errors, 1000µs mean → "1.0ms"
    expect(texts).toContain('1 spans');
    expect(texts).toContain('0% err');
    expect(texts).toContain('1.0ms');
    // fn2: 2 spans, 50% errors, 500µs mean
    expect(texts).toContain('2 spans');
    expect(texts).toContain('50% err');
    expect(texts).toContain('500µs');
  });

  it('badge chips are independent of the selected heat metric', async () => {
    // Pre-fix behavior: only the selected metric was badged; switching the
    // heat metric to span count dropped the duration and error-rate chips.
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData: TRACE },
    });
    const metricButtons = container.querySelectorAll('.eval-metric-group .tb-btn');
    const spansBtn = [...metricButtons].find(b => b.textContent.trim() === 'Spans');
    expect(spansBtn).toBeTruthy();
    await fireEvent.click(spansBtn);
    const texts = drawnTexts();
    // Heat now colors by span count, but the badge row still shows all three.
    expect(texts).toContain('1 spans');
    expect(texts).toContain('0% err');
    expect(texts).toContain('1.0ms');
    expect(texts).toContain('50% err');
    expect(texts).toContain('500µs');
  });

  it('untraced nodes get no badge chips', () => {
    render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData: TRACE },
    });
    // pkg1 has no spans touching it — no chip text should reference it.
    // (No "0 spans"/"0% err" chips anywhere: every chip comes from a
    // traced node with spanCount > 0.)
    expect(drawnTexts()).not.toContain('0 spans');
  });
});

// ── Render-loop survival under synchronous requestAnimationFrame ──────────

describe('ExplorerCanvas — render loop under synchronous rAF', () => {
  it('keeps redrawing after the initial frame when props change', async () => {
    const { rerender } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, lens: 'structural' },
    });
    // Initial frame drew the original nodes.
    expect(drawnTexts()).toContain('create_user');
    // A prop change must schedule a fresh draw. Pre-fix, the stale rAF
    // handle made scheduleRedraw() bail and this new node never rendered.
    await rerender({
      props: {
        nodes: [...NODES, {
          id: 'fn3', node_type: 'function', name: 'delete_user',
          qualified_name: 'api.delete_user', file_path: 'api/handlers.py',
          line_start: 50, line_end: 70, visibility: 'public',
          spec_confidence: 'none', test_node: false,
        }],
        edges: [...EDGES, { id: 'e4', source_id: 'pkg1', target_id: 'fn3', edge_type: 'contains' }],
        lens: 'structural',
      },
    });
    expect(drawnTexts()).toContain('delete_user');
  });
});
