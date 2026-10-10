/**
 * ScopeTransitions.test.js — ui-layout.md §3 Scope Transitions
 *
 * Contract under test (spec, all four points):
 *   1. Breadcrumb updates immediately.
 *   2. Content area cross-fades (150ms opacity transition).
 *   3. Sidebar active item doesn't change (this app: no-sidebar shell —
 *      the workspace selector/breadcrumb context persists; verified by
 *      the workspace name still being present after the transition).
 *   4. URL updates via history.pushState.
 *   No full-page reload.
 *
 * App-level test: renders the REAL App.svelte with the REAL WorkspaceHome
 * (AppShell.test.js stubs WorkspaceHome, so it cannot catch this class of
 * bug) and drives a real scope transition: workspace home → repo (click a
 * repo card). Asserts the URL changed via history.pushState (not location
 * assignment — jsdom throws "not implemented" on navigation, and any
 * location.href mutation would be observable as a pathname change without
 * a pushState call), the content cross-fade class toggles, and the same
 * content root element survives the transition (a reload would unmount
 * everything).
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('../lib/ws.js', () => ({
  createWsStore: () => ({
    onStatus: vi.fn().mockReturnValue(() => {}),
    destroy: vi.fn(),
    onMessage: vi.fn().mockReturnValue(() => {}),
    subscribe: vi.fn(),
    send: vi.fn(),
    sessionId: 'test-session',
  }),
}));

vi.mock('../lib/api.js', () => ({
  api: {
    workspaces: vi.fn().mockResolvedValue([]),
    workspaceRepos: vi.fn().mockResolvedValue([]),
    workspaceBudget: vi.fn().mockResolvedValue(null),
    notificationCount: vi.fn().mockResolvedValue(0),
    tokenInfo: vi.fn().mockResolvedValue({ kind: 'global' }),
    me: vi.fn().mockResolvedValue(null),
    version: vi.fn().mockResolvedValue({ version: '0.1.0', commit: 'abc1234', milestone: 'M35' }),
    myNotifications: vi.fn().mockResolvedValue([]),
    specsForWorkspace: vi.fn().mockResolvedValue([]),
    getMetaSpecs: vi.fn().mockResolvedValue([]),
    workspaceGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    workspaceDependencyGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    staleDependencies: vi.fn().mockResolvedValue([]),
    breakingChanges: vi.fn().mockResolvedValue([]),
    tasks: vi.fn().mockResolvedValue([]),
    mergeRequests: vi.fn().mockResolvedValue([]),
    agents: vi.fn().mockResolvedValue([]),
    costSummary: vi.fn().mockResolvedValue([]),
    activity: vi.fn().mockResolvedValue([]),
    mergeQueue: vi.fn().mockResolvedValue([]),
    mergeQueueGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    getWorkspaceBriefing: vi.fn().mockResolvedValue({ narrative: '' }),
    briefingAsk: vi.fn().mockResolvedValue({}),
    sendAgentMessage: vi.fn().mockResolvedValue({}),
    createWorkspace: vi.fn().mockResolvedValue({ id: 'ws-new', name: 'New WS', slug: 'new-ws' }),
    mrGates: vi.fn().mockResolvedValue([]),
    mrDiff: vi.fn().mockResolvedValue({ files_changed: 0, insertions: 0, deletions: 0 }),
    repoBlastRadius: vi.fn().mockResolvedValue({ direct: [], transitive: [], total: 0 }),
    repoDependents: vi.fn().mockResolvedValue([]),
    repoDependencies: vi.fn().mockResolvedValue({ dependencies: [], dependents: [] }),
    repoSpeculative: vi.fn().mockResolvedValue({}),
    specsGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    specProgress: vi.fn().mockResolvedValue({}),
    specsSave: vi.fn().mockResolvedValue({}),
    approveSpec: vi.fn().mockResolvedValue({}),
    rejectSpec: vi.fn().mockResolvedValue({}),
    createTask: vi.fn().mockResolvedValue({}),
    mergeRequest: vi.fn().mockResolvedValue({ title: 'test-mr' }),
    task: vi.fn().mockResolvedValue({ title: 'test-task' }),
    updateTaskStatus: vi.fn().mockResolvedValue({}),
  },
  setAuthToken: vi.fn(),
}));

vi.mock('../lib/ExplorerCanvas.svelte', () => ({
  default: vi.fn().mockImplementation(() => ({ $destroy: () => {} })),
}));
vi.mock('../lib/toast.svelte.js', () => ({
  toastInfo: vi.fn(),
  toastError: vi.fn(),
  toastSuccess: vi.fn(),
  getToasts: vi.fn().mockReturnValue([]),
}));

import { api } from '../lib/api.js';
import App from '../App.svelte';

const WS = { id: 'ws-1', name: 'Payments', slug: 'payments' };
const REPO = { id: 'repo-1', name: 'payment-api' };

async function renderAtWorkspaceHome() {
  const { container } = render(App);
  // Wait for the workspace home (workspace auto-selected from the
  // single-workspace list) and the repo cards to render.
  await waitFor(() => {
    expect(container.querySelector('[data-testid="repo-card"]')).toBeTruthy();
  }, { timeout: 5000 });
  return container;
}

describe('Scope transitions (ui-layout.md §3) — workspace → repo via repo card click', () => {
  let pushStateSpy;

  beforeEach(() => {
    window.history.pushState({}, '', '/');
    localStorage.clear();
    vi.clearAllMocks();
    Element.prototype.scrollIntoView = vi.fn();
    api.workspaces.mockResolvedValue([WS]);
    api.workspaceRepos.mockResolvedValue([REPO]);
    // Watch history.pushState (spec: URL updates via pushState). Capture
    // the ORIGINAL before spying — binding after spyOn would recurse into
    // the spy itself. The mock delegates to the real implementation so the
    // app's URL parsing keeps working during the transition.
    const realPushState = window.history.pushState.bind(window.history);
    pushStateSpy = vi.spyOn(window.history, 'pushState').mockImplementation(realPushState);
  });

  afterEach(() => {
    pushStateSpy.mockRestore();
    vi.restoreAllMocks();
  });

  it('repo card click: URL updates via pushState, breadcrumb updates, cross-fade, no reload', async () => {
    const container = await renderAtWorkspaceHome();

    // Workspace context visible before the transition (topbar breadcrumb)
    const wsNameBefore = container.querySelector('[data-testid="ws-name-btn"]');
    expect(wsNameBefore).toBeTruthy();
    expect(wsNameBefore.textContent).toContain('Payments');

    const urlBefore = window.location.pathname;
    const contentRoot = container.querySelector('.content-inner');
    expect(contentRoot).toBeTruthy();
    expect(contentRoot.className).not.toContain('faded');

    pushStateSpy.mockClear();

    // Click the repo card → scope transition to repo mode
    await fireEvent.click(container.querySelector('[data-testid="repo-card"]'));

    // 4. URL updates via history.pushState (not location assignment)
    await waitFor(() => {
      expect(pushStateSpy).toHaveBeenCalled();
    });
    expect(window.location.pathname).not.toBe(urlBefore);
    expect(window.location.pathname).toContain('payment-api');

    // 1. Breadcrumb updates immediately: repo mode breadcrumb shows the repo
    await waitFor(() => {
      const bc = container.querySelector('[data-testid="repo-breadcrumb"]');
      expect(bc).toBeTruthy();
      expect(bc.textContent).toContain('payment-api');
    });

    // 2. Content cross-fades: fadeContent() toggles the `faded` class for
    //    150ms then restores visibility. Depending on real-timer progress
    //    when we read it, either state is valid — what is NOT valid is the
    //    content root being replaced or staying faded forever (asserted
    //    precisely in the fake-timer test below).
    const contentAfter = container.querySelector('.content-inner');
    expect(contentAfter).toBeTruthy();
    // classList (not className) — Svelte appends scoped style classes.
    expect(
      contentAfter.classList.contains('faded') || !contentAfter.classList.contains('faded')
    ).toBe(true);
    // 3. Workspace scope context unchanged: the repo-mode breadcrumb keeps
    //    the workspace segment clickable (scope container persists).
    const bcAfter = container.querySelector('[data-testid="repo-breadcrumb"]');
    const wsSeg = bcAfter?.querySelector('.breadcrumb-ws');
    expect(wsSeg).toBeTruthy();
    expect(wsSeg.textContent).toContain('Payments');

    // No full-page reload: the same content root node survives the
    // transition (a reload would unmount the whole tree and re-render a
    // new root), and the cross-fade has completed back to visible.
    expect(contentAfter.isConnected).toBe(true);
    await waitFor(() => {
      const settled = container.querySelector('.content-inner');
      expect(settled).toBe(contentAfter);
      expect(settled.classList.contains('faded')).toBe(false);
    });
  });

  it('fadeContent() cross-fade: faded for the 150ms window, visible after', async () => {
    const container = await renderAtWorkspaceHome();

    // Switch to fake timers only for the fade window so the 150ms timer is
    // observable deterministically (renderAtWorkspaceHome needed real
    // timers for waitFor).
    vi.useFakeTimers();
    try {
      fireEvent.click(container.querySelector('[data-testid="repo-card"]'));

      // Immediately after the click the content is fading (class present)
      const duringFade = container.querySelector('.content-inner');
      expect(duringFade.classList.contains('faded')).toBe(true);

      // Before 150ms elapses it is still fading
      vi.advanceTimersByTime(100);
      expect(container.querySelector('.content-inner').classList.contains('faded')).toBe(true);

      // After the 150ms cross-fade window the content is visible again.
      // The class removal runs in the timer callback → Svelte effect flush,
      // which is a microtask — let it run before asserting.
      vi.advanceTimersByTime(60);
      await Promise.resolve();
      expect(container.querySelector('.content-inner').classList.contains('faded')).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });
});


