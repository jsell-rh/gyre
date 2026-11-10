/**
 * Explorer shell ViewEvent handling tests (ui-layout.md §10 Interaction
 * Events): the shell handles all events uniformly —
 *   click → detail panel, dblclick → drill-down, hover → tooltip,
 *   context-menu → actions menu — and owns the >1000-node list fallback.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { dispatchViewEvent } from '../lib/viewEvents.js';

const { apiMocks, smallGraph } = vi.hoisted(() => {
  const smallGraph = {
    nodes: [
      { id: 'n1', name: 'AuthService', node_type: 'function', visibility: 'public', file_path: 'src/a.rs', repo_id: 'r1' },
      { id: 'n2', name: 'UserModel', node_type: 'type', visibility: 'public', file_path: 'src/b.rs', repo_id: 'r1' },
    ],
    edges: [],
  };
  const apiMocks = {
    workspaces: vi.fn().mockResolvedValue([]),
    workspaceBudget: vi.fn().mockResolvedValue(null),
    repos: vi.fn().mockResolvedValue([]),
    workspaceRepos: vi.fn().mockResolvedValue([]),
    allRepos: vi.fn().mockResolvedValue([]),
    repoGraph: vi.fn().mockResolvedValue(smallGraph),
    getGraphConcept: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    repoDependencies: vi.fn().mockResolvedValue([]),
    repoDependents: vi.fn().mockResolvedValue([]),
    repoGraphRisks: vi.fn().mockResolvedValue([]),
    repoGraphTypes: vi.fn().mockResolvedValue({ nodes: [] }),
    repoGraphModules: vi.fn().mockResolvedValue({ nodes: [] }),
    repoGraphTimeline: vi.fn().mockResolvedValue([]),
  };
  return { apiMocks, smallGraph };
});

vi.mock('../lib/api.js', () => ({ api: apiMocks }));
vi.mock('../lib/toast.svelte.js', () => ({ toast: vi.fn() }));

import ExplorerView from '../components/ExplorerView.svelte';

function bigGraph(count) {
  return {
    nodes: Array.from({ length: count }, (_, i) => ({
      id: `n${i}`,
      name: `node-${i}`,
      node_type: 'function',
      visibility: 'public',
      file_path: `src/f${i}.rs`,
      repo_id: 'r1',
    })),
    edges: [],
  };
}

const REPO_SCOPE = { type: 'repo', workspaceId: 'ws-1', repoId: 'r1' };

beforeEach(() => {
  vi.clearAllMocks();
  global.window.matchMedia = vi.fn(() => ({
    matches: false,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  }));
  global.ResizeObserver = class ResizeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  global.requestAnimationFrame = vi.fn(cb => { cb(); return 1; });
  global.cancelAnimationFrame = vi.fn();
  HTMLCanvasElement.prototype.getContext = vi.fn(() => ({
    clearRect: vi.fn(), fillRect: vi.fn(), strokeRect: vi.fn(),
    beginPath: vi.fn(), closePath: vi.fn(), arc: vi.fn(), fill: vi.fn(),
    stroke: vi.fn(), moveTo: vi.fn(), lineTo: vi.fn(), quadraticCurveTo: vi.fn(),
    fillText: vi.fn(), measureText: vi.fn(() => ({ width: 40 })),
    scale: vi.fn(), setTransform: vi.fn(), save: vi.fn(), restore: vi.fn(),
    translate: vi.fn(), rotate: vi.fn(),
    fillStyle: '', strokeStyle: '', lineWidth: 0, globalAlpha: 1,
    font: '', textAlign: '', textBaseline: '', shadowColor: '', shadowBlur: 0,
    setLineDash: vi.fn(), getLineDash: vi.fn(() => []),
  }));
  global.WebSocket = class MockWebSocket {
    constructor() {
      this.readyState = 1;
      this.send = vi.fn();
      this.close = vi.fn();
    }
  };
});

afterEach(() => {
  // clearAllMocks (not restoreAllMocks): restore would strip the
  // mockResolvedValue implementations these vi.fn() mocks rely on.
  vi.clearAllMocks();
});

function clickEvent(data, extra = {}) {
  return {
    type: 'click',
    entity_type: 'node',
    entity_id: data?.id ?? 'x',
    position: { x: 10, y: 10 },
    data,
    ...extra,
  };
}

describe('ExplorerView uniform ViewEvent handling', () => {
  it('registers the handler on mount: click opens the detail panel', async () => {
    render(ExplorerView, { props: { scope: REPO_SCOPE } });
    await waitFor(() => {
      expect(apiMocks.repoGraph).toHaveBeenCalledWith('r1');
    });
    // No detail panel yet
    expect(document.querySelector('.explorer-detail-area')).toBeNull();

    dispatchViewEvent(null, clickEvent(smallGraph.nodes[0]));
    await waitFor(() => {
      expect(document.querySelector('.explorer-detail-area')).toBeTruthy();
    });
  });

  it('ignores click events already serviced by an integrated emitter (handled flag)', async () => {
    render(ExplorerView, { props: { scope: REPO_SCOPE } });
    await waitFor(() => {
      expect(apiMocks.repoGraph).toHaveBeenCalledWith('r1');
    });
    dispatchViewEvent(null, clickEvent(smallGraph.nodes[0], { handled: true }));
    await new Promise(r => setTimeout(r, 20));
    expect(document.querySelector('.explorer-detail-area')).toBeNull();
  });

  it('hover shows the ViewEvent tooltip', async () => {
    render(ExplorerView, { props: { scope: REPO_SCOPE } });
    await waitFor(() => {
      expect(apiMocks.repoGraph).toHaveBeenCalledWith('r1');
    });
    dispatchViewEvent(null, {
      type: 'hover',
      entity_type: 'node',
      entity_id: 'n1',
      position: { x: 100, y: 200 },
      data: smallGraph.nodes[0],
    });
    await waitFor(() => {
      const tip = document.querySelector('.viewevent-tooltip');
      expect(tip).toBeTruthy();
      expect(tip.textContent).toContain('AuthService');
      expect(tip.style.left).toBe('114px'); // position.x + 14
    });
  });

  it('context-menu opens the actions menu with entity actions', async () => {
    render(ExplorerView, { props: { scope: REPO_SCOPE } });
    await waitFor(() => {
      expect(apiMocks.repoGraph).toHaveBeenCalledWith('r1');
    });
    dispatchViewEvent(null, {
      type: 'context-menu',
      entity_type: 'node',
      entity_id: 'n1',
      position: { x: 50, y: 60 },
      data: smallGraph.nodes[0],
    });
    await waitFor(() => {
      expect(document.querySelector('.viewevent-menu')).toBeTruthy();
    });
    const menu = document.querySelector('.viewevent-menu');
    expect(menu.textContent).toContain('AuthService');
    // Actions conditioned on the entity's data
    expect(menu.textContent).toContain('View details');
    expect(menu.textContent).toContain('Open in code'); // file_path present

    // "View details" opens the detail panel (click → panel semantics)
    await fireEvent.click(menu.querySelectorAll('.viewevent-menu-item')[0]);
    await waitFor(() => {
      expect(document.querySelector('.explorer-detail-area')).toBeTruthy();
    });
  });

  it('dblclick drills into the entity repo (loads that graph)', async () => {
    render(ExplorerView, { props: { scope: REPO_SCOPE } });
    await waitFor(() => {
      expect(apiMocks.repoGraph).toHaveBeenCalledWith('r1');
    });
    dispatchViewEvent(null, {
      type: 'dblclick',
      entity_type: 'node',
      entity_id: 'x1',
      position: { x: 0, y: 0 },
      data: { id: 'x1', name: 'OtherRepoNode', node_type: 'function', repo_id: 'r2' },
    });
    await waitFor(() => {
      expect(apiMocks.repoGraph).toHaveBeenCalledWith('r2');
    });
  });

  it('Escape closes the actions menu (overlay cascade)', async () => {
    render(ExplorerView, { props: { scope: REPO_SCOPE } });
    await waitFor(() => {
      expect(apiMocks.repoGraph).toHaveBeenCalledWith('r1');
    });
    dispatchViewEvent(null, {
      type: 'context-menu',
      entity_type: 'node',
      entity_id: 'n1',
      position: { x: 50, y: 60 },
      data: smallGraph.nodes[0],
    });
    await waitFor(() => {
      expect(document.querySelector('.viewevent-menu')).toBeTruthy();
    });
    await fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() => {
      expect(document.querySelector('.viewevent-menu')).toBeNull();
    });
  });
});

describe('ExplorerView density list fallback (ui-layout.md §10)', () => {
  it('swaps the canvas for a virtualized list above 1000 nodes', async () => {
    apiMocks.repoGraph.mockResolvedValue(bigGraph(1200));
    render(ExplorerView, { props: { scope: REPO_SCOPE } });
    await waitFor(() => {
      expect(document.querySelector('.density-list-fallback')).toBeTruthy();
    });
    expect(document.querySelector('.density-list-banner').textContent)
      .toContain('switched to list layout with virtual scrolling');
    expect(document.querySelector('.vlist')).toBeTruthy();
    // Virtualized — only a window of the 1200 rows is mounted
    const rows = document.querySelectorAll('.vlist-row');
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.length).toBeLessThan(1200);
    // The 2D canvas is not rendered in fallback mode
    expect(document.querySelector('canvas.treemap-canvas')).toBeNull();
  });

  it('keeps the canvas below the threshold', async () => {
    apiMocks.repoGraph.mockResolvedValue(smallGraph);
    render(ExplorerView, { props: { scope: REPO_SCOPE } });
    await waitFor(() => {
      expect(document.querySelector('canvas.treemap-canvas')).toBeTruthy();
    });
    expect(document.querySelector('.density-list-fallback')).toBeNull();
  });
});
