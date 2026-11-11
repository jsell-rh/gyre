/**
 * SideBySideView tests (ui-layout.md §10: side-by-side → CSS Grid 50/50 or
 * 60/40, two sub-views rendered independently).
 */
import { describe, it, expect, beforeEach } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';

import SideBySideView from '../lib/SideBySideView.svelte';

const NODES = [
  { id: 'a', name: 'Alpha', node_type: 'function', visibility: 'public' },
  { id: 'b', name: 'Beta', node_type: 'type', visibility: 'public' },
];

beforeEach(() => {
  global.ResizeObserver = class ResizeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
});

describe('SideBySideView', () => {
  it('renders two panes in a CSS Grid (graph left, list right)', () => {
    const { container } = render(SideBySideView, {
      props: { nodes: NODES, edges: [], repoId: 'r1' },
    });
    const grid = container.querySelector('.sbs-grid');
    expect(grid).toBeTruthy();
    // CSS Grid with two columns
    expect(grid.style.gridTemplateColumns).toBe('1fr 1fr');
    const panes = container.querySelectorAll('.sbs-pane');
    expect(panes).toHaveLength(2);
    // Left pane renders the SVG graph canvas; right pane the node list
    expect(panes[0].querySelector('.graph-svg, .density-banner, .vlist')).toBeTruthy();
    expect(panes[1].querySelector('.vlist')).toBeTruthy();
  });

  it('toggles the split ratio between 50/50 and 60/40', async () => {
    const { container } = render(SideBySideView, {
      props: { nodes: NODES, edges: [], repoId: 'r1' },
    });
    const grid = container.querySelector('.sbs-grid');
    expect(grid.style.gridTemplateColumns).toBe('1fr 1fr');
    await fireEvent.click(container.querySelectorAll('.sbs-toolbar button')[1]);
    expect(grid.style.gridTemplateColumns).toBe('3fr 2fr');
    await fireEvent.click(container.querySelectorAll('.sbs-toolbar button')[0]);
    expect(grid.style.gridTemplateColumns).toBe('1fr 1fr');
  });
});
