// F6 (R9): workspace-scope sidebar clicks for Briefing / Explorer / Specs must
// scroll to real section elements in the DOM. Previously these testids did not
// exist at workspace scope, so the scroll targeted nothing.
// This is an app-level test: App.svelte renders the REAL WorkspaceHome
// (AppShell.test.js stubs it, so it cannot catch this class of bug).

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

describe('Workspace sidebar section navigation (F6/R9 — HSI §1.3 workspace-scope scroll targets)', () => {
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
    // Wait until the workspace home has rendered (workspace auto-selected).
    await waitFor(() => {
      expect(container.querySelector('[data-testid="sidebar-item-briefing"]')).toBeTruthy();
    }, { timeout: 5000 });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="section-briefing"]')).toBeTruthy();
    }, { timeout: 5000 });
    return container;
  }

  // F6: Briefing sidebar click must scroll to a real section element.
  it('renders section-briefing and scrolls to it when the Briefing sidebar item is clicked', async () => {
    const container = await renderApp();
    await fireEvent.click(container.querySelector('[data-testid="sidebar-item-briefing"]'));
    const section = container.querySelector('[data-testid="section-briefing"]');
    expect(section).toBeTruthy();
    expect(Element.prototype.scrollIntoView).toHaveBeenCalled();
  });

  // F6: Explorer sidebar click must scroll to the Architecture section (the
  // realized-architecture view at workspace scope) and expand it.
  it('renders section-architecture, expands and scrolls to it when the Explorer sidebar item is clicked', async () => {
    const container = await renderApp();
    await fireEvent.click(container.querySelector('[data-testid="sidebar-item-explorer"]'));
    const section = container.querySelector('[data-testid="section-architecture"]');
    expect(section).toBeTruthy();
    // Explorer click expands the collapsed architecture section so the scroll
    // target has content.
    await waitFor(() => {
      expect(container.querySelector('[data-testid="arch-toggle"]').getAttribute('aria-expanded')).toBe('true');
    }, { timeout: 3000 });
    expect(container.querySelector('[data-testid="arch-body"]')).toBeTruthy();
    expect(Element.prototype.scrollIntoView).toHaveBeenCalled();
  });

  // F6: Specs sidebar click must scroll to a real section element.
  it('renders section-specs and scrolls to it when the Specs sidebar item is clicked', async () => {
    const container = await renderApp();
    await fireEvent.click(container.querySelector('[data-testid="sidebar-item-specs"]'));
    const section = container.querySelector('[data-testid="section-specs"]');
    expect(section).toBeTruthy();
    expect(Element.prototype.scrollIntoView).toHaveBeenCalled();
  });
});
