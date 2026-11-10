import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

// Mock API — empty defaults, overridden per-test for the queue pause path.
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
    repoStatus: vi.fn(),
  },
}));

// ExplorerCanvas is complex — stub it out
vi.mock('../lib/ExplorerCanvas.svelte', () => ({
  default: function ExplorerCanvasStub() {},
}));

// ELK uses WASM, unavailable in jsdom (same mock as MergeQueueGraph.test.js)
vi.mock('../lib/layout-engines.js', () => ({
  elkLayout: vi.fn().mockImplementation(async (nodes) => {
    const positions = {};
    nodes.forEach((n, i) => {
      positions[n.id] = { x: 100 + i * 220, y: 80 };
    });
    return positions;
  }),
}));

import { api } from '../lib/api.js';
import WorkspaceHome from '../components/WorkspaceHome.svelte';

// Workspace-level merge queue pause state — platform-model.md §6 UI:
// "Merge queue shows paused state with reason". The workspace sidebar merge
// queue widget fetches pause state (gates skipped) for repos with queued
// entries and surfaces it in both list and DAG views.
describe('WorkspaceHome — merge queue paused state', () => {
  const ws = { id: 'ws-1', name: 'Test', slug: 'test' };

  beforeEach(() => {
    vi.clearAllMocks();
    api.myNotifications.mockResolvedValue([]);
    api.workspaceRepos.mockResolvedValue([{ id: 'repo-1', name: 'payments', workspace_id: 'ws-1' }]);
    api.specsForWorkspace.mockResolvedValue([]);
    api.getMetaSpecs.mockResolvedValue([]);
    api.workspaceGraph.mockResolvedValue({ nodes: [], edges: [] });
    api.getWorkspaceBriefing.mockResolvedValue({ narrative: '' });
    api.tasks.mockResolvedValue([]);
    api.mergeRequests.mockResolvedValue([]);
    api.mrGates.mockResolvedValue([]);
    api.agents.mockResolvedValue([]);
    api.workspaceBudget.mockResolvedValue(null);
    api.costSummary.mockResolvedValue([]);
    api.activity.mockResolvedValue([]);
    api.mergeQueue.mockResolvedValue([
      { id: 'e1', merge_request_id: 'mr-1', priority: 50, status: 'queued' },
    ]);
    api.mergeQueueGraph.mockResolvedValue({
      nodes: [{ mr_id: 'mr-1', title: 'Add auth middleware', status: 'approved', priority: 50, depends_on: [], atomic_group: null }],
      edges: [],
    });
    api.mergeRequest.mockResolvedValue({ id: 'mr-1', title: 'Add auth middleware', repository_id: 'repo-1', status: 'open' });
    api.repoStatus.mockResolvedValue({
      repo_id: 'repo-1',
      queue_paused: true,
      pause_reason: 'post-merge validation failed',
      main_green: null,
      post_merge_gates: [],
    });
  });

  it('shows a paused chip on the merge queue widget when a repo queue is paused', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: ws } });

    const chip = await waitFor(() => {
      const el = container.querySelector('[data-testid="mq-paused-chip"]');
      expect(el).toBeTruthy();
      return el;
    });
    expect(chip.textContent).toContain('paused');
    expect(chip.getAttribute('title')).toContain('post-merge validation failed');
    // Pause state is fetched with gates skipped — state display must not
    // execute gate commands.
    expect(api.repoStatus).toHaveBeenCalledWith('repo-1', { withGates: false });
  });

  it('passes the paused state and reason into the DAG view banner', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: ws } });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="mq-paused-chip"]')).toBeTruthy();
    });

    const dagToggle = container.querySelector('[data-testid="mq-toggle-dag"]');
    expect(dagToggle).toBeTruthy();
    await fireEvent.click(dagToggle);

    const banner = await waitFor(() => {
      const el = container.querySelector('[data-testid="mq-paused-banner"]');
      expect(el).toBeTruthy();
      return el;
    });
    expect(banner.textContent).toContain('Merge queue paused');
    expect(banner.textContent).toContain('post-merge validation failed');
  });

  it('shows no paused chip when no repo queue is paused', async () => {
    api.repoStatus.mockResolvedValue({
      repo_id: 'repo-1', queue_paused: false, pause_reason: null, main_green: null, post_merge_gates: [],
    });

    const { container } = render(WorkspaceHome, { props: { workspace: ws } });

    // Wait for the queue widget itself to render (entries present), then
    // assert the paused chip never appears.
    await waitFor(() => {
      expect(container.querySelector('[data-testid="section-merge-queue"]')).toBeTruthy();
    });
    await waitFor(() => {
      expect(api.repoStatus).toHaveBeenCalled();
    });
    expect(container.querySelector('[data-testid="mq-paused-chip"]')).toBeNull();
  });
});
