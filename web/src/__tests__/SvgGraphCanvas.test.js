/**
 * SvgGraphCanvas tests (ui-layout.md §10 Canvas Rendering + Layout Engines).
 *
 * Pins: SVG rendering with individually addressable nodes/edges, pan/zoom
 * controls, ViewEvent emission (click/dblclick/hover/context-menu) from
 * nodes AND edges, and the density policy cascade (public-only banner with
 * [Show All], list fallback with virtual scrolling above the thresholds).
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { onViewEvent } from '../lib/viewEvents.js';

import SvgGraphCanvas from '../lib/SvgGraphCanvas.svelte';

const NODES = [
  { id: 'a', name: 'AuthService', node_type: 'function', visibility: 'public' },
  { id: 'b', name: 'UserModel', node_type: 'type', visibility: 'public' },
  { id: 'c', name: 'Routes', node_type: 'endpoint', visibility: 'private' },
];
const EDGES = [
  { id: 'e1', source_id: 'a', target_id: 'b', edge_type: 'calls' },
  { id: 'e2', source_id: 'c', target_id: 'a', edge_type: 'routes_to' },
];

function manyNodes(count, visibility = 'public') {
  return Array.from({ length: count }, (_, i) => ({
    id: `n${i}`,
    name: `node-${i}`,
    node_type: 'function',
    visibility,
  }));
}

beforeEach(() => {
  global.ResizeObserver = class ResizeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
});

describe('SvgGraphCanvas rendering', () => {
  it('renders an SVG with one addressable group per node and line per edge', async () => {
    const { container } = render(SvgGraphCanvas, {
      props: { nodes: NODES, edges: EDGES, engine: 'graph' },
    });
    await waitFor(() => {
      expect(container.querySelectorAll('g.svg-node')).toHaveLength(3);
    });
    expect(container.querySelectorAll('line.svg-edge')).toHaveLength(2);
    expect(container.querySelector('svg.graph-svg')).toBeTruthy();
  });

  it('positions nodes via the d3-force engine (graph)', async () => {
    const { container } = render(SvgGraphCanvas, {
      props: { nodes: NODES, edges: EDGES, engine: 'graph' },
    });
    // Layout resolves asynchronously (dynamic d3-force import + tick run)
    await waitFor(() => {
      const transforms = [...container.querySelectorAll('g.svg-node')]
        .map((g) => g.style.transform);
      // At least one node has a real (non-pending) position: pending nodes
      // stack at translate(0px, 0px)-ish (40 - NODE_W/2 = -26); force output
      // is centered around the 900x600 canvas middle.
      expect(transforms.some((t) => t && !t.includes('-26px'))).toBe(true);
    });
  });

  it('positions nodes via the ELK engine (hierarchical)', async () => {
    const { container } = render(SvgGraphCanvas, {
      props: { nodes: NODES, edges: EDGES, engine: 'hierarchical' },
    });
    await waitFor(() => {
      expect(container.querySelectorAll('g.svg-node')).toHaveLength(3);
    });
  });
});

describe('SvgGraphCanvas interactions', () => {
  it('emits click / dblclick / hover / context-menu ViewEvents from nodes and calls onSelectNode', async () => {
    const handler = vi.fn();
    const unsub = onViewEvent(handler);
    const onSelectNode = vi.fn();
    const { container } = render(SvgGraphCanvas, {
      props: { nodes: NODES, edges: EDGES, engine: 'graph', onSelectNode },
    });
    await waitFor(() => {
      expect(container.querySelectorAll('g.svg-node')).toHaveLength(3);
    });

    const nodeEl = container.querySelector('g.svg-node');
    await fireEvent.click(nodeEl);
    expect(handler.mock.calls.at(-1)?.[0]).toMatchObject({
      type: 'click',
      entity_type: 'node',
      entity_id: 'a',
    });
    expect(onSelectNode).toHaveBeenCalledWith(expect.objectContaining({ id: 'a' }));

    await fireEvent.dblClick(nodeEl);
    expect(handler.mock.calls.at(-1)?.[0].type).toBe('dblclick');

    await fireEvent.mouseEnter(nodeEl);
    expect(handler.mock.calls.at(-1)?.[0].type).toBe('hover');

    await fireEvent.contextMenu(nodeEl);
    expect(handler.mock.calls.at(-1)?.[0].type).toBe('context-menu');
    expect(handler.mock.calls.at(-1)?.[0].entity_id).toBe('a');
    unsub();
  });

  it('emits ViewEvents from edges', async () => {
    const handler = vi.fn();
    const unsub = onViewEvent(handler);
    const { container } = render(SvgGraphCanvas, {
      props: { nodes: NODES, edges: EDGES, engine: 'graph' },
    });
    await waitFor(() => {
      expect(container.querySelectorAll('line.svg-edge')).toHaveLength(2);
    });
    const edgeEl = container.querySelector('line.svg-edge');
    await fireEvent.click(edgeEl);
    expect(handler.mock.calls.at(-1)?.[0]).toMatchObject({
      type: 'click',
      entity_type: 'edge',
      entity_id: 'e1',
    });
    await fireEvent.mouseEnter(edgeEl);
    expect(handler.mock.calls.at(-1)?.[0]).toMatchObject({
      type: 'hover',
      entity_type: 'edge',
    });
    unsub();
  });

  it('zoom controls change the camera scale (pan/zoom present)', async () => {
    const { container, getByLabelText } = render(SvgGraphCanvas, {
      props: { nodes: NODES, edges: EDGES, engine: 'graph' },
    });
    await waitFor(() => {
      expect(container.querySelectorAll('g.svg-node')).toHaveLength(3);
    });
    const label = container.querySelector('.svg-zoom-label');
    expect(label.textContent).toBe('100%');
    await fireEvent.click(getByLabelText('Zoom in'));
    expect(container.querySelector('.svg-zoom-label').textContent).toBe('130%');
    await fireEvent.click(getByLabelText('Zoom out'));
    expect(container.querySelector('.svg-zoom-label').textContent).toBe('100%');
    // Fit recomputes a scale from the layout bounds
    await fireEvent.click(getByLabelText('Fit view'));
    expect(container.querySelector('.viewport')).toBeTruthy();
  });
});

describe('SvgGraphCanvas density policy (ui-layout.md §10)', () => {
  it('shows the public-only banner above 500 nodes and honors [Show All]', async () => {
    const nodes = [...manyNodes(450), ...manyNodes(60, 'private')];
    const { container } = render(SvgGraphCanvas, {
      props: { nodes, edges: [], engine: 'graph' },
    });
    await waitFor(() => {
      expect(container.querySelector('.density-banner')).toBeTruthy();
    });
    expect(container.textContent).toContain('Showing public API only');
    expect(container.textContent).toContain('60 private nodes hidden');
    // Only public nodes render
    await waitFor(() => {
      expect(container.querySelectorAll('g.svg-node')).toHaveLength(450);
    });
    // [Show All] disables the filter
    await fireEvent.click(container.querySelector('.density-show-all'));
    await waitFor(() => {
      expect(container.querySelectorAll('g.svg-node')).toHaveLength(510);
    });
    expect(container.querySelector('.density-banner')).toBeNull();
  });

  it('switches to the virtualized list above 1000 nodes even after filtering', async () => {
    const nodes = manyNodes(1200);
    const { container } = render(SvgGraphCanvas, {
      props: { nodes, edges: [], engine: 'graph' },
    });
    await waitFor(() => {
      expect(container.querySelector('.density-banner.density-warn')).toBeTruthy();
    });
    expect(container.textContent).toContain('switched to list layout with virtual scrolling');
    expect(container.querySelector('.vlist')).toBeTruthy();
    // Virtualized: a fraction of the 1200 rows is in the DOM
    const rows = container.querySelectorAll('.vlist-row');
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.length).toBeLessThan(1200);
    // No SVG canvas in fallback mode
    expect(container.querySelector('svg.graph-svg')).toBeNull();
  });
});
