import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render } from '@testing-library/svelte';
import ExplorerCanvas from '../lib/ExplorerCanvas.svelte';

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
  global.requestAnimationFrame = vi.fn(cb => { cb(16); return 1; });
  global.cancelAnimationFrame = vi.fn();
});
afterEach(() => { vi.restoreAllMocks(); });

const NODES = [
  { id: 'pkg1', node_type: 'package', name: 'api', qualified_name: 'api', file_path: '', line_start: 0, line_end: 0, visibility: 'public', spec_confidence: 'none', test_node: false },
  { id: 'fn1', node_type: 'function', name: 'create_user', qualified_name: 'api.create_user', file_path: 'api/handlers.py', line_start: 10, line_end: 30, visibility: 'public', spec_confidence: 'high', test_node: false },
];
const EDGES = [
  { id: 'e1', source_id: 'pkg1', target_id: 'fn1', edge_type: 'contains' },
];
const TRACE = {
  spans: [
    { span_id: 's1', parent_span_id: null, operation_name: 'root', start_time: 0, duration_us: 1000, status: 'ok', graph_node_id: 'fn1' },
  ],
  root_spans: ['s1'],
};

describe('verify fix', () => {
  it('evaluative lens draws nodes after layout', () => {
    render(ExplorerCanvas, { props: { nodes: NODES, edges: EDGES, lens: 'evaluative', traceData: TRACE } });
    const texts = mockCtx.fillText.mock.calls.map(c => String(c[0]));
    console.log('FILLTEXT:', JSON.stringify(texts));
    console.log('beginPath:', mockCtx.beginPath.mock.calls.length, 'fillRect:', mockCtx.fillRect.mock.calls.length);
    expect(texts).toContain('create_user');
  });
});
