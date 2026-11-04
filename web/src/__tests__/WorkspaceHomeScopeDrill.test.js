/**
 * WorkspaceHomeScopeDrill.test.js — ui-layout.md §3 Drill-Down (scope wiring)
 *
 * The workspace-home Architecture section mounts ExplorerCanvas with two
 * callbacks added by task-173:
 *   - onNodeDetail — single-click → detail panel with repo metadata, does
 *     NOT change scope (openDetailPanel context)
 *   - onScopeDrill — double-click → drill to the next C4 level = repo scope
 *     via onSelectRepo (App.goToRepo: breadcrumb update, URL pushState,
 *     no reload)
 *
 * Tests fire the callbacks through the real WorkspaceHome wiring using a
 * canvas stub that captures the props. The branch inside ExplorerCanvas
 * that decides scope-drill vs in-graph drill (repo_id leaf, no Contains
 * children) is covered in ExplorerCanvas.test.js "scope drill-down".
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    myNotifications: vi.fn().mockResolvedValue([]),
    workspaceRepos: vi.fn().mockResolvedValue([]),
    specsForWorkspace: vi.fn().mockResolvedValue([]),
    getMetaSpecs: vi.fn().mockResolvedValue([]),
    workspaceGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    getWorkspaceBriefing: vi.fn().mockResolvedValue({ narrative: '' }),
    briefingAsk: vi.fn(),
    approveSpec: vi.fn(),
    revokeSpec: vi.fn(),
    enqueue: vi.fn(),
    markNotificationRead: vi.fn(),
    resolveNotification: vi.fn(),
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
    activity: vi.fn().mockResolvedValue([]),
    mergeQueue: vi.fn().mockResolvedValue([]),
    mergeQueueGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    workspaceDependencyGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    staleDependencies: vi.fn().mockResolvedValue([]),
    breakingChanges: vi.fn().mockResolvedValue([]),
    sendAgentMessage: vi.fn().mockResolvedValue({}),
    createWorkspace: vi.fn().mockResolvedValue({ id: 'ws-new', name: 'New WS', slug: 'new-ws' }),
    repoBlastRadius: vi.fn().mockResolvedValue({ direct: [], transitive: [], total: 0 }),
    repoDependents: vi.fn().mockResolvedValue([]),
    workspaceDependencyPolicy: vi.fn().mockResolvedValue(null),
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({
  toastInfo: vi.fn(),
  toastError: vi.fn(),
  toastSuccess: vi.fn(),
}));

// Canvas stub: captures the callback props and renders trigger buttons.
vi.mock('../lib/ExplorerCanvas.svelte', async () => {
  return { default: (await import('./helpers/CanvasProbeStub.svelte')).default };
});

import { api } from '../lib/api.js';
import { toastError } from '../lib/toast.svelte.js';
import WorkspaceHome from '../components/WorkspaceHome.svelte';

const WORKSPACE = { id: 'ws-1', name: 'Payments', slug: 'payments', trust_level: 'Guided' };

// A repo list entry in this workspace (what onSelectRepo expects to receive).
const REPO = { id: 'repo-123', name: 'payment-api', description: 'payments service' };

/** Render WorkspaceHome with the Architecture section expanded and the
 *  canvas probe mounted. Seeds the workspace repo list so onScopeDrill's
 *  repos.find resolves. */
async function renderHome({ repoList = [REPO], onSelectRepo = vi.fn() } = {}) {
  const openDetailPanel = vi.fn();
  api.workspaceRepos.mockResolvedValue(repoList);
  const { container } = render(WorkspaceHome, {
    props: { workspace: WORKSPACE, onSelectRepo },
    context: new Map([['openDetailPanel', openDetailPanel]]),
  });
  // Expand the Architecture section (collapsed by default per
  // ui-navigation.md §2) and wait for the graph + canvas to mount.
  await fireEvent.click(container.querySelector('[data-testid="arch-toggle"]'));
  await waitFor(() => {
    expect(container.querySelector('[data-testid="canvas-probe"]')).toBeTruthy();
  });
  return { container, openDetailPanel, onSelectRepo };
}

describe('WorkspaceHome — Architecture canvas drill wiring (ui-layout.md §3 Drill-Down)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('single-click on a repo node opens the detail panel with repo metadata (no scope change)', async () => {
    const { container, openDetailPanel, onSelectRepo } = await renderHome();
    await fireEvent.click(container.querySelector('[data-testid="probe-single-click"]'));
    expect(openDetailPanel).toHaveBeenCalledTimes(1);
    expect(openDetailPanel).toHaveBeenCalledWith(
      expect.objectContaining({ type: 'repo', id: 'repo-123' })
    );
    // Single-click must NOT change scope
    expect(onSelectRepo).not.toHaveBeenCalled();
  });

  it('double-click on a repo node drills to repo scope via onSelectRepo (breadcrumb + URL change)', async () => {
    const { container, openDetailPanel, onSelectRepo } = await renderHome();
    await fireEvent.click(container.querySelector('[data-testid="probe-scope-drill"]'));
    expect(onSelectRepo).toHaveBeenCalledTimes(1);
    expect(onSelectRepo).toHaveBeenCalledWith(
      expect.objectContaining({ id: 'repo-123', name: 'payment-api' }),
      'architecture'
    );
    // Scope drill must NOT open the detail panel
    expect(openDetailPanel).not.toHaveBeenCalled();
  });

  it('double-click on a node whose repo_id is not in the workspace shows an error, no navigation', async () => {
    // Empty repo list: repos.find misses → toastError, no onSelectRepo call.
    const { container, onSelectRepo } = await renderHome({ repoList: [] });
    await fireEvent.click(container.querySelector('[data-testid="probe-scope-drill"]'));
    expect(onSelectRepo).not.toHaveBeenCalled();
    expect(toastError).toHaveBeenCalled();
  });
});
