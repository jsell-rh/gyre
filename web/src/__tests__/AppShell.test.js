/**
 * AppShell.test.js — Tests for ui-navigation.md §1 (App Shell), §7 (URL structure),
 *   §6 (Keyboard Shortcuts), §8 (Responsive Design), §10 (Cross-Workspace View)
 *
 * Covers:
 *   - URL routing: parseUrl() maps paths to mode + slug + repoName + tab
 *   - urlFor(): generates canonical URLs
 *   - Entrypoint flow
 *   - Workspace selector (topbar)
 *   - Repo mode: back arrow + breadcrumb
 *   - WorkspaceHome sections visible
 *   - Keyboard shortcuts (g h, g 1-4, Esc, ?, ⌘K)
 *   - Status bar
 *   - Cross-workspace view (/all route, parseUrl, urlFor)
 *   - Mobile hamburger button presence
 *   - Legacy URL redirect logic
 *   - "All Workspaces" in workspace dropdown
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

// ── Pure function tests (parseUrl + urlFor replicated from App.svelte) ──
// The tab list is NOT replicated: it is derived from lib/repoTabs.js, the
// same single source of truth App.svelte and RepoMode.svelte use, so the
// replica cannot drift from the real parser's tab set.
import { REPO_TAB_IDS } from '../lib/repoTabs.js';

const REPO_TABS = REPO_TAB_IDS;

function parseUrl(pathname) {
  const raw = pathname.split('/').filter(Boolean).map(p => {
    try { return decodeURIComponent(p); } catch { return p; }
  });

  if (raw.length === 0) return { mode: 'workspace_home', slug: null, repoName: null, tab: null };

  if (raw.length === 1 && raw[0] === 'profile') {
    return { mode: 'profile', slug: null, repoName: null, tab: null };
  }

  // /@{username} — tenant-scoped public user profile
  // (user-management.md §User Profile Page)
  if (raw[0]?.startsWith('@') && raw[0].length > 1) {
    return { mode: 'user_profile', slug: null, repoName: null, tab: null, username: raw[0].slice(1) };
  }

  // /invite/:workspaceId OR /invite/:token — pending invitation accept
  // page (§UI Pages). A 64-char lowercase-hex segment is a task-110
  // magic-link token; anything else is the authenticated flow's
  // workspace id (a UUID).
  if (raw[0] === 'invite' && raw.length === 2) {
    const isToken = /^[0-9a-f]{64}$/.test(raw[1]);
    return {
      mode: 'invite_accept', slug: null, repoName: null, tab: null,
      inviteId: isToken ? null : raw[1],
      inviteToken: isToken ? raw[1] : null,
    };
  }

  // /all  or  /all/settings  (cross-workspace view §10)
  if (raw[0] === 'all') {
    const tab = raw[1] ?? null;
    return { mode: 'cross_workspace', slug: 'all', repoName: null, tab };
  }

  if (raw[0] === 'workspaces' && raw.length >= 2) {
    const slug = raw[1];

    if (raw[2] === 'r' && raw.length >= 4) {
      const repoName = raw[3];
      const tab = raw[4] && REPO_TABS.includes(raw[4]) ? raw[4] : 'specs';
      return { mode: 'repo', slug, repoName, tab };
    }

    return { mode: 'workspace_home', slug, repoName: null, tab: null };
  }

  return null;
}

function urlFor(parsed) {
  if (!parsed) return '/';
  const { mode: m, slug, repoName, tab, username, inviteId, inviteToken } = parsed;
  if (m === 'profile') return '/profile';
  if (m === 'user_profile') return `/@${encodeURIComponent(username ?? '')}`;
  if (m === 'invite_accept') {
    const seg = inviteToken ?? inviteId ?? '';
    return `/invite/${encodeURIComponent(seg)}`;
  }
  if (m === 'cross_workspace') {
    return tab ? `/all/${encodeURIComponent(tab)}` : '/all';
  }
  if (!slug) return '/';
  if (m === 'workspace_home') return `/workspaces/${encodeURIComponent(slug)}`;
  if (m === 'repo') {
    const base = `/workspaces/${encodeURIComponent(slug)}/r/${encodeURIComponent(repoName)}`;
    if (tab && tab !== 'specs') return `${base}/${tab}`;
    return base;
  }
  return '/';
}

// ── URL routing tests ─────────────────────────────────────────────────

describe('parseUrl', () => {
  it('returns workspace_home (no slug) for root path', () => {
    expect(parseUrl('/')).toEqual({ mode: 'workspace_home', slug: null, repoName: null, tab: null });
  });

  it('returns workspace_home (no slug) for empty path', () => {
    expect(parseUrl('')).toEqual({ mode: 'workspace_home', slug: null, repoName: null, tab: null });
  });

  it('parses profile route', () => {
    expect(parseUrl('/profile')).toEqual({ mode: 'profile', slug: null, repoName: null, tab: null });
  });

  it('parses /@username as the public profile route', () => {
    expect(parseUrl('/@jsell')).toEqual({ mode: 'user_profile', slug: null, repoName: null, tab: null, username: 'jsell' });
  });

  it('decodes URL-encoded handles in /@username', () => {
    expect(parseUrl('/@j%C3%B8rgen')).toEqual({ mode: 'user_profile', slug: null, repoName: null, tab: null, username: 'jørgen' });
  });

  it('does not treat a bare @ or non-@ path as a profile', () => {
    expect(parseUrl('/@')).toBeNull();
    expect(parseUrl('/dashboard')).toBeNull();
  });

  it('parses workspace home', () => {
    expect(parseUrl('/workspaces/payments')).toEqual({
      mode: 'workspace_home', slug: 'payments', repoName: null, tab: null,
    });
  });

  it('parses workspace home with trailing settings path', () => {
    expect(parseUrl('/workspaces/payments/settings')).toEqual({
      mode: 'workspace_home', slug: 'payments', repoName: null, tab: null,
    });
  });

  it('parses workspace home with agent-rules path', () => {
    expect(parseUrl('/workspaces/payments/agent-rules')).toEqual({
      mode: 'workspace_home', slug: 'payments', repoName: null, tab: null,
    });
  });

  it('parses repo mode (default specs tab)', () => {
    expect(parseUrl('/workspaces/payments/r/payment-api')).toEqual({
      mode: 'repo', slug: 'payments', repoName: 'payment-api', tab: 'specs',
    });
  });

  it('parses repo mode with specs tab', () => {
    expect(parseUrl('/workspaces/payments/r/payment-api/specs')).toEqual({
      mode: 'repo', slug: 'payments', repoName: 'payment-api', tab: 'specs',
    });
  });

  it('parses repo mode with architecture tab', () => {
    expect(parseUrl('/workspaces/payments/r/payment-api/architecture')).toEqual({
      mode: 'repo', slug: 'payments', repoName: 'payment-api', tab: 'architecture',
    });
  });

  it('parses repo mode with decisions tab', () => {
    expect(parseUrl('/workspaces/payments/r/payment-api/decisions')).toEqual({
      mode: 'repo', slug: 'payments', repoName: 'payment-api', tab: 'decisions',
    });
  });

  it('parses repo mode with code tab', () => {
    expect(parseUrl('/workspaces/payments/r/payment-api/code')).toEqual({
      mode: 'repo', slug: 'payments', repoName: 'payment-api', tab: 'code',
    });
  });

  it('parses repo mode with settings tab', () => {
    expect(parseUrl('/workspaces/payments/r/payment-api/settings')).toEqual({
      mode: 'repo', slug: 'payments', repoName: 'payment-api', tab: 'settings',
    });
  });

  it('parses repo mode with dependencies tab (drift regression)', () => {
    // The App.svelte route-parse list once drifted from the tab strip and
    // lost 'dependencies', misparsing this deep link to the Specs tab. The
    // tab set is now derived from lib/repoTabs.js, so every tab on the
    // strip — including any added later — must parse.
    expect(parseUrl('/workspaces/payments/r/payment-api/dependencies')).toEqual({
      mode: 'repo', slug: 'payments', repoName: 'payment-api', tab: 'dependencies',
    });
  });

  it('parses every tab on the repo tab strip', () => {
    // Completeness guard: the parser accepts exactly the strip's tab set.
    for (const id of REPO_TAB_IDS) {
      expect(parseUrl(`/workspaces/payments/r/payment-api/${id}`)).toEqual({
        mode: 'repo', slug: 'payments', repoName: 'payment-api', tab: id,
      });
    }
  });

  it('falls back to specs for unknown repo tab', () => {
    expect(parseUrl('/workspaces/payments/r/payment-api/unknown')).toEqual({
      mode: 'repo', slug: 'payments', repoName: 'payment-api', tab: 'specs',
    });
  });

  it('returns null for unknown top-level path', () => {
    expect(parseUrl('/dashboard')).toBeNull();
  });

  it('returns null for /inbox (old nav route)', () => {
    expect(parseUrl('/inbox')).toBeNull();
  });

  it('handles URL-encoded workspace slug', () => {
    expect(parseUrl('/workspaces/my%20workspace')).toEqual({
      mode: 'workspace_home', slug: 'my workspace', repoName: null, tab: null,
    });
  });

  // §10 Cross-workspace URL parsing
  it('parses /all as cross_workspace with no tab', () => {
    expect(parseUrl('/all')).toEqual({
      mode: 'cross_workspace', slug: 'all', repoName: null, tab: null,
    });
  });

  it('parses /all/settings as cross_workspace with settings tab', () => {
    expect(parseUrl('/all/settings')).toEqual({
      mode: 'cross_workspace', slug: 'all', repoName: null, tab: 'settings',
    });
  });

  it('parses /all/agent-rules as cross_workspace with agent-rules tab', () => {
    expect(parseUrl('/all/agent-rules')).toEqual({
      mode: 'cross_workspace', slug: 'all', repoName: null, tab: 'agent-rules',
    });
  });
});

describe('urlFor', () => {
  it('returns / for null', () => {
    expect(urlFor(null)).toBe('/');
  });

  it('returns /profile', () => {
    expect(urlFor({ mode: 'profile', slug: null, repoName: null, tab: null })).toBe('/profile');
  });

  it('parses /@username as user_profile', () => {
    expect(parseUrl('/@asmith')).toEqual({
      mode: 'user_profile', slug: null, repoName: null, tab: null, username: 'asmith',
    });
  });

  it('parses /invite/:id as invite_accept', () => {
    expect(parseUrl('/invite/ws-9')).toEqual({
      mode: 'invite_accept', slug: null, repoName: null, tab: null, inviteId: 'ws-9', inviteToken: null,
    });
    // A UUID workspace id is never mistaken for a token.
    expect(parseUrl('/invite/3f8a2b1e-9c4d-4e2a-8f1b-0d5c6e7a8b9c')).toEqual({
      mode: 'invite_accept', slug: null, repoName: null, tab: null,
      inviteId: '3f8a2b1e-9c4d-4e2a-8f1b-0d5c6e7a8b9c', inviteToken: null,
    });
  });

  it('parses /invite/:token (64-hex magic link) as invite_accept with the token, not a workspace id', () => {
    const token = 'a'.repeat(64);
    expect(parseUrl(`/invite/${token}`)).toEqual({
      mode: 'invite_accept', slug: null, repoName: null, tab: null, inviteId: null, inviteToken: token,
    });
    // Uppercase hex is not a minted token (tokens are lowercase hex);
    // it falls through to the workspace-id shape.
    expect(parseUrl('/invite/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA')).toEqual({
      mode: 'invite_accept', slug: null, repoName: null, tab: null,
      inviteId: 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA', inviteToken: null,
    });
  });

  it('generates /@username and /invite/:id URLs', () => {
    expect(urlFor({ mode: 'user_profile', username: 'asmith' })).toBe('/@asmith');
    expect(urlFor({ mode: 'invite_accept', inviteId: 'ws-9' })).toBe('/invite/ws-9');
    // Token form takes precedence in the canonical URL.
    const token = 'b'.repeat(64);
    expect(urlFor({ mode: 'invite_accept', inviteId: null, inviteToken: token })).toBe(`/invite/${token}`);
  });

  it('round-trips a magic-link /invite/:token URL through parseUrl → urlFor byte-for-byte', () => {
    const token = 'c0ffee'.padEnd(64, '0');
    const parsed = parseUrl(`/invite/${token}`);
    expect(urlFor(parsed)).toBe(`/invite/${token}`);
  });

  it('URL-encodes handles in /@username', () => {
    expect(urlFor({ mode: 'user_profile', slug: null, repoName: null, tab: null, username: 'jørgen' })).toBe('/@j%C3%B8rgen');
  });

  it('round-trips /@username through parseUrl → urlFor', () => {
    const parsed = parseUrl('/@jsell');
    expect(urlFor(parsed)).toBe('/@jsell');
  });

  it('returns / when no slug', () => {
    expect(urlFor({ mode: 'workspace_home', slug: null, repoName: null, tab: null })).toBe('/');
  });

  it('generates workspace home URL', () => {
    expect(urlFor({ mode: 'workspace_home', slug: 'payments', repoName: null, tab: null }))
      .toBe('/workspaces/payments');
  });

  it('generates repo mode URL (default specs tab — omits /specs)', () => {
    expect(urlFor({ mode: 'repo', slug: 'payments', repoName: 'payment-api', tab: 'specs' }))
      .toBe('/workspaces/payments/r/payment-api');
  });

  it('generates repo mode URL with architecture tab', () => {
    expect(urlFor({ mode: 'repo', slug: 'payments', repoName: 'payment-api', tab: 'architecture' }))
      .toBe('/workspaces/payments/r/payment-api/architecture');
  });

  it('generates repo mode URL with decisions tab', () => {
    expect(urlFor({ mode: 'repo', slug: 'payments', repoName: 'payment-api', tab: 'decisions' }))
      .toBe('/workspaces/payments/r/payment-api/decisions');
  });

  it('URL-encodes workspace slug with spaces', () => {
    expect(urlFor({ mode: 'workspace_home', slug: 'my workspace', repoName: null, tab: null }))
      .toBe('/workspaces/my%20workspace');
  });

  it('round-trips workspace home through parseUrl', () => {
    const parsed = { mode: 'workspace_home', slug: 'payments', repoName: null, tab: null };
    expect(parseUrl(urlFor(parsed))).toEqual(parsed);
  });

  it('round-trips repo specs tab through parseUrl', () => {
    const parsed = { mode: 'repo', slug: 'payments', repoName: 'payment-api', tab: 'specs' };
    expect(parseUrl(urlFor(parsed))).toEqual(parsed);
  });

  // §10 cross_workspace urlFor
  it('generates /all for cross_workspace with no tab', () => {
    expect(urlFor({ mode: 'cross_workspace', slug: 'all', repoName: null, tab: null })).toBe('/all');
  });

  it('generates /all/settings for cross_workspace settings tab', () => {
    expect(urlFor({ mode: 'cross_workspace', slug: 'all', repoName: null, tab: 'settings' })).toBe('/all/settings');
  });

  it('round-trips /all through parseUrl → urlFor', () => {
    const parsed = { mode: 'cross_workspace', slug: 'all', repoName: null, tab: null };
    expect(parseUrl(urlFor(parsed))).toEqual(parsed);
  });

  it('round-trips /all/settings through parseUrl → urlFor', () => {
    const parsed = { mode: 'cross_workspace', slug: 'all', repoName: null, tab: 'settings' };
    expect(parseUrl(urlFor(parsed))).toEqual(parsed);
  });

  it('round-trips repo architecture tab through parseUrl', () => {
    const parsed = { mode: 'repo', slug: 'payments', repoName: 'payment-api', tab: 'architecture' };
    expect(parseUrl(urlFor(parsed))).toEqual(parsed);
  });
});

// ── Component tests ───────────────────────────────────────────────────

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
    unreadNotificationCount: vi.fn().mockResolvedValue(0),
    notifications: vi.fn().mockResolvedValue({ notifications: [] }),
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

// ── App shell — topbar-first, no sidebar (ui-navigation.md §1) ─────────

describe('App shell — topbar first, no sidebar (ui-navigation.md §1)', () => {
  beforeEach(() => {
    window.history.pushState({}, '', '/');
    localStorage.clear();
    vi.clearAllMocks();
    api.workspaces.mockResolvedValue([]);
    api.workspaceRepos.mockResolvedValue([]);
    api.workspaceBudget.mockResolvedValue(null);
    api.notificationCount.mockResolvedValue(0);
    api.unreadNotificationCount.mockResolvedValue(0);
    api.notifications.mockResolvedValue({ notifications: [] });
  });

  it('renders topbar without a sidebar', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="topbar"]')).toBeTruthy();
      expect(container.querySelector('[data-testid="sidebar"]')).toBeNull();
    }, { timeout: 3000 });
  });

});

// ── Notification bell (user-management.md §In-App Notification UI, ──────
//    amended ui-navigation.md §1: the bell is the single top-bar
//    notification surface — it shows the unread count and opens the
//    notification drawer)

describe('Notification bell (user-management.md §In-App Notification UI)', () => {
  // The bell's unread count rides lib/notifications.svelte.js
  // (refreshUnreadCount → raw fetch of GET /notifications/unread-count),
  // not the api.js mock — mock the endpoint at the fetch level.
  function mockUnreadCountFetch(counts) {
    let call = 0;
    global.fetch = vi.fn(() => {
      const count = counts[Math.min(call, counts.length - 1)];
      call += 1;
      return Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve({ count }) });
    });
  }

  beforeEach(() => {
    window.history.pushState({}, '', '/');
    localStorage.clear();
    vi.clearAllMocks();
    api.workspaces.mockResolvedValue([]);
    api.workspaceRepos.mockResolvedValue([]);
    api.workspaceBudget.mockResolvedValue(null);
    api.notificationCount.mockResolvedValue(0);
    api.unreadNotificationCount.mockResolvedValue(0);
    api.notifications.mockResolvedValue({ notifications: [] });
    api.version.mockResolvedValue({ version: '0.1.0' });
  });

  it('badge shows the unread count from GET /notifications/unread-count, not the decisions count', async () => {
    // The bell badge is the unread notification count (spec: "Notification
    // bell in top nav with unread count badge"). decisionsCount (legacy
    // /users/me/notifications/count) must not drive the badge.
    api.notificationCount.mockResolvedValue(7);
    mockUnreadCountFetch([3]);
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="notification-bell"]')).toBeTruthy();
    }, { timeout: 3000 });
    await waitFor(() => {
      const badge = container.querySelector('[data-testid="notification-bell"] [data-testid="notification-badge"]');
      expect(badge).toBeTruthy();
      expect(badge.textContent).toBe('3');
    }, { timeout: 3000 });
    // The count came from the unread-count endpoint (raw fetch), not from
    // the decisions-count endpoint (api.notificationCount).
    const fetchedUrls = global.fetch.mock.calls.map(c => String(c[0]));
    expect(fetchedUrls.some(u => u.includes('/notifications/unread-count'))).toBe(true);
  });

  it('clicking the bell opens the notification drawer', async () => {
    mockUnreadCountFetch([2]);
    api.notifications.mockResolvedValue({
      notifications: [
        { id: 'n-1', title: 'Gate failure in payment-api', priority_level: 'High', read: false, created_at: 1700000000 },
      ],
    });
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="notification-bell"]')).toBeTruthy();
    }, { timeout: 3000 });
    await fireEvent.click(container.querySelector('[data-testid="notification-bell"]'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="notification-drawer"]')).toBeTruthy();
    }, { timeout: 3000 });
    // The drawer lists active-only notifications (?active=true).
    expect(api.notifications).toHaveBeenCalledWith({ limit: 50, active: true });
  });

  it('refreshes the unread badge when the drawer closes', async () => {
    // Read-state changes made inside the drawer (mark read / mark all read)
    // must be reflected on the bell badge when it closes.
    mockUnreadCountFetch([2, 0]);
    api.notifications.mockResolvedValue({ notifications: [] });
    const { container } = render(App);
    await waitFor(() => {
      const badge = container.querySelector('[data-testid="notification-bell"] [data-testid="notification-badge"]');
      expect(badge).toBeTruthy();
      expect(badge.textContent).toBe('2');
    }, { timeout: 3000 });
    await fireEvent.click(container.querySelector('[data-testid="notification-bell"]'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="notification-drawer"]')).toBeTruthy();
    }, { timeout: 3000 });
    // Close via the drawer's close button.
    await fireEvent.click(container.querySelector('[data-testid="notification-drawer"] .close-btn'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="notification-bell"] [data-testid="notification-badge"]')).toBeNull();
    }, { timeout: 3000 });
    // The badge was re-read from the server after the close.
    const unreadCalls = global.fetch.mock.calls
      .map(c => String(c[0]))
      .filter(u => u.includes('/notifications/unread-count'));
    expect(unreadCalls.length).toBeGreaterThanOrEqual(2);
  });
});

// ── Mobile drawer section navigation (ui-navigation.md §8) ──────────────
// The drawer is the only navigation chrome besides the topbar; its links
// are the workspace-home sections and must preserve scope.

describe('Mobile drawer section navigation (ui-navigation.md §8)', () => {
  const WS = [{ id: 'ws-1', name: 'Payments', slug: 'payments' }];

  beforeEach(() => {
    window.history.pushState({}, '', '/');
    localStorage.clear();
    vi.clearAllMocks();
    api.workspaces.mockResolvedValue(WS);
    api.workspaceRepos.mockResolvedValue([{ id: 'repo-1', name: 'core' }]);
    api.workspaceBudget.mockResolvedValue(null);
    api.notificationCount.mockResolvedValue(0);
    api.version.mockResolvedValue({ version: '0.1.0' });
    Element.prototype.scrollIntoView = vi.fn();
  });

  function activeDrawerItem(container) {
    const active = container.querySelector('.drawer-link.active');
    return active?.getAttribute('data-testid')?.replace('drawer-item-', '') ?? null;
  }

  function openDrawer(container) {
    return waitFor(() => {
      expect(container.querySelector('[data-testid="hamburger-btn"]')).toBeTruthy();
    }, { timeout: 3000 }).then(() => {
      fireEvent.click(container.querySelector('[data-testid="hamburger-btn"]'));
    });
  }

  it('repo-mode drawer mirrors the tab strip exactly and switches tabs (§1a narrow viewports)', async () => {
    // ui-navigation.md §1a "Narrow viewports": the drawer offers the rail's
    // per-mode item set; in repo mode that set mirrors the repo tab strip
    // exactly (the tab strip is the source of truth - no 'briefing' item,
    // it is a sub-tab of Architecture, not a tab). Selecting a drawer item
    // activates the corresponding tab, same route as clicking the tab.
    window.history.pushState({}, '', '/workspaces/payments/r/core/specs');
    const { container } = render(App);
    // RepoMode is stubbed in jsdom; the document title proves repo mode.
    await waitFor(() => expect(document.title).toBe('core — Payments | Gyre'), { timeout: 3000 });
    await openDrawer(container);
    const drawer = container.querySelector('[data-testid="mobile-drawer"]');
    const itemIds = Array.from(drawer.querySelectorAll('.drawer-link')).map(
      (el) => el.getAttribute('data-testid').replace('drawer-item-', '')
    );
    // Canonical tab order — derived from the same shared list
    // (lib/repoTabs.js) RepoMode's tab strip and App's rail render from.
    expect(itemIds).toEqual(REPO_TAB_IDS);
    // Briefing is an Architecture sub-tab, never a drawer/tab item.
    expect(container.querySelector('[data-testid="drawer-item-briefing"]')).toBeNull();
    // Selecting Architecture activates the Architecture tab (same route as
    // clicking the tab).
    await fireEvent.click(container.querySelector('[data-testid="drawer-item-architecture"]'));
    await waitFor(() => expect(window.location.pathname).toBe('/workspaces/payments/r/core/architecture'), { timeout: 3000 });
  });


  it('clicking Agent Rules at workspace scope scrolls to the dashboard section, not the sub-page (§1a)', async () => {
    // ui-navigation.md §1a: section items scroll to their dashboard
    // section on the workspace home; Settings is the only navigation
    // exception. Agent Rules must not escape to the agent-rules sub-page
    // (WorkspaceHome is stubbed here, so the observable is the URL: the
    // workspace home is preserved; the scroll itself is covered by
    // NavRail.test.js against the real WorkspaceHome).
    window.history.pushState({}, '', '/workspaces/payments');
    const { container } = render(App);
    await openDrawer(container);
    await fireEvent.click(container.querySelector('[data-testid="drawer-item-agent-rules"]'));
    await new Promise(r => setTimeout(r, 100));
    expect(window.location.pathname).toBe('/workspaces/payments');
    expect(window.history.state?.mode).toBe('workspace_home');
  });

  it('repo mode drawer click on Specs switches to the Specs repo tab', async () => {
    window.history.pushState({}, '', '/workspaces/payments/r/core/code');
    const { container } = render(App);
    // RepoMode is stubbed in jsdom; the document title proves repo mode.
    await waitFor(() => expect(document.title).toBe('core — Payments | Gyre'), { timeout: 3000 });
    await openDrawer(container);
    await fireEvent.click(container.querySelector('[data-testid="drawer-item-specs"]'));
    // Specs is the default repo tab, so its canonical URL is the bare repo
    // URL (see the urlFor tests). The history entry records the active tab —
    // this is the observable that the tab actually switched from Code.
    await waitFor(() => expect(window.location.pathname).toBe('/workspaces/payments/r/core'), { timeout: 3000 });
    expect(window.history.state?.repoTab).toBe('specs');
  });
});

// ── Tenant scope (/all) — scope preservation (ui-navigation.md §10) ────
// At tenant scope drawer clicks must preserve tenant scope: sections are
// scroll sections of the cross-workspace dashboard, Agent Rules is the
// tenant meta-spec catalog, settings is tenant settings (admin-only).
describe('Drawer tenant-scope navigation (ui-navigation.md §10)', () => {
  beforeEach(() => {
    window.history.pushState({}, '', '/');
    localStorage.clear();
    vi.clearAllMocks();
    api.workspaces.mockResolvedValue([]);
    api.workspaceRepos.mockResolvedValue([]);
    api.workspaceBudget.mockResolvedValue(null);
    api.notificationCount.mockResolvedValue(0);
    api.version.mockResolvedValue({ version: '0.1.0' });
    // jsdom lacks scrollIntoView; the cross-workspace sections render for
    // real, so the section scroll would throw.
    Element.prototype.scrollIntoView = vi.fn();
  });

  function openDrawer(container) {
    return waitFor(() => {
      expect(container.querySelector('[data-testid="hamburger-btn"]')).toBeTruthy();
    }, { timeout: 3000 }).then(() => {
      fireEvent.click(container.querySelector('[data-testid="hamburger-btn"]'));
    });
  }

  it('clicking Briefing at /all stays at /all', async () => {
    window.history.pushState({}, '', '/all');
    const { container } = render(App);
    await openDrawer(container);
    await fireEvent.click(container.querySelector('[data-testid="drawer-item-briefing"]'));
    await new Promise(r => setTimeout(r, 50));
    expect(window.location.pathname).toBe('/all');
  });

  it('clicking Specs at /all stays at /all', async () => {
    window.history.pushState({}, '', '/all');
    const { container } = render(App);
    await openDrawer(container);
    await fireEvent.click(container.querySelector('[data-testid="drawer-item-specs"]'));
    await new Promise(r => setTimeout(r, 50));
    expect(window.location.pathname).toBe('/all');
  });

  it('clicking Agent Rules at /all navigates to /all/agent-rules', async () => {
    window.history.pushState({}, '', '/all');
    const { container } = render(App);
    await openDrawer(container);
    await fireEvent.click(container.querySelector('[data-testid="drawer-item-agent-rules"]'));
    await waitFor(() => expect(window.location.pathname).toBe('/all/agent-rules'), { timeout: 3000 });
  });

  it('tenant settings are admin-only: drawer has no settings escape at /all', async () => {
    api.me.mockResolvedValue({ role: 'Member' });
    window.history.pushState({}, '', '/all');
    const { container } = render(App);
    // The drawer lists home sections only; settings lives behind the
    // admin-gated gear, not a drawer item (§10).
    await openDrawer(container);
    expect(container.querySelector('[data-testid="drawer-item-settings"]')).toBeNull();
    expect(window.location.pathname).toBe('/all');
  });
});


// ── Entrypoint flow ───────────────────────────────────────────────────

describe('Entrypoint flow', () => {
  beforeEach(() => {
    window.history.pushState({}, '', '/');
    localStorage.clear();
    vi.clearAllMocks();
    api.workspaces.mockResolvedValue([]);
    api.workspaceRepos.mockResolvedValue([]);
    api.workspaceBudget.mockResolvedValue(null);
    api.notificationCount.mockResolvedValue(0);
  });

  it('first visit: shows select-workspace prompt', async () => {
    localStorage.clear();
    api.workspaces.mockResolvedValue([]);
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="ws-select-prompt"]')).toBeTruthy();
    }, { timeout: 3000 });
  });

  it('subsequent visit (workspace found): shows workspace name', async () => {
    const mockWs = { id: 'ws-test-1', name: 'Test Workspace', trust_level: 'Guided' };
    localStorage.setItem('gyre_workspace_id', mockWs.id);
    api.workspaces.mockResolvedValue([mockWs]);
    api.workspaceBudget.mockResolvedValue(null);

    const { container } = render(App);
    await waitFor(() => {
      const wsBtn = container.querySelector('[data-testid="ws-name-btn"]');
      expect(wsBtn?.textContent?.trim()).toBe('Test Workspace');
    }, { timeout: 3000 });
  });

  it('subsequent visit (workspace NOT found): falls back to select-workspace prompt', async () => {
    localStorage.setItem('gyre_workspace_id', 'stale-id');
    api.workspaces.mockResolvedValue([]);

    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="ws-select-prompt"]')).toBeTruthy();
      expect(localStorage.getItem('gyre_workspace_id')).toBeNull();
    }, { timeout: 3000 });
  });
});

// ── Workspace selector ────────────────────────────────────────────────

describe('Workspace selector', () => {
  beforeEach(() => {
    window.history.pushState({}, '', '/');
    localStorage.clear();
    vi.clearAllMocks();
    api.workspaceRepos.mockResolvedValue([]);
    api.workspaceBudget.mockResolvedValue(null);
    api.notificationCount.mockResolvedValue(0);
  });

  it('shows workspace name when workspace is active', async () => {
    const mockWs = { id: 'ws-1', name: 'Payments', trust_level: 'Guided' };
    localStorage.setItem('gyre_workspace_id', mockWs.id);
    api.workspaces.mockResolvedValue([mockWs]);

    const { container } = render(App);
    await waitFor(() => {
      expect(container.textContent).toContain('Payments');
    }, { timeout: 3000 });
  });

  it('opens dropdown when arrow button clicked', async () => {
    const mockWs = { id: 'ws-1', name: 'Payments', trust_level: 'Guided' };
    localStorage.setItem('gyre_workspace_id', mockWs.id);
    api.workspaces.mockResolvedValue([mockWs]);

    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="ws-dropdown-toggle"]')).toBeTruthy();
    }, { timeout: 3000 });

    await fireEvent.click(container.querySelector('[data-testid="ws-dropdown-toggle"]'));

    await waitFor(() => {
      expect(container.querySelector('[data-testid="ws-dropdown"]')).toBeTruthy();
    });
  });
});

// ── Status bar ────────────────────────────────────────────────────────

describe('Status bar', () => {
  beforeEach(() => {
    window.history.pushState({}, '', '/');
    localStorage.clear();
    vi.clearAllMocks();
    api.workspaces.mockResolvedValue([]);
    api.workspaceRepos.mockResolvedValue([]);
    api.workspaceBudget.mockResolvedValue(null);
    api.notificationCount.mockResolvedValue(0);
  });

  it('renders status bar', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('.status-bar')).toBeTruthy();
    });
  });

  it('shows WebSocket status indicator', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('.status-ws')).toBeTruthy();
    });
  });

  it('shows trust level when workspace is active', async () => {
    const mockWs = { id: 'ws-1', name: 'W', trust_level: 'Guided' };
    localStorage.setItem('gyre_workspace_id', mockWs.id);
    api.workspaces.mockResolvedValue([mockWs]);
    api.workspaceBudget.mockResolvedValue(null);

    const { container } = render(App);
    await waitFor(() => {
      expect(container.textContent).toContain('Trust: Guided');
    }, { timeout: 3000 });
  });

  it('shows budget percentage when available', async () => {
    const mockWs = { id: 'ws-1', name: 'W', trust_level: 'Guided' };
    localStorage.setItem('gyre_workspace_id', mockWs.id);
    api.workspaces.mockResolvedValue([mockWs]);
    api.workspaceBudget.mockResolvedValue({ used_credits: 67, total_credits: 100 });

    const { container } = render(App);
    await waitFor(() => {
      expect(container.textContent).toContain('Budget: 67%');
    }, { timeout: 3000 });
  });
});

// ── Keyboard shortcuts ────────────────────────────────────────────────

describe('Keyboard shortcuts', () => {
  beforeEach(() => {
    window.history.pushState({}, '', '/');
    localStorage.clear();
    vi.clearAllMocks();
    api.workspaces.mockResolvedValue([]);
    api.workspaceRepos.mockResolvedValue([]);
    api.workspaceBudget.mockResolvedValue(null);
    api.notificationCount.mockResolvedValue(0);
  });

  it('? key opens keyboard shortcut overlay', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="topbar"]')).toBeTruthy();
    });

    await fireEvent.keyDown(window, { key: '?' });
    await waitFor(() => {
      expect(document.querySelector('.shortcuts-overlay')).toBeTruthy();
    });
  });

  it('? key toggles shortcut overlay off', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="topbar"]')).toBeTruthy();
    });

    await fireEvent.keyDown(window, { key: '?' });
    await waitFor(() => { expect(document.querySelector('.shortcuts-overlay')).toBeTruthy(); });

    await fireEvent.keyDown(window, { key: '?' });
    await waitFor(() => { expect(document.querySelector('.shortcuts-overlay')).toBeNull(); });
  });

  it('Esc closes shortcut overlay', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="topbar"]')).toBeTruthy();
    });

    await fireEvent.keyDown(window, { key: '?' });
    await waitFor(() => { expect(document.querySelector('.shortcuts-overlay')).toBeTruthy(); });

    await fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() => { expect(document.querySelector('.shortcuts-overlay')).toBeNull(); });
  });

  it('shortcut overlay shows new g-key sequences for all 8 tabs', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="topbar"]')).toBeTruthy();
    });

    await fireEvent.keyDown(window, { key: '?' });
    await waitFor(() => {
      const overlay = document.querySelector('.shortcuts-overlay');
      expect(overlay?.textContent).toContain('g h');
      expect(overlay?.textContent).toContain('g 1');
      expect(overlay?.textContent).toContain('g 2');
      expect(overlay?.textContent).toContain('g 3');
      expect(overlay?.textContent).toContain('g 4');
      expect(overlay?.textContent).toContain('g 5');
      expect(overlay?.textContent).toContain('g 6');
      expect(overlay?.textContent).toContain('g 7');
      expect(overlay?.textContent).toContain('g 8');
    });
  });

  it('shortcut overlay shows g 8 for settings tab', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="topbar"]')).toBeTruthy();
    });

    await fireEvent.keyDown(window, { key: '?' });
    await waitFor(() => {
      const overlay = document.querySelector('.shortcuts-overlay');
      expect(overlay?.textContent).toContain('g 8');
      expect(overlay?.textContent).toContain('Settings tab');
    });
  });

  it('shortcut overlay has no ⌘1-6 sidebar shortcuts (superseded by ui-navigation.md §6)', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="topbar"]')).toBeTruthy();
    });

    await fireEvent.keyDown(window, { key: '?' });
    await waitFor(() => {
      const overlay = document.querySelector('.shortcuts-overlay');
      expect(overlay?.textContent).not.toContain('⌘1');
      expect(overlay?.textContent).not.toContain('⌘6');
    });
  });

  it('⌘K does not crash', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="topbar"]')).toBeTruthy();
    });
    await fireEvent.keyDown(window, { key: 'k', metaKey: true });
  });

  // §6 shortcut overlay: context-sensitive labels visible
  it('shortcut overlay labels repo-mode-only shortcuts', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="topbar"]')).toBeTruthy();
    });

    await fireEvent.keyDown(window, { key: '?' });
    await waitFor(() => {
      const overlay = document.querySelector('.shortcuts-overlay');
      // repo-mode shortcuts should be labeled so user knows context
      expect(overlay?.textContent).toContain('repo mode');
    });
  });
});

// ── §8 Responsive Design: mobile hamburger button ─────────────────────

describe('Responsive — hamburger button', () => {
  beforeEach(() => {
    window.history.pushState({}, '', '/');
    localStorage.clear();
    vi.clearAllMocks();
    api.workspaces.mockResolvedValue([]);
    api.workspaceRepos.mockResolvedValue([]);
    api.workspaceBudget.mockResolvedValue(null);
    api.notificationCount.mockResolvedValue(0);
  });

  it('hamburger button is present in the DOM', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="hamburger-btn"]')).toBeTruthy();
    }, { timeout: 3000 });
  });

  it('hamburger button has accessible label', async () => {
    const { container } = render(App);
    await waitFor(() => {
      const btn = container.querySelector('[data-testid="hamburger-btn"]');
      expect(btn).toBeTruthy();
      expect(btn.getAttribute('aria-label')).toBeTruthy();
    }, { timeout: 3000 });
  });

  it('clicking hamburger opens mobile drawer', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="hamburger-btn"]')).toBeTruthy();
    }, { timeout: 3000 });

    await fireEvent.click(container.querySelector('[data-testid="hamburger-btn"]'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="mobile-drawer"]')).toBeTruthy();
    });
  });

  it('mobile drawer lists the workspace-home sections (ui-navigation.md §8)', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="hamburger-btn"]')).toBeTruthy();
    }, { timeout: 3000 });

    await fireEvent.click(container.querySelector('[data-testid="hamburger-btn"]'));
    await waitFor(() => {
      const drawer = container.querySelector('[data-testid="mobile-drawer"]');
      expect(drawer).toBeTruthy();
      expect(drawer.textContent).toContain('Decisions');
      expect(drawer.textContent).toContain('Repos');
      expect(drawer.textContent).toContain('Briefing');
      expect(drawer.textContent).toContain('Specs');
      expect(drawer.textContent).toContain('Agent Rules');
    });
  });

  it('Esc closes mobile drawer', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="hamburger-btn"]')).toBeTruthy();
    }, { timeout: 3000 });

    await fireEvent.click(container.querySelector('[data-testid="hamburger-btn"]'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="mobile-drawer"]')).toBeTruthy();
    });

    await fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="mobile-drawer"]')).toBeNull();
    });
  });

  it('drawer has aria-label for accessibility', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="hamburger-btn"]')).toBeTruthy();
    }, { timeout: 3000 });

    await fireEvent.click(container.querySelector('[data-testid="hamburger-btn"]'));
    await waitFor(() => {
      const drawer = container.querySelector('[data-testid="mobile-drawer"]');
      expect(drawer?.getAttribute('aria-label')).toBeTruthy();
    });
  });
});

// ── §10 Cross-Workspace View: workspace dropdown + routing ────────────

describe('Cross-workspace view', () => {
  beforeEach(() => {
    window.history.pushState({}, '', '/');
    localStorage.clear();
    vi.clearAllMocks();
    api.workspaces.mockResolvedValue([{ id: 'ws-1', name: 'Payments', slug: 'payments' }]);
    api.workspaceRepos.mockResolvedValue([]);
    api.workspaceBudget.mockResolvedValue(null);
    api.notificationCount.mockResolvedValue(0);
  });

  it('workspace dropdown includes "All Workspaces" entry', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="ws-selector"]')).toBeTruthy();
    }, { timeout: 3000 });

    // Open dropdown
    const arrowBtn = container.querySelector('[data-testid="ws-dropdown-toggle"]');
    expect(arrowBtn).toBeTruthy();
    await fireEvent.click(arrowBtn);
    await waitFor(() => {
      const dropdown = container.querySelector('[data-testid="ws-dropdown"]');
      expect(dropdown).toBeTruthy();
      expect(dropdown.textContent).toContain('All Workspaces');
    });
  });

  it('"All Workspaces" entry has a testid', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="ws-selector"]')).toBeTruthy();
    }, { timeout: 3000 });

    const arrowBtn = container.querySelector('[data-testid="ws-dropdown-toggle"]');
    expect(arrowBtn).toBeTruthy();
    await fireEvent.click(arrowBtn);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="ws-all-workspaces"]')).toBeTruthy();
    });
  });

  it('clicking "All Workspaces" navigates to /all', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="ws-selector"]')).toBeTruthy();
    }, { timeout: 3000 });

    const arrowBtn = container.querySelector('[data-testid="ws-dropdown-toggle"]');
    expect(arrowBtn).toBeTruthy();
    await fireEvent.click(arrowBtn);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="ws-all-workspaces"]')).toBeTruthy();
    });
    await fireEvent.click(container.querySelector('[data-testid="ws-all-workspaces"]'));
    await waitFor(() => {
      expect(window.location.pathname).toBe('/all');
    });
  });

  // ── Cross-workspace topbar (Bug 7 fix) ──────────────────────────────
  describe('cross-workspace topbar', () => {
    it('shows "All Workspaces" topbar button when starting at /all', async () => {
      window.history.pushState({}, '', '/all');
      const { container } = render(App);
      await waitFor(() => {
        expect(container.querySelector('[data-testid="ws-all-workspaces-btn"]')).toBeTruthy();
      }, { timeout: 3000 });
    });

    it('shows admin gear icon when user is Admin and starting at /all', async () => {
      window.history.pushState({}, '', '/all');
      api.me.mockResolvedValue({ role: 'Admin' });
      const { container } = render(App);
      await waitFor(() => {
        expect(container.querySelector('[data-testid="all-settings-gear-btn"]')).toBeTruthy();
      }, { timeout: 3000 });
    });

    it('hides admin gear icon when user is Member and starting at /all', async () => {
      window.history.pushState({}, '', '/all');
      api.me.mockResolvedValue({ role: 'Member' });
      const { container } = render(App);
      await waitFor(() => {
        expect(container.querySelector('[data-testid="ws-all-workspaces-btn"]')).toBeTruthy();
      }, { timeout: 3000 });
      // Give time for api.me to resolve
      await new Promise(r => setTimeout(r, 50));
      expect(container.querySelector('[data-testid="all-settings-gear-btn"]')).toBeNull();
    });
  });
});
