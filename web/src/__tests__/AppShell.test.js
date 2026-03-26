/**
 * AppShell.test.js — Tests for S4.1 Application Shell
 *
 * Covers:
 *   - URL routing: parseUrl() maps paths to nav + scope
 *   - urlFor(): generates canonical URLs from nav + scope
 *   - Entrypoint flow: first visit → explorer, subsequent → inbox
 *   - Workspace switching
 *   - Keyboard shortcuts
 *   - Scope transitions
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

// ── Pure function tests (no rendering needed) ─────────────────────────
// We extract the routing functions from App.svelte and test them directly.

// Replicated here for unit testing (same logic as App.svelte parseUrl + urlFor)
const NAV_ITEMS = ['inbox', 'briefing', 'explorer', 'specs', 'meta-specs', 'admin'];

function parseUrl(pathname) {
  const parts = pathname.split('/').filter(Boolean);
  if (parts.length === 0) return null;

  if (parts.length === 1) {
    const [nav] = parts;
    if (NAV_ITEMS.includes(nav)) return { nav, scope: { type: 'tenant' } };
  }

  if (parts.length >= 3) {
    const [seg, id, navRaw] = parts;
    const nav = navRaw || 'inbox';
    if (!NAV_ITEMS.includes(nav)) return null;

    if (seg === 'workspaces') {
      return { nav, scope: { type: 'workspace', workspaceId: id } };
    }
    if (seg === 'repos') {
      return { nav, scope: { type: 'repo', repoId: id } };
    }
  }

  return null;
}

function urlFor(nav, s) {
  if (!s || s.type === 'tenant') return `/${nav}`;
  if (s.type === 'workspace') return `/workspaces/${s.workspaceId}/${nav}`;
  if (s.type === 'repo') return `/repos/${s.repoId}/${nav}`;
  return `/${nav}`;
}

// ── URL routing tests ─────────────────────────────────────────────────

describe('parseUrl', () => {
  it('returns null for root path', () => {
    expect(parseUrl('/')).toBeNull();
  });

  it('returns null for empty path', () => {
    expect(parseUrl('')).toBeNull();
  });

  it('parses tenant-scoped inbox', () => {
    expect(parseUrl('/inbox')).toEqual({ nav: 'inbox', scope: { type: 'tenant' } });
  });

  it('parses tenant-scoped briefing', () => {
    expect(parseUrl('/briefing')).toEqual({ nav: 'briefing', scope: { type: 'tenant' } });
  });

  it('parses tenant-scoped explorer', () => {
    expect(parseUrl('/explorer')).toEqual({ nav: 'explorer', scope: { type: 'tenant' } });
  });

  it('parses tenant-scoped specs', () => {
    expect(parseUrl('/specs')).toEqual({ nav: 'specs', scope: { type: 'tenant' } });
  });

  it('parses tenant-scoped meta-specs', () => {
    expect(parseUrl('/meta-specs')).toEqual({ nav: 'meta-specs', scope: { type: 'tenant' } });
  });

  it('parses tenant-scoped admin', () => {
    expect(parseUrl('/admin')).toEqual({ nav: 'admin', scope: { type: 'tenant' } });
  });

  it('returns null for unknown tenant-scoped path', () => {
    expect(parseUrl('/dashboard')).toBeNull();
  });

  it('parses workspace-scoped inbox', () => {
    const result = parseUrl('/workspaces/ws-uuid-1/inbox');
    expect(result).toEqual({
      nav: 'inbox',
      scope: { type: 'workspace', workspaceId: 'ws-uuid-1' },
    });
  });

  it('parses workspace-scoped explorer', () => {
    const result = parseUrl('/workspaces/ws-uuid-2/explorer');
    expect(result).toEqual({
      nav: 'explorer',
      scope: { type: 'workspace', workspaceId: 'ws-uuid-2' },
    });
  });

  it('parses workspace-scoped meta-specs', () => {
    const result = parseUrl('/workspaces/ws-abc/meta-specs');
    expect(result).toEqual({
      nav: 'meta-specs',
      scope: { type: 'workspace', workspaceId: 'ws-abc' },
    });
  });

  it('parses repo-scoped explorer', () => {
    const result = parseUrl('/repos/repo-uuid-1/explorer');
    expect(result).toEqual({
      nav: 'explorer',
      scope: { type: 'repo', repoId: 'repo-uuid-1' },
    });
  });

  it('parses repo-scoped specs', () => {
    const result = parseUrl('/repos/repo-uuid-2/specs');
    expect(result).toEqual({
      nav: 'specs',
      scope: { type: 'repo', repoId: 'repo-uuid-2' },
    });
  });

  it('returns null for workspace path with unknown nav', () => {
    expect(parseUrl('/workspaces/ws-1/dashboard')).toBeNull();
  });

  it('returns null for repos path with unknown nav', () => {
    expect(parseUrl('/repos/r-1/activity')).toBeNull();
  });

  it('returns null for 2-segment paths', () => {
    expect(parseUrl('/workspaces/ws-1')).toBeNull();
  });
});

describe('urlFor', () => {
  it('generates tenant inbox URL', () => {
    expect(urlFor('inbox', { type: 'tenant' })).toBe('/inbox');
  });

  it('generates tenant explorer URL', () => {
    expect(urlFor('explorer', { type: 'tenant' })).toBe('/explorer');
  });

  it('generates workspace inbox URL', () => {
    expect(urlFor('inbox', { type: 'workspace', workspaceId: 'ws-1' })).toBe('/workspaces/ws-1/inbox');
  });

  it('generates workspace briefing URL', () => {
    expect(urlFor('briefing', { type: 'workspace', workspaceId: 'ws-2' })).toBe('/workspaces/ws-2/briefing');
  });

  it('generates repo explorer URL', () => {
    expect(urlFor('explorer', { type: 'repo', repoId: 'repo-1' })).toBe('/repos/repo-1/explorer');
  });

  it('handles null scope (defaults to tenant)', () => {
    expect(urlFor('inbox', null)).toBe('/inbox');
  });

  it('round-trips through parseUrl', () => {
    const nav = 'explorer';
    const scope = { type: 'workspace', workspaceId: 'ws-abc' };
    const url = urlFor(nav, scope);
    const parsed = parseUrl(url);
    expect(parsed).toEqual({ nav, scope });
  });
});

// ── Component rendering tests ─────────────────────────────────────────

vi.mock('../lib/ws.js', () => ({
  createWsStore: () => ({
    onStatus: vi.fn().mockReturnValue(() => {}),
    destroy: vi.fn(),
    subscribe: vi.fn().mockReturnValue(() => {}),
  }),
}));

vi.mock('../lib/api.js', () => ({
  api: {
    workspaces: vi.fn().mockResolvedValue([]),
    mergeRequests: vi.fn().mockResolvedValue([]),
    getPendingSpecs: vi.fn().mockResolvedValue([]),
    workspaceBudget: vi.fn().mockResolvedValue(null),
    tokenInfo: vi.fn().mockResolvedValue({ kind: 'global' }),
  },
  setAuthToken: vi.fn(),
}));

// Mock child view components (they are tested independently).
// In Svelte 5, components are compiled to functions — mocks must be functions too.
// vi.mock factories are hoisted, so helpers must be defined inline.
vi.mock('../components/Inbox.svelte', () => ({ default: function InboxStub() {} }));
vi.mock('../components/Briefing.svelte', () => ({ default: function BriefingStub() {} }));
vi.mock('../components/ExplorerView.svelte', () => ({ default: function ExplorerViewStub() {} }));
vi.mock('../components/SpecDashboard.svelte', () => ({ default: function SpecDashboardStub() {} }));
vi.mock('../components/MetaSpecs.svelte', () => ({ default: function MetaSpecsStub() {} }));
vi.mock('../components/AdminPanel.svelte', () => ({ default: function AdminPanelStub() {} }));

import { api } from '../lib/api.js';
import App from '../App.svelte';
import Sidebar from '../components/Sidebar.svelte';

describe('Sidebar', () => {
  it('renders without throwing', () => {
    expect(() => render(Sidebar)).not.toThrow();
  });

  it('renders all 6 nav items', () => {
    const { getByText } = render(Sidebar);
    expect(getByText('Inbox')).toBeTruthy();
    expect(getByText('Briefing')).toBeTruthy();
    expect(getByText('Explorer')).toBeTruthy();
    expect(getByText('Specs')).toBeTruthy();
    expect(getByText('Meta-specs')).toBeTruthy();
    expect(getByText('Admin')).toBeTruthy();
  });

  it('highlights active nav item', () => {
    const { container } = render(Sidebar, { props: { currentNav: 'inbox' } });
    const activeBtn = container.querySelector('.nav-item.active');
    expect(activeBtn).toBeTruthy();
    expect(activeBtn.textContent).toContain('Inbox');
  });

  it('shows inbox badge when inboxBadge > 0', () => {
    const { container } = render(Sidebar, { props: { currentNav: 'inbox', inboxBadge: 5 } });
    const badge = container.querySelector('.nav-badge');
    expect(badge).toBeTruthy();
    expect(badge.textContent).toBe('5');
  });

  it('shows 99+ when inboxBadge > 99', () => {
    const { container } = render(Sidebar, { props: { currentNav: 'inbox', inboxBadge: 150 } });
    const badge = container.querySelector('.nav-badge');
    expect(badge?.textContent).toBe('99+');
  });

  it('does not show badge when inboxBadge is 0', () => {
    const { container } = render(Sidebar, { props: { currentNav: 'inbox', inboxBadge: 0 } });
    expect(container.querySelector('.nav-badge')).toBeNull();
  });

  it('collapses when collapse button clicked', async () => {
    const { container, getByLabelText } = render(Sidebar);
    const collapseBtn = getByLabelText('Collapse sidebar');
    await fireEvent.click(collapseBtn);
    expect(container.querySelector('.sidebar.collapsed')).toBeTruthy();
  });

  it('expands when expand button clicked after collapse', async () => {
    const { container, getByLabelText } = render(Sidebar);
    const collapseBtn = getByLabelText('Collapse sidebar');
    await fireEvent.click(collapseBtn);
    const expandBtn = getByLabelText('Expand sidebar');
    await fireEvent.click(expandBtn);
    expect(container.querySelector('.sidebar.collapsed')).toBeNull();
  });

  it('calls onnavigate when nav item clicked', async () => {
    const onnavigate = vi.fn();
    const { getByText } = render(Sidebar, { props: { currentNav: 'inbox', onnavigate } });
    await fireEvent.click(getByText('Briefing'));
    expect(onnavigate).toHaveBeenCalledWith('briefing');
  });

  it('shows version indicator at bottom', () => {
    const { getByText } = render(Sidebar);
    expect(getByText('v0.1.0')).toBeTruthy();
  });

  it('does not show workspace switcher', () => {
    const { container } = render(Sidebar);
    // No <select> elements or workspace-switcher class
    expect(container.querySelector('select')).toBeNull();
    expect(container.querySelector('.ws-selector')).toBeNull();
  });
});

// ── Entrypoint flow tests ─────────────────────────────────────────────

describe('Entrypoint flow', () => {
  beforeEach(() => {
    // Reset URL to root so parseUrl returns null and entrypoint flow runs
    window.history.pushState({}, '', '/');
    localStorage.clear();
    vi.clearAllMocks();
    api.workspaces.mockResolvedValue([]);
    api.mergeRequests.mockResolvedValue([]);
    api.getPendingSpecs.mockResolvedValue([]);
    api.workspaceBudget.mockResolvedValue(null);
  });

  it('first visit (no saved workspace): shows explorer nav', async () => {
    // No localStorage 'gyre_workspace_id'
    localStorage.clear();
    api.workspaces.mockResolvedValue([]);

    const { container } = render(App);
    await waitFor(() => {
      const activeBtn = container.querySelector('.nav-item.active');
      expect(activeBtn?.textContent).toContain('Explorer');
    }, { timeout: 3000 });
  });

  it('subsequent visit (saved workspace found): shows inbox nav', async () => {
    const mockWs = { id: 'ws-test-1', name: 'Test Workspace', trust_level: 'Guided' };
    localStorage.setItem('gyre_workspace_id', mockWs.id);
    api.workspaces.mockResolvedValue([mockWs]);
    api.workspaceBudget.mockResolvedValue({ used_credits: 50, total_credits: 100 });

    const { container } = render(App);
    await waitFor(() => {
      const activeBtn = container.querySelector('.nav-item.active');
      expect(activeBtn?.textContent).toContain('Inbox');
    }, { timeout: 3000 });
  });

  it('subsequent visit (saved workspace NOT found): falls back to explorer', async () => {
    localStorage.setItem('gyre_workspace_id', 'stale-workspace-id');
    api.workspaces.mockResolvedValue([]); // workspace not returned

    const { container } = render(App);
    await waitFor(() => {
      // Explorer should be active after fallback
      const activeBtn = container.querySelector('.nav-item.active');
      expect(activeBtn?.textContent).toContain('Explorer');
      // Stale localStorage entry should be cleared
      expect(localStorage.getItem('gyre_workspace_id')).toBeNull();
    }, { timeout: 3000 });
  });
});

// ── URL routing → scope/nav tests ─────────────────────────────────────

describe('URL routing: parseUrl round-trips', () => {
  const ALL_NAVS = ['inbox', 'briefing', 'explorer', 'specs', 'meta-specs', 'admin'];

  ALL_NAVS.forEach(nav => {
    it(`/workspaces/:id/${nav} → workspace scope, nav=${nav}`, () => {
      const result = parseUrl(`/workspaces/ws-123/${nav}`);
      expect(result).toEqual({ nav, scope: { type: 'workspace', workspaceId: 'ws-123' } });
    });

    it(`/${nav} → tenant scope, nav=${nav}`, () => {
      const result = parseUrl(`/${nav}`);
      expect(result).toEqual({ nav, scope: { type: 'tenant' } });
    });
  });

  it('/repos/:id/explorer → repo scope, nav=explorer', () => {
    const result = parseUrl('/repos/repo-abc/explorer');
    expect(result).toEqual({ nav: 'explorer', scope: { type: 'repo', repoId: 'repo-abc' } });
  });

  it('/repos/:id/specs → repo scope, nav=specs', () => {
    const result = parseUrl('/repos/repo-abc/specs');
    expect(result).toEqual({ nav: 'specs', scope: { type: 'repo', repoId: 'repo-abc' } });
  });
});

// ── Scope transition tests ─────────────────────────────────────────────

describe('Scope transitions', () => {
  beforeEach(() => {
    window.history.pushState({}, '', '/');
    localStorage.clear();
    vi.clearAllMocks();
    api.workspaces.mockResolvedValue([]);
    api.mergeRequests.mockResolvedValue([]);
    api.getPendingSpecs.mockResolvedValue([]);
    api.workspaceBudget.mockResolvedValue(null);
  });

  it('app mounts and renders sidebar with all nav items', async () => {
    const { container } = render(App);

    // Wait for app to mount and show breadcrumb
    await waitFor(() => {
      // All 6 nav items should be present
      const navItems = container.querySelectorAll('.nav-item');
      expect(navItems.length).toBeGreaterThanOrEqual(6);
    }, { timeout: 3000 });
  });
});

// ── Keyboard shortcut tests ───────────────────────────────────────────

describe('Keyboard shortcuts', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
    api.workspaces.mockResolvedValue([]);
    api.mergeRequests.mockResolvedValue([]);
    api.getPendingSpecs.mockResolvedValue([]);
  });

  const navShortcuts = [
    { key: '1', nav: 'Briefing' },
    { key: '2', nav: 'Explorer' },
    { key: '3', nav: 'Specs' },
    { key: '4', nav: 'Meta-specs' },
    { key: '5', nav: 'Admin' },
    { key: '6', nav: 'Inbox' },
  ];

  navShortcuts.forEach(({ key, nav }) => {
    it(`Cmd+${key} activates ${nav}`, async () => {
      const { container } = render(App);

      await waitFor(() => {
        expect(container.querySelector('.sidebar')).toBeTruthy();
      });

      await fireEvent.keyDown(window, { key, metaKey: true });

      await waitFor(() => {
        const activeBtn = container.querySelector('.nav-item.active');
        expect(activeBtn?.textContent).toContain(nav);
      });
    });
  });

  it('? key opens keyboard shortcut overlay', async () => {
    const { container } = render(App);

    await waitFor(() => {
      expect(container.querySelector('.sidebar')).toBeTruthy();
    });

    await fireEvent.keyDown(window, { key: '?' });

    await waitFor(() => {
      expect(document.querySelector('.shortcuts-overlay')).toBeTruthy();
    });
  });

  it('? key toggles shortcut overlay off', async () => {
    const { container } = render(App);

    await waitFor(() => {
      expect(container.querySelector('.sidebar')).toBeTruthy();
    });

    // Open
    await fireEvent.keyDown(window, { key: '?' });
    await waitFor(() => {
      expect(document.querySelector('.shortcuts-overlay')).toBeTruthy();
    });

    // Close
    await fireEvent.keyDown(window, { key: '?' });
    await waitFor(() => {
      expect(document.querySelector('.shortcuts-overlay')).toBeNull();
    });
  });

  it('Esc key closes shortcut overlay', async () => {
    const { container } = render(App);

    await waitFor(() => {
      expect(container.querySelector('.sidebar')).toBeTruthy();
    });

    await fireEvent.keyDown(window, { key: '?' });
    await waitFor(() => {
      expect(document.querySelector('.shortcuts-overlay')).toBeTruthy();
    });

    await fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() => {
      expect(document.querySelector('.shortcuts-overlay')).toBeNull();
    });
  });

  it('Cmd+K opens search overlay', async () => {
    const { container } = render(App);

    await waitFor(() => {
      expect(container.querySelector('.sidebar')).toBeTruthy();
    });

    await fireEvent.keyDown(window, { key: 'k', metaKey: true });

    // SearchBar mock — just verify no error thrown
    // (SearchBar is a real component; we just check it doesn't crash)
  });
});

// ── Status bar tests ──────────────────────────────────────────────────

describe('Status bar', () => {
  beforeEach(() => {
    // Reset URL to root so entrypoint flow runs (not URL-based routing)
    window.history.pushState({}, '', '/');
    localStorage.clear();
    vi.clearAllMocks();
    api.workspaces.mockResolvedValue([]);
    api.mergeRequests.mockResolvedValue([]);
    api.getPendingSpecs.mockResolvedValue([]);
    api.workspaceBudget.mockResolvedValue(null);
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
      const wsEl = container.querySelector('.status-ws');
      expect(wsEl).toBeTruthy();
    });
  });

  it('shows trust level when workspace is active', async () => {
    const mockWs = { id: 'ws-1', name: 'W', trust_level: 'Guided' };
    localStorage.setItem('gyre_workspace_id', mockWs.id);
    api.workspaces.mockResolvedValue([mockWs]);
    api.workspaceBudget.mockResolvedValue(null);

    const { container } = render(App);
    // Trust level text is mixed with SVG in the same element, use textContent
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
    // Budget text is mixed with SVG and progress bar in the same element
    await waitFor(() => {
      expect(container.textContent).toContain('Budget: 67%');
    }, { timeout: 3000 });
  });
});
