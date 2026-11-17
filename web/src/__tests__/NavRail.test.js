/**
 * NavRail.test.js — ui-navigation.md §1a Collapsible Navigation Rail
 * (task-082 retargeted requirement)
 *
 * Component contract (§1a "Pattern"):
 *   - Collapsed (icon-only) by default; toggle expands to labelled form;
 *     preference persisted per user per mode in localStorage.
 *   - Scope-labelled header per mode; stable per-mode item ordering.
 *   - Active destination highlighted (aria-current), onSelect fires.
 *
 * App integration:
 *   - Rail renders on workspace home and repo mode (without an open entity
 *     detail) only; never on focused surfaces (sub-pages, entity detail).
 *   - Workspace-home item set mirrors the §2 dashboard sections in
 *     canonical order plus Settings.
 *   - Repo-mode item set mirrors the repo tab strip exactly.
 *   - Selecting a rail item navigates the same way the dashboard/tab strip
 *     does (rail never load-bearing, but must actually work).
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

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
    // WorkspaceHome data surface (same mock set as
    // WorkspaceDrawerSectionNav.test.js, which renders the real
    // WorkspaceHome): rail section selection must scroll to real DOM
    // sections, so the dashboard renders for real.
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
    allRepos: vi.fn().mockResolvedValue([]),
  },
  setAuthToken: vi.fn(),
}));

vi.mock('../lib/ExplorerCanvas.svelte', () => ({
  default: vi.fn().mockImplementation(() => ({ $destroy: () => {} })),
}));
vi.mock('../lib/toast.svelte.js', () => ({
  toast: vi.fn(),
  showToast: vi.fn(),
  toastInfo: vi.fn(),
  toastError: vi.fn(),
  getToasts: vi.fn().mockReturnValue([]),
}));;

vi.mock('../components/RepoMode.svelte', () => ({ default: function RepoModeStub() {} }));
vi.mock('../components/UserProfile.svelte', () => ({ default: function UserProfileStub() {} }));
// The full-page entity detail is a focused surface; its data loading is
// irrelevant to the rail-absence assertion, so stub it.
vi.mock('../lib/DetailPanel.svelte', () => ({ default: function DetailPanelStub() {} }));

import { api } from '../lib/api.js';
import { REPO_TAB_IDS } from '../lib/repoTabs.js';
import NavRail from '../components/NavRail.svelte';
import App from '../App.svelte';

const WS_ITEMS = [
  { id: 'decisions', label: 'Decisions' },
  { id: 'specs', label: 'Specs' },
  { id: 'architecture', label: 'Architecture' },
  { id: 'repos', label: 'Repos' },
  { id: 'briefing', label: 'Briefing' },
  { id: 'agent-rules', label: 'Agent Rules' },
  { id: 'settings', label: 'Settings' },
];

// ── Component contract ─────────────────────────────────────────────────

describe('NavRail component (ui-navigation.md §1a)', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('renders collapsed by default: icon-only, no labels, toggle aria-expanded=false', async () => {
    const { container } = render(NavRail, {
      mode: 'workspace-home',
      scopeLabel: 'Payments',
      items: WS_ITEMS,
      activeId: 'decisions',
      onSelect: () => {},
      userKey: 'user-1',
    });
    const rail = container.querySelector('[data-testid="nav-rail"]');
    expect(rail).toBeTruthy();
    expect(rail.classList.contains('expanded')).toBe(false);
    // Icon-only: no item labels rendered while collapsed.
    expect(container.querySelector('.nav-rail-label')).toBeNull();
    const toggle = container.querySelector('[data-testid="nav-rail-toggle"]');
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    // Scope label header exists (collapsed shows the scope glyph; the full
    // label rides the title tooltip).
    const scope = container.querySelector('[data-testid="nav-rail-scope"]');
    expect(scope.getAttribute('title')).toBe('Payments');
  });

  it('toggle expands to the labelled form and persists the preference per user per mode', async () => {
    const { container } = render(NavRail, {
      mode: 'workspace-home',
      scopeLabel: 'Payments',
      items: WS_ITEMS,
      activeId: null,
      onSelect: () => {},
      userKey: 'user-1',
    });
    await fireEvent.click(container.querySelector('[data-testid="nav-rail-toggle"]'));
    const rail = container.querySelector('[data-testid="nav-rail"]');
    expect(rail.classList.contains('expanded')).toBe(true);
    // Labels appear in item order.
    const labels = Array.from(container.querySelectorAll('.nav-rail-label')).map((el) => el.textContent);
    expect(labels).toEqual(['Decisions', 'Specs', 'Architecture', 'Repos', 'Briefing', 'Agent Rules', 'Settings']);
    expect(localStorage.getItem('gyre_nav_rail_expanded.workspace-home.user-1')).toBe('expanded');
    // Scope label rendered in the header when expanded.
    expect(container.querySelector('.nav-rail-scope-label').textContent).toBe('Payments');
  });

  it('restores the persisted expanded preference on mount', async () => {
    localStorage.setItem('gyre_nav_rail_expanded.repo.user-1', 'expanded');
    const { container } = render(NavRail, {
      mode: 'repo',
      scopeLabel: 'Payments / payment-api',
      items: [{ id: 'specs', label: 'Specs' }],
      activeId: 'specs',
      onSelect: () => {},
      userKey: 'user-1',
    });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="nav-rail"]').classList.contains('expanded')).toBe(true);
    });
    expect(localStorage.getItem('gyre_nav_rail_expanded.workspace-home.user-1')).toBeNull();
  });

  it('collapsing persists collapsed state', async () => {
    localStorage.setItem('gyre_nav_rail_expanded.workspace-home.user-1', 'expanded');
    const { container } = render(NavRail, {
      mode: 'workspace-home',
      scopeLabel: 'Payments',
      items: WS_ITEMS,
      activeId: null,
      onSelect: () => {},
      userKey: 'user-1',
    });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="nav-rail"]').classList.contains('expanded')).toBe(true);
    });
    await fireEvent.click(container.querySelector('[data-testid="nav-rail-toggle"]'));
    expect(container.querySelector('[data-testid="nav-rail"]').classList.contains('expanded')).toBe(false);
    expect(localStorage.getItem('gyre_nav_rail_expanded.workspace-home.user-1')).toBe('collapsed');
  });

  it('highlights the active item (aria-current + active class) and fires onSelect with the item id', async () => {
    const onSelect = vi.fn();
    const { container } = render(NavRail, {
      mode: 'workspace-home',
      scopeLabel: 'Payments',
      items: WS_ITEMS,
      activeId: 'repos',
      onSelect,
      userKey: 'user-1',
    });
    const active = container.querySelector('[data-testid="nav-rail-item-repos"]');
    expect(active.getAttribute('aria-current')).toBe('page');
    expect(active.classList.contains('active')).toBe(true);
    const inactive = container.querySelector('[data-testid="nav-rail-item-specs"]');
    expect(inactive.getAttribute('aria-current')).toBeNull();

    await fireEvent.click(inactive);
    expect(onSelect).toHaveBeenCalledWith('specs');
  });
});

// ── App integration ────────────────────────────────────────────────────

describe('Navigation rail in the app shell (ui-navigation.md §1a)', () => {
  const WS = [{ id: 'ws-1', name: 'Payments', slug: 'payments' }];

  beforeEach(() => {
    window.history.pushState({}, '', '/');
    localStorage.clear();
    vi.clearAllMocks();
    api.workspaces.mockResolvedValue(WS);
    api.workspaceRepos.mockResolvedValue([{ id: 'repo-1', name: 'core' }]);
    api.workspaceBudget.mockResolvedValue(null);
    api.notificationCount.mockResolvedValue(0);
    api.me.mockResolvedValue({ id: 'user-1' });
    Element.prototype.scrollIntoView = vi.fn();
  });

  it('renders the workspace-home rail: scope-labelled, canonical §2 section order plus Settings', async () => {
    window.history.pushState({}, '', '/workspaces/payments');
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="nav-rail"]')).toBeTruthy();
    }, { timeout: 5000 });
    expect(container.querySelector('[data-testid="nav-rail"]').getAttribute('data-rail-mode')).toBe('workspace-home');
    // Scope label: workspace name at workspace home.
    expect(container.querySelector('[data-testid="nav-rail-scope"]').getAttribute('title')).toBe('Payments');
    const ids = Array.from(container.querySelectorAll('.nav-rail-item')).map(
      (el) => el.getAttribute('data-testid').replace('nav-rail-item-', '')
    );
    expect(ids).toEqual(['decisions', 'specs', 'architecture', 'repos', 'briefing', 'agent-rules', 'settings']);
    // Collapsed by default on first use.
    expect(container.querySelector('[data-testid="nav-rail"]').classList.contains('expanded')).toBe(false);
  });

  it('renders resolved i18n labels in the expanded rail and the mobile drawer (no raw keys)', async () => {
    // F7 regression guard: the Settings item label must be the resolved
    // translation ("Settings"), never the raw fallback key
    // ("common.settings") — svelte-i18n silently falls back to the key
    // when it is missing from en.json, which id/order assertions cannot
    // catch. Component-level tests pass hand-written labels, so this must
    // be asserted against the real App label functions.
    window.history.pushState({}, '', '/workspaces/payments');
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="nav-rail-toggle"]')).toBeTruthy();
    }, { timeout: 5000 });
    await fireEvent.click(container.querySelector('[data-testid="nav-rail-toggle"]'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="nav-rail"]').classList.contains('expanded')).toBe(true);
    });
    const labels = Array.from(container.querySelectorAll('.nav-rail-item .nav-rail-label')).map(
      (el) => el.textContent
    );
    expect(labels).toEqual(['Decisions', 'Specs', 'Architecture', 'Repos', 'Briefing', 'Agent Rules', 'Settings']);
    // No raw i18n key may surface as user-visible label text.
    expect(labels.some((l) => l.includes('common.') || l.includes('topbar.') || l.includes('repo_mode.'))).toBe(false);

    // The narrow-viewport drawer always shows labels (no collapsed
    // icon-only form) — assert the Settings label there too.
    await fireEvent.click(container.querySelector('[data-testid="hamburger-btn"]'));
    const drawer = container.querySelector('[data-testid="mobile-drawer"]');
    expect(drawer).toBeTruthy();
    expect(drawer.querySelector('[data-testid="drawer-item-settings"]').textContent).toBe('Settings');
  });

  it('selecting a workspace-home rail item scrolls to the section (Architecture expands it)', async () => {
    window.history.pushState({}, '', '/workspaces/payments');
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="nav-rail-item-architecture"]')).toBeTruthy();
    }, { timeout: 5000 });
    await fireEvent.click(container.querySelector('[data-testid="nav-rail-item-architecture"]'));
    await waitFor(() => {
      expect(Element.prototype.scrollIntoView).toHaveBeenCalled();
    }, { timeout: 3000 });
    // The active rail item follows the selection.
    await waitFor(() => {
      expect(container.querySelector('[data-testid="nav-rail-item-architecture"]').getAttribute('aria-current')).toBe('page');
    });
  });

  it('selecting Agent Rules scrolls to the section on the workspace home and highlights it (§1a)', async () => {
    // §1a: section items scroll to their dashboard section — Settings is
    // the only navigation exception. At the review candidate this item
    // escaped to the agent-rules sub-page, so it could never show its
    // active highlight on the workspace home.
    window.history.pushState({}, '', '/workspaces/payments');
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="nav-rail-item-agent-rules"]')).toBeTruthy();
    }, { timeout: 5000 });
    // Entering the workspace home already scrolls to the default section
    // (App.svelte 'inbox' → section-decisions), so the assertion must pin
    // the actual scroll target of THIS click, not just that a scroll
    // happened.
    Element.prototype.scrollIntoView.mockClear();
    await fireEvent.click(container.querySelector('[data-testid="nav-rail-item-agent-rules"]'));
    await waitFor(() => {
      expect(Element.prototype.scrollIntoView).toHaveBeenCalled();
    }, { timeout: 3000 });
    const targets = Element.prototype.scrollIntoView.mock.contexts;
    expect(targets.some((el) => el?.getAttribute?.('data-testid') === 'section-agent-rules')).toBe(true);
    // Stays on the workspace home — no escape to the agent-rules sub-page
    // (where the rail would not render at all).
    expect(window.location.pathname).toBe('/workspaces/payments');
    // The active rail item follows the selection.
    await waitFor(() => {
      expect(container.querySelector('[data-testid="nav-rail-item-agent-rules"]').getAttribute('aria-current')).toBe('page');
    });
  });

  it('selecting Settings from the workspace-home rail navigates to workspace settings', async () => {
    window.history.pushState({}, '', '/workspaces/payments');
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="nav-rail-item-settings"]')).toBeTruthy();
    }, { timeout: 5000 });
    await fireEvent.click(container.querySelector('[data-testid="nav-rail-item-settings"]'));
    await waitFor(() => {
      expect(window.location.pathname).toBe('/workspaces/payments/settings');
    }, { timeout: 3000 });
    // Settings is a focused sub-page: the rail is not rendered there.
    await waitFor(() => {
      expect(container.querySelector('[data-testid="nav-rail"]')).toBeNull();
    });
  });

  it('renders the repo-mode rail mirroring the repo tab strip exactly; selecting an item switches tabs', async () => {
    window.history.pushState({}, '', '/workspaces/payments/r/core/code');
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="nav-rail"]')).toBeTruthy();
    }, { timeout: 5000 });
    const rail = container.querySelector('[data-testid="nav-rail"]');
    expect(rail.getAttribute('data-rail-mode')).toBe('repo');
    // Scope label: workspace / repo path in repo mode.
    expect(container.querySelector('[data-testid="nav-rail-scope"]').getAttribute('title')).toBe('Payments / core');
    const ids = Array.from(container.querySelectorAll('.nav-rail-item')).map(
      (el) => el.getAttribute('data-testid').replace('nav-rail-item-', '')
    );
    // RepoMode TABS order — the tab strip is the source of truth. The
    // expectation is derived from the same shared list (lib/repoTabs.js)
    // RepoMode renders its tab bar from, so a tab added/removed/reordered
    // there automatically updates this assertion; a hand-maintained copy in
    // App.svelte would fail against it (mirrored-logic guard).
    expect(ids).toEqual(REPO_TAB_IDS);
    // Active item is the current tab (Code).
    expect(container.querySelector('[data-testid="nav-rail-item-code"]').getAttribute('aria-current')).toBe('page');
    // Selecting a rail item activates the corresponding tab.
    await fireEvent.click(container.querySelector('[data-testid="nav-rail-item-decisions"]'));
    await waitFor(() => {
      expect(window.location.pathname).toBe('/workspaces/payments/r/core/decisions');
    }, { timeout: 3000 });
  });

  it('never renders on focused surfaces: entity detail in repo mode, and sub-pages', async () => {
    // Full-page entity detail (popped-out detail panel surface): no rail.
    window.history.pushState({}, '', '/workspaces/payments/r/core/specs/retry-logic.md');
    const { container } = render(App);
    await waitFor(() => {
      expect(document.title).toBe('core — Payments | Gyre');
    }, { timeout: 5000 });
    await new Promise((r) => setTimeout(r, 100));
    expect(container.querySelector('[data-testid="nav-rail"]')).toBeNull();
  });

  it('renders no rail when no workspace is selected (nothing to scope it to)', async () => {
    api.workspaces.mockResolvedValue([]);
    window.history.pushState({}, '', '/');
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="topbar"]')).toBeTruthy();
    }, { timeout: 5000 });
    await new Promise((r) => setTimeout(r, 100));
    expect(container.querySelector('[data-testid="nav-rail"]')).toBeNull();
  });

  it('rail expansion preference is persisted per mode (repo key differs from workspace-home key)', async () => {
    window.history.pushState({}, '', '/workspaces/payments/r/core/specs');
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="nav-rail"]')).toBeTruthy();
    }, { timeout: 5000 });
    await fireEvent.click(container.querySelector('[data-testid="nav-rail-toggle"]'));
    expect(localStorage.getItem('gyre_nav_rail_expanded.repo.user-1')).toBe('expanded');
    expect(localStorage.getItem('gyre_nav_rail_expanded.workspace-home.user-1')).toBeNull();
  });
});
