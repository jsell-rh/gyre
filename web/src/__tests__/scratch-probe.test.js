import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render } from '@testing-library/svelte';
import ExplorerCanvas from '../lib/ExplorerCanvas.svelte';
import { tick } from 'svelte';

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
  };
  HTMLCanvasElement.prototype.getContext = vi.fn(() => mockCtx);
  global.ResizeObserver = class { observe() {} disconnect() {} unobserve() {} };
  global.requestAnimationFrame = vi.fn(cb => { cb(); return 1; });
  global.cancelAnimationFrame = vi.fn();
});
afterEach(() => { vi.restoreAllMocks(); });

// Fixture: all nodes share qualified_name 'mod' → single top-level synthetic
// tree-group containing all four as leaf graph nodes (no contains edges).
const NODES = [
  { id: 'part1', node_type: 'function', name: 'part1', qualified_name: 'mod', file_path: '', line_start: 0, line_end: 0, visibility: 'public', spec_confidence: 'none', test_node: false },
  { id: 'part2', node_type: 'function', name: 'part2', qualified_name: 'mod', file_path: '', line_start: 0, line_end: 0, visibility: 'public', spec_confidence: 'none', test_node: false },
  { id: 'lone1', node_type: 'function', name: 'lone1', qualified_name: 'mod', file_path: '', line_start: 0, line_end: 0, visibility: 'public', spec_confidence: 'none', test_node: false },
  { id: 'lone2', node_type: 'function', name: 'lone2', qualified_name: 'mod', file_path: '', line_start: 0, line_end: 0, visibility: 'public', spec_confidence: 'none', test_node: false },
];
const EDGES = [
  { id: 'e1', source_id: 'part1', target_id: 'part2', edge_type: 'calls' },
];

describe('probe', () => {
  it('dumps fillText calls with alpha', async () => {
    const draws = [];
    mockCtx.fillText = vi.fn((text) => draws.push({ text: String(text), alpha: mockCtx.globalAlpha }));
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, filter: 'dependencies' },
    });
    await tick();
    const zoomInd = container.querySelector('.zoom-ind');
    console.log('ZOOM IND:', zoomInd?.textContent);
    const summary = {};
    for (const d of draws) {
      const key = `${d.text}|${d.alpha.toFixed(3)}`;
      summary[key] = (summary[key] ?? 0) + 1;
    }
    console.log('DRAW SUMMARY:', JSON.stringify(summary, null, 1));
    expect(true).toBe(true);
  });
});
