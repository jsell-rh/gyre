/**
 * VirtualNodeList tests (ui-layout.md §10: list layout with virtual scroll).
 *
 * Pins the windowing behavior: only the rows intersecting the scroll
 * viewport (plus overscan) exist in the DOM, scrolling advances the
 * window, and rows emit standardized ViewEvents.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import { onViewEvent } from '../lib/viewEvents.js';

import VirtualNodeList from '../lib/VirtualNodeList.svelte';

const ROW_HEIGHT = 34;
const NODES = Array.from({ length: 5000 }, (_, i) => ({
  id: `n${i}`,
  name: `node-${i}`,
  node_type: i % 2 ? 'function' : 'type',
  file_path: `src/f${i}.rs`,
}));

function renderedRows(container) {
  return container.querySelectorAll('.vlist-row');
}

beforeEach(() => {
  global.ResizeObserver = class ResizeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
});

describe('VirtualNodeList', () => {
  it('renders only a window of rows for a large list (not one row per node)', () => {
    const { container } = render(VirtualNodeList, {
      props: { nodes: NODES, viewportHeight: 480, rowHeight: ROW_HEIGHT },
    });
    const rows = renderedRows(container);
    // 480px viewport / 34px rows ≈ 15 visible + 2*6 overscan ≈ 27 — nowhere
    // near 5000. If virtualization breaks this jumps to 5000.
    expect(rows.length).toBeGreaterThan(10);
    expect(rows.length).toBeLessThan(60);
  });

  it('keeps spacer height equal to the full list height', () => {
    const { container } = render(VirtualNodeList, {
      props: { nodes: NODES, viewportHeight: 480, rowHeight: ROW_HEIGHT },
    });
    const spacer = container.querySelector('.vlist-spacer');
    expect(spacer.style.height).toBe(`${5000 * ROW_HEIGHT}px`);
  });

  it('starts at the top of the list', () => {
    const { container, getByText } = render(VirtualNodeList, {
      props: { nodes: NODES, viewportHeight: 480, rowHeight: ROW_HEIGHT },
    });
    expect(getByText('node-0')).toBeTruthy();
    expect(renderedRows(container).length).toBeLessThan(5000);
  });

  it('advances the window on scroll (middle rows appear, top rows unmount)', async () => {
    const { container } = render(VirtualNodeList, {
      props: { nodes: NODES, viewportHeight: 480, rowHeight: ROW_HEIGHT },
    });
    const scroller = container.querySelector('.vlist');
    // Scroll to row 2500
    scroller.scrollTop = 2500 * ROW_HEIGHT;
    await fireEvent.scroll(scroller);

    const rows = renderedRows(container);
    expect(rows.length).toBeGreaterThan(10);
    expect(rows.length).toBeLessThan(60);
    // The window now covers row 2500 (within overscan)
    const first = Number(rows[0].textContent.match(/node-(\d+)/)[1]);
    const last = Number(rows[rows.length - 1].textContent.match(/node-(\d+)/)[1]);
    expect(first).toBeLessThanOrEqual(2500);
    expect(first).toBeGreaterThan(2400);
    expect(last).toBeGreaterThanOrEqual(2500);
    // Top-of-list rows unmounted
    expect(container.textContent).not.toContain('node-0\n');
    expect(rows[0].textContent).not.toContain('node-0');
  });

  it('clamps the window at the end of the list', async () => {
    const { container } = render(VirtualNodeList, {
      props: { nodes: NODES, viewportHeight: 480, rowHeight: ROW_HEIGHT },
    });
    const scroller = container.querySelector('.vlist');
    scroller.scrollTop = 5000 * ROW_HEIGHT; // past the end
    await fireEvent.scroll(scroller);
    const rows = renderedRows(container);
    const last = Number(rows[rows.length - 1].textContent.match(/node-(\d+)/)[1]);
    expect(last).toBe(4999);
    // No out-of-range rows
    expect(rows.length).toBeLessThan(60);
  });

  it('emits a ViewEvent and calls onSelectNode on row click', async () => {
    const handler = vi.fn();
    const unsub = onViewEvent(handler);
    const onSelectNode = vi.fn();
    const { container } = render(VirtualNodeList, {
      props: { nodes: NODES, onSelectNode, viewportHeight: 480, rowHeight: ROW_HEIGHT },
    });
    const row = renderedRows(container)[0];
    await fireEvent.click(row);

    expect(onSelectNode).toHaveBeenCalledTimes(1);
    const ev = handler.mock.calls.at(-1)?.[0];
    expect(ev).toMatchObject({ type: 'click', entity_type: 'node', entity_id: 'n0' });
    expect(ev.position).toEqual({ x: 0, y: 0 }); // jsdom click has no coords
    expect(ev.data.id).toBe('n0');
    unsub();
  });

  it('emits hover and context-menu ViewEvents from rows', async () => {
    const handler = vi.fn();
    const unsub = onViewEvent(handler);
    const { container } = render(VirtualNodeList, {
      props: { nodes: NODES, viewportHeight: 480, rowHeight: ROW_HEIGHT },
    });
    const row = renderedRows(container)[0];
    await fireEvent.mouseEnter(row);
    expect(handler.mock.calls.at(-1)?.[0].type).toBe('hover');
    await fireEvent.contextMenu(row);
    expect(handler.mock.calls.at(-1)?.[0].type).toBe('context-menu');
    unsub();
  });
});
