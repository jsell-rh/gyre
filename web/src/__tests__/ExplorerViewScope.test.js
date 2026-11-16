import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    workspaces: vi.fn().mockResolvedValue([]),
    workspaceBudget: vi.fn().mockResolvedValue(null),
    repos: vi.fn().mockResolvedValue([]),
    workspaceRepos: vi.fn().mockResolvedValue([]),
    allRepos: vi.fn().mockResolvedValue([]),
    repoGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    getGraphConcept: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    workspaceDependencyGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    workspaceGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    getWorkspaceBriefing: vi.fn().mockResolvedValue({
      completed: [], in_progress: [], cross_workspace: [], exceptions: [], metrics: null,
    }),
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({ toast: vi.fn() }));

import ExplorerView from '../components/ExplorerView.svelte';

beforeEach(() => {
  // Mock matchMedia for the chatCollapsed viewport sync $effect
  global.window.matchMedia = vi.fn(() => ({
    matches: false,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  }));
  // Mock globals needed by ExplorerCanvas when graph loads
  global.ResizeObserver = class ResizeObserver {
    observe() {}
    disconnect() {}
    unobserve() {}
  };
  global.requestAnimationFrame = vi.fn(cb => { cb(); return 1; });
  global.cancelAnimationFrame = vi.fn();
  HTMLCanvasElement.prototype.getContext = vi.fn(() => ({
    clearRect: vi.fn(), fillRect: vi.fn(), strokeRect: vi.fn(),
    beginPath: vi.fn(), closePath: vi.fn(), arc: vi.fn(), fill: vi.fn(),
    stroke: vi.fn(), moveTo: vi.fn(), lineTo: vi.fn(), quadraticCurveTo: vi.fn(),
    fillText: vi.fn(), measureText: vi.fn(() => ({ width: 40 })),
    scale: vi.fn(), setTransform: vi.fn(), save: vi.fn(), restore: vi.fn(),
    translate: vi.fn(), rotate: vi.fn(),
    fillStyle: '', strokeStyle: '', lineWidth: 0, globalAlpha: 1,
    font: '', textAlign: '', textBaseline: '', shadowColor: '', shadowBlur: 0,
    setLineDash: vi.fn(), getLineDash: vi.fn(() => []),
  }));
  global.WebSocket = class MockWebSocket {
    constructor() {
      this.readyState = 1;
      this.send = vi.fn();
      this.close = vi.fn();
    }
  };
});

afterEach(() => {
  vi.restoreAllMocks();
  // Reset the jsdom URL so subTab deep-link tests do not leak state.
  window.history.replaceState(null, '', '/');
});

describe('ExplorerView scope branching', () => {
  it('renders without throwing with no props', () => {
    expect(() => render(ExplorerView)).not.toThrow();
  });

  it('renders WorkspaceCards at tenant scope (default)', async () => {
    const { container } = render(ExplorerView, { props: { scope: { type: 'tenant' } } });
    await waitFor(() => {
      // WorkspaceCards shows explorer header or empty state
      expect(container.innerHTML.length).toBeGreaterThan(0);
    });
  });

  it('renders canvas placeholder at workspace scope', async () => {
    const { getByText } = render(ExplorerView, {
      props: { scope: { type: 'workspace', workspaceId: 'ws-1' } },
    });
    await waitFor(() => {
      expect(getByText('Workspace Architecture')).toBeTruthy();
    });
  });

  it('renders repo graph view at repo scope', async () => {
    // Don't set repoId to avoid triggering ExplorerCanvas render
    const { getByText } = render(ExplorerView, {
      props: { scope: { type: 'repo' } },
    });
    await waitFor(() => {
      // Explorer header is rendered for repo scope
      expect(getByText('Architecture')).toBeTruthy();
    });
  });

  it('defaults to tenant scope when scope prop is omitted', async () => {
    const { container } = render(ExplorerView);
    // Should render WorkspaceCards (tenant scope), not the graph
    await waitFor(() => {
      expect(container.innerHTML).not.toContain('Architecture');
    });
  });
});

describe('Architecture sub-tab control (repo scope, ui-navigation.md §3)', () => {
  // Regression: a drawer Briefing click while the Architecture tab is
  // already active must switch the rendered content, not just rewrite the
  // ?subTab= URL param. App drives the sub-tab through the archSubTab prop
  // (single source of truth); the URL param alone only seeds a fresh mount.
  it('switches to Briefing content when the archSubTab prop changes to briefing', async () => {
    const api = (await import('../lib/api.js')).api;
    const props = {
      scope: { type: 'repo', workspaceId: 'ws-1', repoId: 'repo-1' },
      archSubTab: 'graph',
    };
    const { container, rerender } = render(ExplorerView, { props });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="arch-subtab-briefing"]')).toBeTruthy();
    });
    // Graph is the visible sub-tab; no Briefing content rendered.
    expect(container.querySelector('[data-testid="arch-briefing"]')).toBeNull();
    expect(
      container.querySelector('[data-testid="arch-subtab-briefing"]').classList.contains('active')
    ).toBe(false);

    // App state change after the drawer click (repoArchSubTab → 'briefing')
    // must drive the content switch.
    await rerender({ ...props, archSubTab: 'briefing' });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="arch-briefing"]')).toBeTruthy();
    });
    // The Briefing sub-tab fetches the real repo-scoped narrative.
    await waitFor(() => expect(api.getWorkspaceBriefing).toHaveBeenCalledWith('ws-1', null, 'repo-1'));
  });

  it('control-bar buttons report sub-tab changes to the parent', async () => {
    const onArchSubTabChange = vi.fn();
    const { container } = render(ExplorerView, {
      props: {
        scope: { type: 'repo', workspaceId: 'ws-1', repoId: 'repo-1' },
        archSubTab: 'graph',
        onArchSubTabChange,
      },
    });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="arch-subtab-graph"]')).toBeTruthy();
    });
    await fireEvent.click(container.querySelector('[data-testid="arch-subtab-briefing"]'));
    expect(onArchSubTabChange).toHaveBeenCalledWith('briefing');
    await fireEvent.click(container.querySelector('[data-testid="arch-subtab-graph"]'));
    expect(onArchSubTabChange).toHaveBeenCalledWith('graph');
  });

  // Regression: the initial-read effect used to delete ?subTab from the URL
  // after seeding the sub-tab, so a reload/copy of a deep-linked
  // architecture?subTab=briefing URL silently reopened the Graph sub-tab.
  // App's setRepoArchSubTab invariant requires the URL to keep matching the
  // rendered sub-tab (docs/ui.md "deep-linkable via ?subTab=briefing").
  it('keeps the ?subTab deep-link param in the URL after the initial read', async () => {
    window.history.replaceState(null, '', '/workspaces/ws-1/r/repo-1/architecture?subTab=briefing');
    // App-mimicking onArchSubTabChange: setRepoArchSubTab mirrors the
    // sub-tab into the ?subTab= URL param.
    const onArchSubTabChange = vi.fn((sub) => {
      if (sub === 'briefing') {
        const url = new URL(window.location.href);
        url.searchParams.set('subTab', 'briefing');
        window.history.replaceState(window.history.state, '', url.toString());
      }
    });
    render(ExplorerView, {
      props: {
        scope: { type: 'repo', workspaceId: 'ws-1', repoId: 'repo-1' },
        onArchSubTabChange,
      },
    });
    // The deep link seeds the Briefing sub-tab and is reported to the parent.
    await waitFor(() => {
      expect(onArchSubTabChange).toHaveBeenCalledWith('briefing');
    });
    // The URL still matches the rendered sub-tab, not stripped by the
    // initial read, so reload/copy reopens Briefing.
    expect(new URLSearchParams(window.location.search).get('subTab')).toBe('briefing');
  });
});
