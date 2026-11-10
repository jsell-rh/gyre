/**
 * WorkspaceHomeScopeDrill.test.js — ui-layout.md §3 Drill-Down (scope wiring)
 *
 * Integration test for the workspace-home Architecture section drill wiring.
 * After task-174 the section embeds the workspace-scoped ExplorerView
 * (ui-layout.md §5 Workspace Scope — Realized Architecture) instead of
 * mounting ExplorerCanvas directly:
 *
 *   WorkspaceHome (real) → ExplorerView (real) → DependencyGraph (real)
 *
 * ExplorerView renders repo nodes via DependencyGraph:
 *   - onNodeClick (single-click) → ExplorerView's repo metadata detail panel
 *     (ws-repo-detail) — does NOT change scope
 *   - onNodeDblClick (double-click) → enterRepo → onEnterRepo prop →
 *     WorkspaceHome.handleArchEnterRepo → onSelectRepo (App.goToRepo:
 *     breadcrumb update, URL pushState, no reload)
 *
 * Everything is real except the api layer and the ELK layout engine (WASM),
 * so the full callback chain from a rendered repo node to the host-shell
 * scope change is exercised. The ExplorerView-internal behaviors (empty
 * state, in-place fallback drill, dependency-fetch failure) are covered by
 * ExplorerViewWorkspaceGraph.test.js; section-level wiring (collapsed by
 * default, aria, scope containment) by WorkspaceHomeArchitecture.test.js.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    // WorkspaceHome data
    myNotifications: vi.fn().mockResolvedValue([]),
    workspaceRepos: vi.fn().mockResolvedValue([]),
    specsForWorkspace: vi.fn().mockResolvedValue([]),
    getMetaSpecs: vi.fn().mockResolvedValue([]),
    approveSpec: vi.fn(),
    revokeSpec: vi.fn(),
    enqueue: vi.fn(),
    markNotificationRead: vi.fn(),
    getWorkspaceBriefing: vi.fn().mockResolvedValue({ narrative: '' }),
    briefingAsk: vi.fn(),
    tasks: vi.fn().mockResolvedValue([]),
    mergeRequests: vi.fn().mockResolvedValue([]),
    mrGates: vi.fn().mockResolvedValue([]),
    mrDiff: vi.fn().mockResolvedValue({ files_changed: 0, insertions: 0, deletions: 0 }),
    updateTaskStatus: vi.fn().mockResolvedValue({}),
    agents: vi.fn().mockResolvedValue([]),
    workspaceBudget: vi.fn().mockResolvedValue(null),
    costSummary: vi.fn().mockResolvedValue([]),
    agent: vi.fn().mockResolvedValue({ name: 'test-agent' }),
    task: vi.fn().mockResolvedValue({ title: 'test-task' }),
    mergeRequest: vi.fn().mockResolvedValue({ title: 'test-mr' }),
    mergeQueue: vi.fn().mockResolvedValue([]),
    mergeQueueGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    staleDependencies: vi.fn().mockResolvedValue([]),
    breakingChanges: vi.fn().mockResolvedValue([]),
    sendAgentMessage: vi.fn().mockResolvedValue({}),
    createWorkspace: vi.fn().mockResolvedValue({ id: 'ws-new', name: 'New WS', slug: 'new-ws' }),
    repoBlastRadius: vi.fn().mockResolvedValue({ direct: [], transitive: [], total: 0 }),
    repoDependents: vi.fn().mockResolvedValue([]),
    workspaceDependencyPolicy: vi.fn().mockResolvedValue(null),
    // ExplorerView data (workspace scope)
    repos: vi.fn(),
    allRepos: vi.fn().mockResolvedValue([]),
    workspaceDependencyGraph: vi.fn(),
    repoGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    getGraphConcept: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({
  toast: vi.fn(),
  toastInfo: vi.fn(),
  toastError: vi.fn(),
  toastSuccess: vi.fn(),
}));

// ELK uses WASM — mock the layout engine with deterministic positions
// (same pattern as ExplorerViewWorkspaceGraph.test.js).
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
import { toastError } from '../lib/toast.svelte.js';
import WorkspaceHome from '../components/WorkspaceHome.svelte';

const WORKSPACE = { id: 'ws-1', name: 'Payments', slug: 'payments', trust_level: 'Guided' };

// A repo list entry in this workspace (what onSelectRepo expects to receive).
const REPO = { id: 'repo-123', name: 'payment-api', description: 'payments service' };

// Workspace dependency graph (what GET /workspaces/ws-1/dependency-graph
// returns): repo-123 plus an external repo node from another workspace.
const DEP_GRAPH = {
  nodes: [
    { repo_id: 'repo-123', name: 'payment-api' },
    { repo_id: 'ext-9', name: 'shared-external' },
  ],
  edges: [
    { id: 'e1', source: 'repo-123', target: 'ext-9', type: 'code', status: 'active' },
  ],
};

/** Render the real WorkspaceHome with the Architecture section expanded.
 *  Seeds the workspace repo list so the drill wiring can resolve the repo. */
async function renderHome({ repoList = [REPO], onSelectRepo = vi.fn() } = {}) {
  api.workspaceRepos.mockResolvedValue(repoList);
  api.repos.mockResolvedValue(repoList);
  api.workspaceDependencyGraph.mockResolvedValue(DEP_GRAPH);
  const { container } = render(WorkspaceHome, {
    props: { workspace: WORKSPACE, onSelectRepo },
  });
  // Expand the Architecture section (collapsed by default per
  // ui-navigation.md §2) and wait for the graph canvas + repo nodes.
  await fireEvent.click(container.querySelector('[data-testid="arch-toggle"]'));
  await waitFor(() => {
    expect(container.querySelector('[data-testid="ws-arch-canvas"]')).toBeTruthy();
  });
  await waitFor(() => {
    expect(container.querySelector('[data-testid="dep-node-repo-123"]')).toBeTruthy();
  });
  return { container, onSelectRepo };
}

describe('WorkspaceHome — Architecture canvas drill wiring (ui-layout.md §3 Drill-Down)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    global.window.matchMedia = vi.fn(() => ({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }));
    global.ResizeObserver = class ResizeObserver {
      observe() {}
      disconnect() {}
      unobserve() {}
    };
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('single-click on a repo node opens the detail panel with repo metadata (no scope change)', async () => {
    const { container, onSelectRepo } = await renderHome();
    await fireEvent.click(container.querySelector('[data-testid="dep-node-repo-123"]'));
    // ExplorerView's repo metadata detail panel opens
    const detail = await waitFor(() => {
      const el = container.querySelector('[data-testid="ws-repo-detail"]');
      expect(el).toBeTruthy();
      return el;
    });
    expect(detail.textContent).toContain('payment-api');
    // Single-click must NOT change scope
    expect(onSelectRepo).not.toHaveBeenCalled();
  });

  it('double-click on a repo node drills to repo scope via onSelectRepo (breadcrumb + URL change)', async () => {
    const { container, onSelectRepo } = await renderHome();
    // Open the detail panel first so the drill also proves it closes on drill
    await fireEvent.click(container.querySelector('[data-testid="dep-node-repo-123"]'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="ws-repo-detail"]')).toBeTruthy();
    });
    await fireEvent.dblClick(container.querySelector('[data-testid="dep-node-repo-123"]'));
    expect(onSelectRepo).toHaveBeenCalledTimes(1);
    expect(onSelectRepo).toHaveBeenCalledWith(
      expect.objectContaining({ id: 'repo-123', name: 'payment-api' }),
      'architecture'
    );
    // Scope drill must close the detail panel (scope changed)
    await waitFor(() => {
      expect(container.querySelector('[data-testid="ws-repo-detail"]')).toBeNull();
    });
  });

  it('double-click on a node whose repo is not in the workspace shows an error, no navigation', async () => {
    // The external repo node renders, but is not in the workspace repo list:
    // repos.find misses → toastError, no onSelectRepo call.
    const { container, onSelectRepo } = await renderHome();
    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-node-ext-9"]')).toBeTruthy();
    });
    await fireEvent.dblClick(container.querySelector('[data-testid="dep-node-ext-9"]'));
    expect(onSelectRepo).not.toHaveBeenCalled();
    expect(toastError).toHaveBeenCalled();
  });
});
