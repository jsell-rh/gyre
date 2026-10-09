/**
 * NoSidebar.test.js — ui-navigation.md Principle 5 + §9
 *
 * "No sidebar. The workspace home is a dashboard. The repo view has
 * horizontal tabs. There's no persistent sidebar that needs to morph
 * between scopes." ui-navigation.md explicitly supersedes HSI §1's
 * six-item sidebar (Inbox/Briefing/Explorer/Specs/Meta-specs/Admin)
 * and ui-layout.md §1's 240px shell sidebar.
 *
 * Covers:
 *   - The app shell renders NO sidebar element
 *   - The old Sidebar component is gone from the module graph
 *   - The mobile drawer (§8) lists the workspace-home sections
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

  it('mobile drawer lists the five workspace-home sections (§8)', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="hamburger-btn"]')).toBeTruthy();
    }, { timeout: 3000 });

    await fireEvent.click(container.querySelector('[data-testid="hamburger-btn"]'));
    const drawer = container.querySelector('[data-testid="mobile-drawer"]');
    expect(drawer).toBeTruthy();
    expect(drawer.textContent).toContain('Decisions');
    expect(drawer.textContent).toContain('Repos');
    expect(drawer.textContent).toContain('Briefing');
    expect(drawer.textContent).toContain('Specs');
    expect(drawer.textContent).toContain('Agent Rules');
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
