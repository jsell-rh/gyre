import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
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
  global.requestAnimationFrame = vi.fn(cb => { cb(); return 1; });
  global.cancelAnimationFrame = vi.fn();
});
afterEach(() => { vi.restoreAllMocks(); });

const WS_NODE = { id: 'node-a', repo_id: 'repo-123', node_type: 'workspace', name: 'ws', qualified_name: 'ws', file_path: '', line_start: 0, line_end: 0, visibility: 'public', spec_confidence: 'none', test_node: false };

describe('repro', () => {
  it('mount + dblclick', async () => {
    const { container } = render(ExplorerCanvas, { props: { nodes: [WS_NODE], edges: [] } });
    await new Promise(r => setTimeout(r, 50));
    const canvas = container.querySelector('canvas.treemap-canvas');
    canvas.dispatchEvent(new MouseEvent('mousedown', { clientX: 450, clientY: 300, bubbles: true }));
    canvas.dispatchEvent(new MouseEvent('mouseup', { clientX: 450, clientY: 300, bubbles: true }));
    canvas.dispatchEvent(new MouseEvent('dblclick', { clientX: 450, clientY: 300, bubbles: true }));
    await new Promise(r => setTimeout(r, 20));
    expect(true).toBe(true);
  });
});
