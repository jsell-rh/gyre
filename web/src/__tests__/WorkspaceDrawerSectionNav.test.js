// F6 (R9): workspace-scope navigation clicks for Briefing / Architecture /
// Specs must scroll to real section elements in the DOM. Previously these
// testids did not exist at workspace scope, so the scroll targeted nothing.
// This is an app-level test: App.svelte renders the REAL WorkspaceHome
// (AppShell.test.js stubs it, so it cannot catch this class of bug).
//
// ui-navigation.md §8: the mobile drawer lists the workspace-home sections
// and its links navigate to scroll anchors on the workspace home page.

import { describe, it, expect, vi, beforeEach } from 'vitest';
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
    workspaceDependencyPolicy: vi.fn().mockResolvedValue(null),
  },
  setAuthToken: vi.fn(),
}));

vi.mock('../lib/ExplorerCanvas.svelte', () => ({
  default: vi.fn().mockImplementation(() => ({ $destroy: () => {} })),
}));
vi.mock('../lib/toast.svelte.js', () => ({
  toastInfo: vi.fn(),
  toastError: vi.fn(),
  getToasts: vi.fn().mockReturnValue([]),
}));


import { api } from '../lib/api.js';
import App from '../App.svelte';

const WS = [{ id: 'ws-1', name: 'Payments', slug: 'payments' }];

describe('Workspace drawer section navigation (F6/R9 — workspace-scope scroll targets)', () => {
  beforeEach(() => {
    window.history.pushState({}, '', '/');
    localStorage.clear();
    vi.clearAllMocks();
    Element.prototype.scrollIntoView = vi.fn();
    api.workspaces.mockResolvedValue(WS);
    api.workspaceRepos.mockResolvedValue([{ id: 'repo-1', name: 'core' }]);
    api.workspaceBudget.mockResolvedValue(null);
    api.notificationCount.mockResolvedValue(0);
    api.version.mockResolvedValue({ version: '0.1.0' });
  });

  async function renderApp() {
    const { container } = render(App);
    // Wait until the workspace home has rendered (workspace auto-selected)
    // and the drawer is open.
    await waitFor(() => {
      expect(container.querySelector('[data-testid="hamburger-btn"]')).toBeTruthy();
    }, { timeout: 5000 });
    await fireEvent.click(container.querySelector('[data-testid="hamburger-btn"]'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="drawer-item-briefing"]')).toBeTruthy();
    }, { timeout: 5000 });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="section-briefing"]')).toBeTruthy();
    }, { timeout: 5000 });
    return container;
  }

  // F6: Briefing drawer click must scroll to a real section element.
  it('renders section-briefing and scrolls to it when the Briefing drawer item is clicked', async () => {
    const container = await renderApp();
    await fireEvent.click(container.querySelector('[data-testid="drawer-item-briefing"]'));
    const section = container.querySelector('[data-testid="section-briefing"]');
    expect(section).toBeTruthy();
    await waitFor(() => {
      expect(Element.prototype.scrollIntoView).toHaveBeenCalled();
    }, { timeout: 3000 });
  });

  // F6: drawer navigation must reach the real DOM sections of the workspace
  // home — the repos section (primary repo list) and specs section.
  it('renders section-repos and section-specs on the workspace home', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="section-repos"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="section-specs"]')).toBeTruthy();
    }, { timeout: 5000 });
  });

  // F6: Specs drawer click must scroll to a real section element.
  it('renders section-specs and scrolls to it when the Specs drawer item is clicked', async () => {
    const container = await renderApp();
    await fireEvent.click(container.querySelector('[data-testid="drawer-item-specs"]'));
    const section = container.querySelector('[data-testid="section-specs"]');
    expect(section).toBeTruthy();
    await waitFor(() => {
      expect(Element.prototype.scrollIntoView).toHaveBeenCalled();
    }, { timeout: 3000 });
  });
});
