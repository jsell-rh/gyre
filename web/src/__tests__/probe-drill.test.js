import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render } from '@testing-library/svelte';
import ExplorerCanvas from '../lib/ExplorerCanvas.svelte';

// Mock canvas context
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
    font: '', textAlign: '', textBaseline: '',
    shadowColor: '', shadowBlur: 0,
    setLineDash: vi.fn(), getLineDash: vi.fn(() => []),
  };
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

// Hierarchy matching spec §5: repo > package > module > type > method/field
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

// Scan the canvas with mousemove events; return map of node name -> screen pos
// discovered via the hover tooltip.
async function scanNodePositions(container) {
  const canvas = container.querySelector('canvas.treemap-canvas');
  const found = new Map();
  for (let x = 20; x < 900; x += 15) {
    for (let y = 20; y < 600; y += 15) {
      canvas.dispatchEvent(new MouseEvent('mousemove', { clientX: x, clientY: y, bubbles: true }));
      await new Promise(r => setTimeout(r, 0));
      const tip = container.querySelector('.tooltip-name');
      if (tip) {
        const name = tip.textContent;
        if (!found.has(name)) found.set(name, { x, y });
      }
    }
  }
  return found;
}

function dblclick(canvas, x, y) {
  canvas.dispatchEvent(new MouseEvent('dblclick', { clientX: x, clientY: y, bubbles: true }));
}

function sleep(ms) { return new Promise(r => setTimeout(r, ms)); }

describe('PROBE: §5 progressive drill-down', () => {
  it('discovers node positions via hover', async () => {
    const { container } = render(ExplorerCanvas, { props: { nodes: NODES, edges: EDGES } });
    await sleep(100);
    const positions = await scanNodePositions(container);
    console.log('FOUND NODES:', JSON.stringify([...positions.keys()]));
    expect(positions.size).toBeGreaterThan(0);
  });

  it('double-click container drills: breadcrumb + filtered layout', async () => {
    const onNodeDetail = vi.fn();
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, onNodeDetail, rootLabel: 'e2e-repo' },
    });
    await sleep(100);
    const canvas = container.querySelector('canvas.treemap-canvas');
    const positions = await scanNodePositions(container);
    console.log('POSITIONS:', JSON.stringify([...positions.entries()]));
    const pkg = positions.get('greeting-service');
    if (!pkg) throw new Error('greeting-service not found on canvas');
    dblclick(canvas, pkg.x, pkg.y);
    await sleep(500);
    const bc = container.querySelector('.treemap-breadcrumb');
    console.log('BREADCRUMB HTML:', bc?.textContent);
    const crumbs = container.querySelectorAll('.breadcrumb-item');
    console.log('CRUMBS:', Array.from(crumbs).map(c => c.textContent));
    const after = await scanNodePositions(container);
    console.log('AFTER DRILL NODES:', JSON.stringify([...after.keys()]));
    // breadcrumb segment navigation: click the root segment
    const rootBtn = container.querySelector('.breadcrumb-item.root');
    if (rootBtn) {
      rootBtn.click();
      await sleep(500);
      const afterUp = await scanNodePositions(container);
      console.log('AFTER ROOT-CLICK NODES:', JSON.stringify([...afterUp.keys()]));
      console.log('CRUMBS AFTER ROOT:', Array.from(container.querySelectorAll('.breadcrumb-item')).map(c => c.textContent));
    }
  });

  it('double-click real container (GreetingService type) drills', async () => {
    const onNodeDetail = vi.fn();
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, onNodeDetail },
    });
    await sleep(100);
    const canvas = container.querySelector('canvas.treemap-canvas');
    const positions = await scanNodePositions(container);
    const t = positions.get('GreetingService');
    if (!t) throw new Error('GreetingService not found');
    dblclick(canvas, t.x, t.y);
    await sleep(400); // let drill transition finish
    const crumbs = container.querySelectorAll('.breadcrumb-item');
    console.log('CRUMBS AFTER TYPE DRILL:', Array.from(crumbs).map(c => c.textContent));
    const after = await scanNodePositions(container);
    console.log('NODES AFTER TYPE DRILL:', JSON.stringify([...after.keys()]));
    // click root breadcrumb
    const rootBtn = container.querySelector('.breadcrumb-item.root');
    if (rootBtn) {
      rootBtn.click();
      await sleep(400);
      const crumbs2 = container.querySelectorAll('.breadcrumb-item');
      console.log('CRUMBS AFTER ROOT CLICK:', Array.from(crumbs2).map(c => c.textContent));
    }
  });

  it('double-click leaf opens detail panel', async () => {
    const onNodeDetail = vi.fn();
    const { container } = render(ExplorerCanvas, {
      props: { nodes: NODES, edges: EDGES, onNodeDetail },
    });
    await sleep(100);
    const canvas = container.querySelector('canvas.treemap-canvas');
    const positions = await scanNodePositions(container);
    // leaves are only visible when zoomed into their parent; drill to GreetingService first
    const t = positions.get('GreetingService');
    if (t) {
      dblclick(canvas, t.x, t.y);
      await sleep(400);
    }
    const positions2 = await scanNodePositions(container);
    console.log('POSITIONS AFTER DRILL:', JSON.stringify([...positions2.keys()]));
    const leaf = positions2.get('greet') ?? positions2.get('config');
    if (!leaf) throw new Error('leaf not found after drill: ' + JSON.stringify([...positions2.keys()]));
    // check what tooltip shows at the leaf position right before dblclick
    canvas.dispatchEvent(new MouseEvent('mousemove', { clientX: leaf.x, clientY: leaf.y, bubbles: true }));
    await sleep(10);
    console.log('TOOLTIP AT LEAF POS:', container.querySelector('.node-tooltip, .treemap-tooltip, [class*="tooltip"]')?.textContent);
    // single click first
    canvas.dispatchEvent(new MouseEvent('mousedown', { clientX: leaf.x, clientY: leaf.y, bubbles: true }));
    canvas.dispatchEvent(new MouseEvent('mouseup', { clientX: leaf.x, clientY: leaf.y, bubbles: true }));
    canvas.dispatchEvent(new MouseEvent('click', { clientX: leaf.x, clientY: leaf.y, bubbles: true }));
    await sleep(20);
    console.log('AFTER SINGLE CLICK:', onNodeDetail.mock.calls.map(c => JSON.stringify(c[0])));
    onNodeDetail.mockClear();
    dblclick(canvas, leaf.x, leaf.y);
    await sleep(50);
    console.log('LEAF DETAIL CALLS:', onNodeDetail.mock.calls.map(c => JSON.stringify(c[0])));
  });
});
