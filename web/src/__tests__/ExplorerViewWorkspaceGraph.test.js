/**
 * ExplorerViewWorkspaceGraph.test.js
 *
 * ui-layout.md §5 — Workspace Scope (Realized Architecture):
 *   - Default Boundary View: graph canvas showing repos as nodes,
 *     cross-repo dependencies as edges (C4 "Container" level)
 *   - Repos without dependencies still appear as nodes
 *   - Empty state: zero repos → "No repos in this workspace yet. Create a
 *     repo to start." with a create link
 *   - Single-click a repo node → detail panel shows repo metadata
 *   - Double-click a repo node → drill to crate level: scope changes to repo
 *     (host navigation via onEnterRepo; in-place crate-level graph fallback
 *     when no host shell is available)
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    workspaces: vi.fn().mockResolvedValue([]),
    workspaceBudget: vi.fn().mockResolvedValue(null),
    repos: vi.fn(),
    workspaceRepos: vi.fn().mockResolvedValue([]),
    allRepos: vi.fn().mockResolvedValue([]),
    repoGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    getGraphConcept: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    workspaceDependencyGraph: vi.fn(),
    mergeRequests: vi.fn().mockResolvedValue([]),
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({ toast: vi.fn() }));

// ELK uses WASM — mock the layout engine with deterministic positions
// (same pattern as DependencyGraph.test.js).
vi.mock('../lib/layout-engines.js', () => ({
  elkLayout: vi.fn().mockImplementation(async (nodes) => {
    const positions = {};
    nodes.forEach((n, i) => {
      positions[n.id] = { x: 100 + i * 200, y: 80 };
    });
    return positions;
  }),
}));

import { api } from '../lib/api.js';
import ExplorerView from '../components/ExplorerView.svelte';

const WS_REPOS = [
  { id: 'repo-1', name: 'frontend', description: 'SPA dashboard', default_branch: 'main', created_at: 1700000000, updated_at: 1700100000 },
  { id: 'repo-2', name: 'backend-api', description: 'HTTP API', default_branch: 'main', created_at: 1700000000, updated_at: 1700200000 },
  { id: 'repo-3', name: 'isolated-lib', description: 'No deps', default_branch: 'main' },
];

const DEP_GRAPH = {
  nodes: [
    { repo_id: 'repo-1', name: 'frontend' },
    { repo_id: 'repo-2', name: 'backend-api' },
    { repo_id: 'external-9', name: 'shared-external' },
  ],
  edges: [
    { id: 'e1', source: 'repo-1', target: 'repo-2', type: 'code', status: 'active' },
    { id: 'e2', source: 'repo-2', target: 'external-9', type: 'spec', status: 'stale' },
  ],
};

beforeEach(() => {
  global.window.matchMedia = vi.fn(() => ({
    matches: false,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  }));
  // ExplorerCanvas globals (needed for the in-place fallback drill test)
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

  vi.clearAllMocks();
  api.repos.mockResolvedValue(WS_REPOS);
  api.workspaceDependencyGraph.mockResolvedValue(DEP_GRAPH);
});

afterEach(() => {
  vi.restoreAllMocks();
  // The in-place drill test pushes a ?repo= URL entry (deep-linkable drill,
  // task-173) and ExplorerView restores the drilled state from the URL on
  // mount — reset it so later renders in this file start at workspace scope.
  window.history.replaceState(null, '', '/');
});

async function renderWorkspaceScope(props = {}) {
  const rendered = render(ExplorerView, {
    props: { scope: { type: 'workspace', workspaceId: 'ws-1' }, ...props },
  });
  await waitFor(() => {
    expect(rendered.container.querySelector('[data-testid="ws-arch-canvas"]')).toBeTruthy();
  });
  return rendered;
}

describe('ExplorerView — workspace scope realized architecture (ui-layout.md §5)', () => {
  it('renders repos as nodes with cross-repo dependency edges (C4 Container level)', async () => {
    const { container } = await renderWorkspaceScope();

    // Graph canvas with dependency edges rendered
    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-svg"]')).toBeTruthy();
      expect(container.querySelectorAll('[data-testid^="dep-edge-"]').length).toBe(DEP_GRAPH.edges.length);
    });
  });

  it('renders every workspace repo as a node, including repos with no dependencies', async () => {
    const { container } = await renderWorkspaceScope();

    await waitFor(() => {
      // 3 workspace repos + 1 external repo referenced by an edge
      expect(container.querySelectorAll('.dep-node').length).toBe(4);
    });
    // Isolated repo (no edges) still appears
    expect(container.querySelector('[data-testid="dep-node-repo-3"]')).toBeTruthy();
    // External repo endpoint is visible so cross-workspace edges resolve
    expect(container.querySelector('[data-testid="dep-node-external-9"]')).toBeTruthy();
  });

  it('shows the spec empty state with a create link when the workspace has zero repos', async () => {
    api.repos.mockResolvedValue([]);
    const { container, getByText } = render(ExplorerView, {
      props: { scope: { type: 'workspace', workspaceId: 'ws-1' } },
    });

    await waitFor(() => {
      expect(getByText('No repos in this workspace yet')).toBeTruthy();
    });
    expect(getByText('Create a repo to start.')).toBeTruthy();
    const createLink = container.querySelector('[data-testid="ws-arch-create-repo"]');
    expect(createLink).toBeTruthy();
    // No graph canvas in the empty state
    expect(container.querySelector('[data-testid="ws-arch-canvas"]')).toBeNull();
  });

  it('single-click a repo node opens a detail panel with repo metadata (no scope change)', async () => {
    const onEnterRepo = vi.fn();
    const { container } = await renderWorkspaceScope({ onEnterRepo });

    const node = container.querySelector('[data-testid="dep-node-repo-1"]');
    await fireEvent.click(node);

    const detail = await waitFor(() => {
      const d = container.querySelector('[data-testid="ws-repo-detail"]');
      expect(d).toBeTruthy();
      return d;
    });
    expect(detail.textContent).toContain('frontend');
    expect(detail.textContent).toContain('SPA dashboard');
    expect(detail.textContent).toContain('main'); // default branch
    // Single click must NOT trigger the scope change
    expect(onEnterRepo).not.toHaveBeenCalled();
    expect(api.repoGraph).not.toHaveBeenCalled();
  });

  it('double-click a repo node changes scope to repo via the host (onEnterRepo)', async () => {
    const onEnterRepo = vi.fn();
    const { container } = await renderWorkspaceScope({ onEnterRepo });

    const node = container.querySelector('[data-testid="dep-node-repo-1"]');
    // Single click first (opens detail), then double-click drills
    await fireEvent.click(node);
    await waitFor(() => expect(container.querySelector('[data-testid="ws-repo-detail"]')).toBeTruthy());

    await fireEvent.dblClick(node);

    await waitFor(() => {
      expect(onEnterRepo).toHaveBeenCalledTimes(1);
      expect(onEnterRepo.mock.calls[0][0].id).toBe('repo-1');
      expect(onEnterRepo.mock.calls[0][0].name).toBe('frontend');
    });
    // Host handles navigation — no in-place fetch
    expect(api.repoGraph).not.toHaveBeenCalled();
    // Detail panel closed by the drill
    await waitFor(() => {
      expect(container.querySelector('[data-testid="ws-repo-detail"]')).toBeNull();
    });
  });

  it('detail panel "Explore architecture" button is the keyboard-reachable drill', async () => {
    const onEnterRepo = vi.fn();
    const { container } = await renderWorkspaceScope({ onEnterRepo });

    await fireEvent.click(container.querySelector('[data-testid="dep-node-repo-2"]'));
    const btn = await waitFor(() => {
      const b = container.querySelector('[data-testid="ws-repo-detail-explore"]');
      expect(b).toBeTruthy();
      return b;
    });
    await fireEvent.click(btn);
    await waitFor(() => {
      expect(onEnterRepo).toHaveBeenCalledTimes(1);
      expect(onEnterRepo.mock.calls[0][0].id).toBe('repo-2');
    });
  });

  it('without a host shell, double-click drills in place to the repo crate-level graph', async () => {
    const { container } = await renderWorkspaceScope();

    const node = container.querySelector('[data-testid="dep-node-repo-1"]');
    await fireEvent.dblClick(node);

    // In-place fallback: the repo's own (crate-level) graph is fetched and shown
    await waitFor(() => {
      expect(api.repoGraph).toHaveBeenCalledWith('repo-1');
    });
    await waitFor(() => {
      expect(container.querySelector('.explorer-view')).toBeTruthy();
    });
  });

  it('dependency-graph fetch failure keeps repos-as-nodes rendering (additive context only)', async () => {
    api.workspaceDependencyGraph.mockRejectedValue(new Error('boom'));
    const { container } = await renderWorkspaceScope();

    await waitFor(() => {
      expect(container.querySelectorAll('.dep-node').length).toBe(WS_REPOS.length);
    });
    // Non-blocking warning surfaced, canvas still present
    expect(container.querySelector('.ws-dep-warning')).toBeTruthy();
    expect(container.querySelectorAll('[data-testid^="dep-edge-"]').length).toBe(0);
  });

  it('fetches the dependency graph scoped to the workspace id', async () => {
    await renderWorkspaceScope();
    await waitFor(() => {
      expect(api.workspaceDependencyGraph).toHaveBeenCalledWith('ws-1');
    });
  });
});
