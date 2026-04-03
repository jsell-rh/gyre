/**
 * WorkspaceHomeArchitecture.test.js
 *
 * Tests for the collapsible Architecture section in WorkspaceHome (ui-navigation.md §2).
 *
 * Spec requirements:
 *   - Expanded by default (knowledge graph is a primary view)
 *   - "Hide workspace graph" toggle collapses; "Show workspace graph" expands
 *   - Calls api.workspaceGraph(workspaceId) on mount
 *   - Shows graph canvas when loaded
 *   - Shows error + retry on failure
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

// ── Mocks ──────────────────────────────────────────────────────────────────────

vi.mock('../lib/api.js', () => ({
  api: {
    myNotifications: vi.fn().mockResolvedValue([]),
    workspaceRepos: vi.fn().mockResolvedValue([]),
    specsForWorkspace: vi.fn().mockResolvedValue([]),
    getMetaSpecs: vi.fn().mockResolvedValue([]),
    workspaceGraph: vi.fn(),
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
    activity: vi.fn().mockResolvedValue([]),
    mergeQueue: vi.fn().mockResolvedValue([]),
    mergeQueueGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
  },
}));

// ExplorerCanvas is complex — stub it out
vi.mock('../lib/ExplorerCanvas.svelte', () => ({
  default: function ExplorerCanvasStub() {},
}));

vi.mock('../lib/toast.svelte.js', () => ({
  toastInfo: vi.fn(),
  toastError: vi.fn(),
}));

import { api } from '../lib/api.js';
import WorkspaceHome from '../components/WorkspaceHome.svelte';

// ── Fixtures ──────────────────────────────────────────────────────────────────

const WORKSPACE = { id: 'ws-1', name: 'Payments', slug: 'payments', trust_level: 'Guided' };

const GRAPH = {
  nodes: [
    { id: 'r1', label: 'payment-api', kind: 'Repo' },
    { id: 'r2', label: 'user-service', kind: 'Repo' },
  ],
  edges: [
    { source: 'r1', target: 'r2', label: 'depends_on' },
  ],
};

// ── Tests ──────────────────────────────────────────────────────────────────────

// TODO: Architecture section moved to repo mode — update tests for new layout
describe.skip('WorkspaceHome — Architecture section (old layout)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.workspaceGraph.mockResolvedValue(GRAPH);
  });

  it('renders the Architecture section when workspace is provided', () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    expect(container.querySelector('[data-testid="section-architecture"]')).toBeTruthy();
  });

  it('is expanded by default', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    const toggle = container.querySelector('[data-testid="arch-toggle"]');
    expect(toggle).toBeTruthy();
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    await waitFor(() => {
      expect(container.querySelector('[data-testid="arch-body"]')).toBeTruthy();
    });
  });

  it('shows "Hide workspace graph" label when expanded (default)', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="arch-toggle"]').textContent).toContain('Hide workspace graph');
    });
  });

  it('calls workspaceGraph on initial render', async () => {
    render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(api.workspaceGraph).toHaveBeenCalledWith('ws-1');
    });
  });

  it('collapses when toggle is clicked', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    const toggle = container.querySelector('[data-testid="arch-toggle"]');
    await waitFor(() => expect(container.querySelector('[data-testid="arch-body"]')).toBeTruthy());
    await fireEvent.click(toggle);
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(container.querySelector('[data-testid="arch-body"]')).toBeNull();
  });

  it('shows "Show workspace graph" label when collapsed', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    const toggle = container.querySelector('[data-testid="arch-toggle"]');
    await waitFor(() => expect(container.querySelector('[data-testid="arch-body"]')).toBeTruthy());
    await fireEvent.click(toggle);
    expect(toggle.textContent).toContain('Show workspace graph');
  });

  it('re-expands when toggle is clicked a second time', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    const toggle = container.querySelector('[data-testid="arch-toggle"]');
    await waitFor(() => expect(container.querySelector('[data-testid="arch-body"]')).toBeTruthy());
    await fireEvent.click(toggle); // collapse
    expect(container.querySelector('[data-testid="arch-body"]')).toBeNull();
    await fireEvent.click(toggle); // expand again
    await waitFor(() => expect(container.querySelector('[data-testid="arch-body"]')).toBeTruthy());
  });

  it('renders the canvas after graph loads', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(container.querySelector('[data-testid="arch-canvas"]')).toBeTruthy();
    });
  });

  it('does NOT re-fetch graph on collapse/expand cycle', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    const toggle = container.querySelector('[data-testid="arch-toggle"]');

    await waitFor(() => expect(api.workspaceGraph).toHaveBeenCalledTimes(1));

    await fireEvent.click(toggle); // collapse
    await fireEvent.click(toggle); // expand again

    expect(api.workspaceGraph).toHaveBeenCalledTimes(1);
  });

  it('shows error row and retry button on API failure', async () => {
    api.workspaceGraph.mockRejectedValue(new Error('network error'));
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await waitFor(() => {
      expect(container.querySelector('[role="alert"]')).toBeTruthy();
      expect(container.querySelector('[aria-label="Retry loading workspace graph"]')).toBeTruthy();
    });
  });

  it('retry button calls workspaceGraph again', async () => {
    api.workspaceGraph.mockRejectedValueOnce(new Error('fail'));
    api.workspaceGraph.mockResolvedValue(GRAPH);

    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });

    await waitFor(() => {
      expect(container.querySelector('[aria-label="Retry loading workspace graph"]')).toBeTruthy();
    });

    const retryBtn = container.querySelector('[aria-label="Retry loading workspace graph"]');
    await fireEvent.click(retryBtn);

    await waitFor(() => {
      expect(api.workspaceGraph).toHaveBeenCalledTimes(2);
    });
  });

  it('Architecture section has correct aria-labelledby', () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    const section = container.querySelector('[data-testid="section-architecture"]');
    const labelId = section.getAttribute('aria-labelledby');
    expect(labelId).toBe('section-architecture');
    expect(container.querySelector(`#${labelId}`)).toBeTruthy();
  });

  it('toggle button has aria-controls pointing to arch-body id', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    const toggle = container.querySelector('[data-testid="arch-toggle"]');
    expect(toggle.getAttribute('aria-controls')).toBe('arch-body');
  });

  it('section is not rendered when workspace is null', () => {
    const { container } = render(WorkspaceHome, { props: { workspace: null } });
    expect(container.querySelector('[data-testid="section-architecture"]')).toBeNull();
  });

  it('shows six sections total when workspace is provided', () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    expect(container.querySelector('[data-testid="section-decisions"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="section-repos"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="section-architecture"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="section-briefing"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="section-specs"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="section-agent-rules"]')).toBeTruthy();
  });

  it('Architecture section appears between Specs and Agent Rules (per ui-navigation.md §2)', () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    const sections = [...container.querySelectorAll('[data-testid^="section-"]')];
    const ids = sections.map(s => s.getAttribute('data-testid'));
    const specsIdx = ids.indexOf('section-specs');
    const archIdx = ids.indexOf('section-architecture');
    const rulesIdx = ids.indexOf('section-agent-rules');
    expect(archIdx).toBeGreaterThan(specsIdx);
    expect(archIdx).toBeLessThan(rulesIdx);
  });
});
