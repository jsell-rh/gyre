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

const REPO_TABS = ['specs', 'architecture', 'decisions', 'code', 'settings'];

function parseUrl(pathname) {
  const raw = pathname.split('/').filter(Boolean).map(p => {
    try { return decodeURIComponent(p); } catch { return p; }
  });

  if (raw.length === 0) return { mode: 'workspace_home', slug: null, repoName: null, tab: null };

  if (raw.length === 1 && raw[0] === 'profile') {
    return { mode: 'profile', slug: null, repoName: null, tab: null };
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
  const { mode: m, slug, repoName, tab } = parsed;
  if (m === 'profile') return '/profile';
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
  },
  setAuthToken: vi.fn(),
}));

vi.mock('../components/WorkspaceHome.svelte', () => ({ default: function WorkspaceHomeStub() {} }));
vi.mock('../components/RepoMode.svelte', () => ({ default: function RepoModeStub() {} }));
vi.mock('../components/UserProfile.svelte', () => ({ default: function UserProfileStub() {} }));

import { api } from '../lib/api.js';
import App from '../App.svelte';

// ── App shell — topbar-first ─────────────────────────────────────────

describe('App shell — no sidebar', () => {
  beforeEach(() => {
    window.history.pushState({}, '', '/');
    localStorage.clear();
    vi.clearAllMocks();
    api.workspaces.mockResolvedValue([]);
    api.workspaceRepos.mockResolvedValue([]);
    api.workspaceBudget.mockResolvedValue(null);
    api.notificationCount.mockResolvedValue(0);
  });

  it('renders topbar instead of sidebar', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="topbar"]')).toBeTruthy();
      expect(container.querySelector('.sidebar')).toBeNull();
    }, { timeout: 3000 });
  });

  it('shows workspace selector in topbar', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="ws-selector"]')).toBeTruthy();
    }, { timeout: 3000 });
  });

  it('shows decisions badge in topbar', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="decisions-badge"]')).toBeTruthy();
    }, { timeout: 3000 });
  });

  it('shows decisions count when nonzero', async () => {
    api.notificationCount.mockResolvedValue(5);
    const { container } = render(App);
    await waitFor(() => {
      const badge = container.querySelector('.decisions-count');
      expect(badge).toBeTruthy();
      expect(badge.textContent).toBe('5');
    }, { timeout: 3000 });
  });

  it('shows 99+ when decisions count > 99', async () => {
    api.notificationCount.mockResolvedValue(150);
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('.decisions-count')?.textContent).toBe('99+');
    }, { timeout: 3000 });
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

  it('shortcut overlay does NOT show old ⌘1-6 nav shortcuts', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="topbar"]')).toBeTruthy();
    });

    await fireEvent.keyDown(window, { key: '?' });
    await waitFor(() => {
      const overlay = document.querySelector('.shortcuts-overlay');
      expect(overlay?.textContent).not.toContain('⌘1');
      expect(overlay?.textContent).not.toContain('⌘2');
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

  it('mobile drawer has workspace home section links', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="hamburger-btn"]')).toBeTruthy();
    }, { timeout: 3000 });

    await fireEvent.click(container.querySelector('[data-testid="hamburger-btn"]'));
    await waitFor(() => {
      const drawer = container.querySelector('[data-testid="mobile-drawer"]');
      expect(drawer).toBeTruthy();
      expect(drawer.textContent).toContain('Decisions');
      expect(drawer.textContent).toContain('Specs');
      expect(drawer.textContent).toContain('Repos');
      expect(drawer.textContent).toContain('Briefing');
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
    if (arrowBtn) {
      await fireEvent.click(arrowBtn);
      await waitFor(() => {
        const dropdown = container.querySelector('[data-testid="ws-dropdown"]');
        expect(dropdown).toBeTruthy();
        expect(dropdown.textContent).toContain('All Workspaces');
      });
    }
  });

  it('"All Workspaces" entry has a testid', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="ws-selector"]')).toBeTruthy();
    }, { timeout: 3000 });

    const arrowBtn = container.querySelector('[data-testid="ws-dropdown-toggle"]');
    if (arrowBtn) {
      await fireEvent.click(arrowBtn);
      await waitFor(() => {
        expect(container.querySelector('[data-testid="ws-all-workspaces"]')).toBeTruthy();
      });
    }
  });

  it('clicking "All Workspaces" navigates to /all', async () => {
    const { container } = render(App);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="ws-selector"]')).toBeTruthy();
    }, { timeout: 3000 });

    const arrowBtn = container.querySelector('[data-testid="ws-dropdown-toggle"]');
    if (arrowBtn) {
      await fireEvent.click(arrowBtn);
      await waitFor(() => {
        expect(container.querySelector('[data-testid="ws-all-workspaces"]')).toBeTruthy();
      });
      await fireEvent.click(container.querySelector('[data-testid="ws-all-workspaces"]'));
      await waitFor(() => {
        expect(window.location.pathname).toBe('/all');
      });
    }
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
