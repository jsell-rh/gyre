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

function manyNodes(count, visibility = 'public', prefix = 'n') {
  return Array.from({ length: count }, (_, i) => ({
    id: `${prefix}${i}`,
    name: `node-${prefix}${i}`,
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

  it('positions nodes via the d3-force engine (graph)', { timeout: 60000 }, async () => {
    // Fixture chosen to discriminate d3-force from the columnLayout
    // fallback: four nodes of the SAME type with two disjoint edges
    // (a→b, c→d). columnLayout stacks same-type nodes in a single column
    // (identical x for all four), and pending nodes stack in the x = -26px
    // pending column — d3-force's link/charge forces spread all four onto
    // distinct x positions (probe on this exact fixture: force → 4 distinct
    // x, connected pair ≈ 89px apart vs ≈ 851px across pairs; column → all
    // x identical).
    const FORCE_NODES = [
      { id: 'a', name: 'A', node_type: 'function', visibility: 'public' },
      { id: 'b', name: 'B', node_type: 'function', visibility: 'public' },
      { id: 'c', name: 'C', node_type: 'function', visibility: 'public' },
      { id: 'd', name: 'D', node_type: 'function', visibility: 'public' },
    ];
    const FORCE_EDGES = [
      { id: 'e1', source_id: 'a', target_id: 'b', edge_type: 'calls' },
      { id: 'e2', source_id: 'c', target_id: 'd', edge_type: 'calls' },
    ];
    const { container } = render(SvgGraphCanvas, {
      props: { nodes: FORCE_NODES, edges: FORCE_EDGES, engine: 'graph' },
    });
    // Layout resolves asynchronously (dynamic d3-force import + tick run)
    await waitFor(() => {
      const transforms = [...container.querySelectorAll('g.svg-node')]
        .map((g) => g.style.transform);
      expect(transforms).toHaveLength(4);
      // No node left in the pending column (x translate -26px)
      expect(transforms.every((t) => t && !t.includes('-26px'))).toBe(true);
      // d3-force output: every node on a distinct x position — the column
      // fallback would put all four on one shared x (same node_type →
      // single column), so this fails if the engine is swapped out.
      const xs = transforms.map(
        (t) => parseFloat(/translate\((-?[\d.]+)px/.exec(t)[1])
      );
      expect(new Set(xs).size).toBe(4);
    }, { timeout: 15000 });
  });

  it('positions nodes via the ELK engine (hierarchical)', { timeout: 60000 }, async () => {
    const { container } = render(SvgGraphCanvas, {
      props: { nodes: NODES, edges: EDGES, engine: 'hierarchical' },
    });
    // Real ELK layered layout, not the column fallback: the edge chain
    // c → a → b must yield three distinct y layers. The column fallback
    // puts all three (distinct node_types → distinct columns) at the same
    // y, and pending (layout unresolved) nodes stack in the x = -26px
    // pending column — both must be excluded.
    await waitFor(() => {
      const transforms = [...container.querySelectorAll('g.svg-node')]
        .map((g) => g.style.transform);
      expect(transforms).toHaveLength(3);
      expect(transforms.every((t) => t && !t.includes('-26px'))).toBe(true);
      const ys = transforms.map(
        (t) => parseFloat(/translate\([^,]+,\s*(-?[\d.]+)px\)/.exec(t)[1])
      );
      expect(new Set(ys).size).toBe(3);
    }, { timeout: 15000 });
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
  it('shows the public-only banner above 500 nodes and honors [Show All]', { timeout: 60000 }, async () => {
    const nodes = [...manyNodes(450), ...manyNodes(60, 'private', 'p')];
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

  it('switches to the virtualized list above 1000 nodes even after filtering', { timeout: 60000 }, async () => {
    const nodes = manyNodes(1200);
    const { container } = render(SvgGraphCanvas, {
      props: { nodes, edges: [], engine: 'graph' },
    });
    await waitFor(() => {
      expect(container.querySelector('.density-banner.density-warn')).toBeTruthy();
    });
    // The banner wraps across template lines — normalize whitespace before matching.
    expect(container.textContent.replace(/\s+/g, ' ')).toContain(
      'switched to list layout with virtual scrolling'
    );
    expect(container.querySelector('.vlist')).toBeTruthy();
    // Virtualized: a fraction of the 1200 rows is in the DOM
    const rows = container.querySelectorAll('.vlist-row');
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.length).toBeLessThan(1200);
    // No SVG canvas in fallback mode
    expect(container.querySelector('svg.graph-svg')).toBeNull();
  });
});
