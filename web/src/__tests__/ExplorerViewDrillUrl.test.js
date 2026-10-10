/**
 * ExplorerViewDrillUrl.test.js — ui-layout.md §3 Drill-Down / Scope drill
 *
 * ExplorerView.selectRepo (workspace scope): selecting a repo loads its
 * graph in this view AND updates the URL via history.pushState (spec:
 * "the breadcrumb updates, the URL changes via history.pushState ... and
 * the canvas re-renders for the new scope"). No full-page reload.
 * backToRepoList pops the drill URL entry (Back returns to the repo list).
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
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({ toast: vi.fn() }));

import ExplorerView from '../components/ExplorerView.svelte';

const REPO = { id: 'repo-1', name: 'payment-api' };

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
    const realPushState = window.history.pushState.bind(window.history);
    pushStateSpy = vi.spyOn(window.history, 'pushState').mockImplementation(realPushState);
  });

  afterEach(() => {
    pushStateSpy.mockRestore();
    vi.restoreAllMocks();
  });

  it('selecting a repo at workspace scope pushes a drill URL (?repo=name), no reload', async () => {
    const { api } = await import('../lib/api.js');
    api.repos.mockResolvedValue([REPO]);
    api.workspaceRepos.mockResolvedValue([REPO]);
    api.allRepos.mockResolvedValue([REPO]);

    const { container } = render(ExplorerView, {
      props: { scope: { type: 'workspace', workspaceId: 'ws-1' } },
    });

    // Workspace scope shows the repo list (S4.4b)
    const repoCard = await waitFor(() => {
      const btn = container.querySelector('.ws-repo-card');
      expect(btn).toBeTruthy();
      return btn;
    }, { timeout: 5000 });

    pushStateSpy.mockClear();

    // Click the repo card → scope drill (selectRepo)
    await fireEvent.click(repoCard);

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
    const { api } = await import('../lib/api.js');
    api.repos.mockResolvedValue([REPO]);
    api.workspaceRepos.mockResolvedValue([REPO]);
    api.allRepos.mockResolvedValue([REPO]);

    const { container } = render(ExplorerView, {
      props: { scope: { type: 'workspace', workspaceId: 'ws-1' } },
    });

    const repoCard = await waitFor(() => {
      const btn = container.querySelector('.ws-repo-card');
      expect(btn).toBeTruthy();
      return btn;
    }, { timeout: 5000 });

    // Drill in
    await fireEvent.click(repoCard);
    await waitFor(() => {
      expect(new URL(window.location.href).searchParams.get('repo')).toBe('payment-api');
    });

    // Back to repo list pops the drill entry
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
    // Repo list is visible again
    await waitFor(() => {
      expect(container.querySelector('.ws-repo-card')).toBeTruthy();
    });
  });
});
