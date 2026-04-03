import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor } from '@testing-library/svelte';

// Mock API — Decisions section now fetches notifications; provide empty defaults
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
  },
}));

// ExplorerCanvas is complex — stub it out
vi.mock('../lib/ExplorerCanvas.svelte', () => ({
  default: function ExplorerCanvasStub() {},
}));

import WorkspaceHome from '../components/WorkspaceHome.svelte';

describe('WorkspaceHome', () => {
  it('renders without throwing', () => {
    expect(() => render(WorkspaceHome)).not.toThrow();
  });

  it('shows "Select a workspace" when workspace is null', () => {
    const { getByText } = render(WorkspaceHome, { props: { workspace: null } });
    expect(getByText('Select a workspace')).toBeTruthy();
  });

  it('shows key sections when workspace is provided', () => {
    const ws = { id: 'ws-1', name: 'Test', slug: 'test' };
    const { container } = render(WorkspaceHome, { props: { workspace: ws } });
    // Streamlined layout: Repos, Entity/Activity tabs (PipelineOverview removed for cleaner UX)
    expect(container.querySelector('[data-testid="section-repos"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="browse-panel"]')).toBeTruthy();
  });

  it('each section has correct aria-labelledby', () => {
    const ws = { id: 'ws-1', name: 'Test', slug: 'test' };
    const { container } = render(WorkspaceHome, { props: { workspace: ws } });
    const sections = container.querySelectorAll('.home-section');
    sections.forEach(section => {
      const labelledBy = section.getAttribute('aria-labelledby');
      expect(labelledBy).toBeTruthy();
      expect(container.querySelector(`#${labelledBy}`)).toBeTruthy();
    });
  });
});
