/**
 * ExplorerViewDrillUrl.test.js — ui-layout.md §3 Drill-Down / Scope drill
 *
 * ExplorerView in-place repo drill (workspace scope, no host shell): drilling
 * into a repo loads its graph in this view AND updates the URL via
 * history.pushState (spec: "the breadcrumb updates, the URL changes via
 * history.pushState ... and the canvas re-renders for the new scope"). No
 * full-page reload. backToRepoList pops the drill URL entry (Back returns to
 * the workspace graph).
 *
 * Since task-174 the workspace scope renders the realized-architecture graph
 * (repos as nodes, ui-layout.md §5) instead of a repo-card list, so the drill
 * is driven by double-clicking a rendered repo node (dep-node-*). Without a
 * host shell (no onEnterRepo prop / goToRepo context) the double-click falls
 * back to the in-place drill (selectRepo), which owns the URL behavior
 * asserted here.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    workspaces: vi.fn().mockResolvedValue([]),
    workspaceBudget: vi.fn().mockResolvedValue(null),
    repos: vi.fn().mockResolvedValue([]),
    workspaceRepos: vi.fn().mockResolvedValue([]),
    allRepos: vi.fn().mockResolvedValue([]),
    repoGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    getGraphConcept: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    workspaceDependencyGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    mergeRequests: vi.fn().mockResolvedValue([]),
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({ toast: vi.fn() }));

// ELK uses WASM — mock the layout engine with deterministic positions
// (same pattern as ExplorerViewWorkspaceGraph.test.js).
vi.mock('../lib/layout-engines.js', () => ({
  elkLayout: vi.fn().mockImplementation(async (nodes) => {
    const positions = {};
    nodes.forEach((n, i) => {
      positions[n.id] = { x: 100 + i * 200, y: 80 };
    });
    return positions;
  }),
}));

import ExplorerView from '../components/ExplorerView.svelte';

const REPO = { id: 'repo-1', name: 'payment-api' };

const DEP_GRAPH = {
  nodes: [{ repo_id: 'repo-1', name: 'payment-api' }],
  edges: [],
};

describe('ExplorerView — workspace-scope repo drill URL (ui-layout.md §3)', () => {
  let pushStateSpy;

  beforeEach(() => {
    window.history.pushState({}, '', '/workspaces/payments/explorer');
    vi.clearAllMocks();
    global.window.matchMedia = vi.fn(() => ({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }));
    global.ResizeObserver = class ResizeObserver {
      observe() {}
      disconnect() {}
      unobserve() {}
    };
    global.requestAnimationFrame = vi.fn(cb => { cb(); return 1; });
    global.cancelAnimationFrame = vi.fn();
    // ExplorerCanvas globals (needed once the drilled repo graph mounts)
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
    const realPushState = window.history.pushState.bind(window.history);
    pushStateSpy = vi.spyOn(window.history, 'pushState').mockImplementation(realPushState);
  });

  afterEach(() => {
    pushStateSpy.mockRestore();
    vi.restoreAllMocks();
    // Reset any drill param left by a test so later renders start clean
    window.history.replaceState(null, '', '/');
  });

  /** Render ExplorerView at workspace scope with the repo available both in
   *  the workspace repo list and the dependency graph, and wait for the
   *  rendered repo node. */
  async function renderWorkspaceGraph() {
    const { api } = await import('../lib/api.js');
    api.repos.mockResolvedValue([REPO]);
    api.workspaceRepos.mockResolvedValue([REPO]);
    api.allRepos.mockResolvedValue([REPO]);
    api.workspaceDependencyGraph.mockResolvedValue(DEP_GRAPH);

    const { container } = render(ExplorerView, {
      props: { scope: { type: 'workspace', workspaceId: 'ws-1' } },
    });

    // Workspace scope shows the realized-architecture graph (ui-layout.md §5)
    const repoNode = await waitFor(() => {
      const el = container.querySelector('[data-testid="dep-node-repo-1"]');
      expect(el).toBeTruthy();
      return el;
    }, { timeout: 5000 });
    return { container, repoNode };
  }

  it('drilling into a repo at workspace scope pushes a drill URL (?repo=name), no reload', async () => {
    const { api } = await import('../lib/api.js');
    const { container, repoNode } = await renderWorkspaceGraph();

    pushStateSpy.mockClear();

    // Double-click the repo node → in-place scope drill (selectRepo fallback)
    await fireEvent.dblClick(repoNode);

    // URL updated via history.pushState with the repo query param
    await waitFor(() => {
      expect(pushStateSpy).toHaveBeenCalled();
    });
    const url = new URL(window.location.href);
    expect(url.searchParams.get('repo')).toBe('payment-api');
    // Drill did not navigate away from the explorer view
    expect(url.pathname).toContain('explorer');

    // Graph load was triggered for the drilled repo (canvas re-renders
    // for the new scope level)
    await waitFor(() => {
      expect(api.repoGraph).toHaveBeenCalledWith('repo-1');
    });
  });

  it('Back from the drilled state (backToRepoList) removes the drill param', async () => {
    const { container, repoNode } = await renderWorkspaceGraph();

    // Drill in
    await fireEvent.dblClick(repoNode);
    await waitFor(() => {
      expect(new URL(window.location.href).searchParams.get('repo')).toBe('payment-api');
    });

    // Back to the workspace graph pops the drill entry
    const backBtn = await waitFor(() => {
      const btn = container.querySelector('.back-to-repos-btn');
      expect(btn).toBeTruthy();
      return btn;
    }, { timeout: 5000 });
    pushStateSpy.mockClear();
    await fireEvent.click(backBtn);

    await waitFor(() => {
      expect(pushStateSpy).toHaveBeenCalled();
    });
    const url = new URL(window.location.href);
    expect(url.searchParams.has('repo')).toBe(false);
    // The workspace graph is visible again
    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-node-repo-1"]')).toBeTruthy();
    });
  });

  it('restores the drilled repo from ?repo= on mount (deep-link)', async () => {
    const { api } = await import('../lib/api.js');
    api.repos.mockResolvedValue([REPO]);
    api.workspaceRepos.mockResolvedValue([REPO]);
    api.allRepos.mockResolvedValue([REPO]);
    api.workspaceDependencyGraph.mockResolvedValue(DEP_GRAPH);

    // Deep-link: the URL already carries the drill param (as left by
    // selectRepo on a previous page lifetime, or shared by a user).
    window.history.pushState({}, '', '/workspaces/payments/explorer?repo=payment-api');

    const { container } = render(ExplorerView, {
      props: { scope: { type: 'workspace', workspaceId: 'ws-1' } },
    });

    // The drilled graph loads (canvas re-renders for the drilled scope)
    // without another pushState - the URL is already the drilled one.
    await waitFor(() => {
      expect(api.repoGraph).toHaveBeenCalledWith('repo-1');
    });
    // The workspace graph nodes are replaced by the drilled graph view
    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-node-repo-1"]')).toBeFalsy();
    });
    // The back-to-repos button appears (drilled state is active)
    expect(container.querySelector('.back-to-repos-btn')).toBeTruthy();
  });

  it('browser Back from the drilled URL returns to the workspace graph (popstate sync)', async () => {
    const { container, repoNode } = await renderWorkspaceGraph();

    // Drill in (pushes ?repo=payment-api)
    await fireEvent.dblClick(repoNode);
    await waitFor(() => {
      expect(new URL(window.location.href).searchParams.get('repo')).toBe('payment-api');
    });
    await waitFor(() => {
      expect(container.querySelector('.back-to-repos-btn')).toBeTruthy();
    });

    // Browser Back: popstate fires with the URL param gone - the view must
    // return to the workspace graph (no drift between URL and view state).
    window.history.back();
    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-node-repo-1"]')).toBeTruthy();
    });
    await waitFor(() => {
      expect(new URL(window.location.href).searchParams.has('repo')).toBe(false);
    });
    expect(container.querySelector('.back-to-repos-btn')).toBeFalsy();
  });
});
