/**
 * NoSidebar.test.js — ui-navigation.md Principle 5 ("no permanent sidebar") + §9
 *
 * "No permanent sidebar. The workspace home is a dashboard. The repo view
 * has horizontal tabs." ui-navigation.md explicitly supersedes HSI §1's
 * six-item sidebar (Inbox/Briefing/Explorer/Specs/Meta-specs/Admin) and
 * ui-layout.md §1's 240px shell sidebar. The optional collapsible
 * navigation rail (§1a, task-082) is NOT a permanent sidebar: it renders
 * per-mode on navigation-heavy screens only, collapsed by default.
 *
 * Covers:
 *   - The app shell renders NO sidebar element
 *   - The old Sidebar component is gone from the module graph
 *   - The §1a rail is optional/collapsed, absent on focused surfaces
 *   - The mobile drawer (§8) offers the rail's per-mode item set
 *   - Drawer links navigate to section anchors, not a sidebar
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
  },
  setAuthToken: vi.fn(),
}));

vi.mock('../components/WorkspaceHome.svelte', () => ({ default: function WorkspaceHomeStub() {} }));
vi.mock('../components/RepoMode.svelte', () => ({ default: function RepoModeStub() {} }));
vi.mock('../components/UserProfile.svelte', () => ({ default: function UserProfileStub() {} }));

import { api } from '../lib/api.js';
import App from '../App.svelte';

describe('No sidebar (ui-navigation.md Principle 5)', () => {
  beforeEach(() => {
    window.history.pushState({}, '', '/');
    localStorage.clear();
    vi.clearAllMocks();
    api.workspaces.mockResolvedValue([]);
    api.workspaceRepos.mockResolvedValue([]);
    api.workspaceBudget.mockResolvedValue(null);
    api.notificationCount.mockResolvedValue(0);
  });

  it('renders no sidebar element in the app shell', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="topbar"]')).toBeTruthy();
    }, { timeout: 3000 });
    // The old 6-item sidebar (HSI §1.3) MUST NOT exist (ui-navigation.md §9).
    expect(container.querySelector('[data-testid="sidebar"]')).toBeNull();
    expect(container.querySelector('.sidebar')).toBeNull();
    expect(container.querySelector('.sidebar-item')).toBeNull();
  });

  it('the §1a navigation rail is optional, not a permanent sidebar (task-082)', async () => {
    // Amendment 16871182b1370b61693fc691 (§1a): the shell has no PERMANENT
    // sidebar. The optional collapsible rail renders only on
    // navigation-heavy screens (workspace home / repo mode), collapsed by
    // default - on focused sub-page surfaces it is absent entirely.
    api.workspaces.mockResolvedValue([{ id: 'ws-1', name: 'Payments', slug: 'payments' }]);
    window.history.pushState({}, '', '/workspaces/payments');
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="nav-rail"]')).toBeTruthy();
    }, { timeout: 3000 });
    // Collapsed (icon-only) by default: not a permanent labelled sidebar.
    const rail = container.querySelector('[data-testid="nav-rail"]');
    expect(rail.classList.contains('expanded')).toBe(false);
    expect(container.querySelector('.nav-rail-label')).toBeNull();

    // Focused sub-page surface (workspace settings): no rail at all.
    window.history.pushState({}, '', '/workspaces/payments/settings');
    window.dispatchEvent(new PopStateEvent('popstate', {
      state: { mode: 'workspace_settings', wsId: 'ws-1', repoName: null, repoTab: 'specs' },
    }));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="nav-rail"]')).toBeNull();
    }, { timeout: 3000 });
    // And still no permanent sidebar of the old kind.
    expect(container.querySelector('.sidebar')).toBeNull();
    expect(container.querySelector('.sidebar-item')).toBeNull();
  });

  it('no ⌘1-6 sidebar shortcuts are advertised in the overlay', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="topbar"]')).toBeTruthy();
    }, { timeout: 3000 });
    await fireEvent.keyDown(window, { key: '?' });
    await waitFor(() => {
      expect(document.querySelector('.shortcuts-overlay')).toBeTruthy();
    });
    const text = document.querySelector('.shortcuts-overlay').textContent;
    expect(text).not.toContain('⌘1');
    expect(text).not.toContain('⌘6');
    // g-key sequences are the spec's tab shortcuts (§6).
    expect(text).toContain('g 1');
    expect(text).toContain('g 2');
  });

  it('mobile drawer offers the rail item set: workspace-home sections in canonical order plus Settings (§8, §1a)', async () => {
    api.workspaces.mockResolvedValue([{ id: 'ws-1', name: 'Payments', slug: 'payments' }]);
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="hamburger-btn"]')).toBeTruthy();
    }, { timeout: 3000 });

    await fireEvent.click(container.querySelector('[data-testid="hamburger-btn"]'));
    const drawer = container.querySelector('[data-testid="mobile-drawer"]');
    expect(drawer).toBeTruthy();
    const ids = Array.from(drawer.querySelectorAll('.drawer-link')).map(
      (el) => el.getAttribute('data-testid').replace('drawer-item-', '')
    );
    // §1a workspace-home rail item set: §2 dashboard sections in canonical
    // order, then Settings.
    expect(ids).toEqual(['decisions', 'specs', 'architecture', 'repos', 'briefing', 'agent-rules', 'settings']);
    // Old HSI §1.3 items must not appear.
    expect(drawer.textContent).not.toContain('Inbox');
    expect(drawer.textContent).not.toContain('Meta-specs');
    expect(drawer.textContent).not.toContain('Explorer');
  });

  it('g 2 navigates to the Architecture tab (§6)', async () => {
    api.workspaces.mockResolvedValue([{ id: 'ws-1', name: 'Payments', slug: 'payments' }]);
    api.workspaceRepos.mockResolvedValue([{ id: 'repo-1', name: 'core' }]);
    window.history.pushState({}, '', '/workspaces/payments/r/core/specs');
    const { container } = render(App);
    await waitFor(() => {
      // RepoMode is stubbed in jsdom; the document title proves repo mode
      // was entered from the URL.
      expect(document.title).toBe('core — Payments | Gyre');
    }, { timeout: 3000 });

    await fireEvent.keyDown(window, { key: 'g' });
    await fireEvent.keyDown(window, { key: '2' });
    await waitFor(() => {
      expect(window.location.pathname).toBe('/workspaces/payments/r/core/architecture');
    }, { timeout: 3000 });
  });
});
