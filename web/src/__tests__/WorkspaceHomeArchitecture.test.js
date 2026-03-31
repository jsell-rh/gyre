/**
 * WorkspaceHomeArchitecture.test.js
 *
 * Tests for the collapsible Architecture section in WorkspaceHome (ui-navigation.md §2).
 *
 * Spec requirements:
 *   - Collapsed by default
 *   - "Show workspace graph" toggle expands; "Hide workspace graph" collapses
 *   - When expanded, calls api.workspaceGraph(workspaceId)
 *   - Shows graph canvas when loaded
 *   - Shows error + retry on failure
 *   - Does NOT load graph until first expand (lazy)
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

describe('WorkspaceHome — Architecture section', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.workspaceGraph.mockResolvedValue(GRAPH);
  });

  it('renders the Architecture section when workspace is provided', () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    expect(container.querySelector('[data-testid="section-architecture"]')).toBeTruthy();
  });

  it('is collapsed by default', () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    const toggle = container.querySelector('[data-testid="arch-toggle"]');
    expect(toggle).toBeTruthy();
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(container.querySelector('[data-testid="arch-body"]')).toBeNull();
  });

  it('shows "Show workspace graph" label when collapsed', () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    const toggle = container.querySelector('[data-testid="arch-toggle"]');
    expect(toggle.textContent).toContain('Show workspace graph');
  });

  it('does NOT call workspaceGraph on initial render (lazy load)', () => {
    render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    expect(api.workspaceGraph).not.toHaveBeenCalled();
  });

  it('expands and loads graph when toggle is clicked', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    const toggle = container.querySelector('[data-testid="arch-toggle"]');
    await fireEvent.click(toggle);

    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    expect(api.workspaceGraph).toHaveBeenCalledWith('ws-1');

    await waitFor(() => {
      expect(container.querySelector('[data-testid="arch-body"]')).toBeTruthy();
    });
  });

  it('shows "Hide workspace graph" label when expanded', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await fireEvent.click(container.querySelector('[data-testid="arch-toggle"]'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="arch-toggle"]').textContent).toContain('Hide workspace graph');
    });
  });

  it('collapses again when toggle is clicked a second time', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    const toggle = container.querySelector('[data-testid="arch-toggle"]');
    await fireEvent.click(toggle);
    await waitFor(() => expect(container.querySelector('[data-testid="arch-body"]')).toBeTruthy());
    await fireEvent.click(toggle);
    expect(container.querySelector('[data-testid="arch-body"]')).toBeNull();
  });

  it('renders the canvas after graph loads', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await fireEvent.click(container.querySelector('[data-testid="arch-toggle"]'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="arch-canvas"]')).toBeTruthy();
    });
  });

  it('does NOT re-fetch graph on second expand', async () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    const toggle = container.querySelector('[data-testid="arch-toggle"]');

    await fireEvent.click(toggle); // expand
    await waitFor(() => expect(api.workspaceGraph).toHaveBeenCalledTimes(1));

    await fireEvent.click(toggle); // collapse
    await fireEvent.click(toggle); // expand again

    // Should still be called only once
    expect(api.workspaceGraph).toHaveBeenCalledTimes(1);
  });

  it('shows error row and retry button on API failure', async () => {
    api.workspaceGraph.mockRejectedValue(new Error('network error'));
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await fireEvent.click(container.querySelector('[data-testid="arch-toggle"]'));
    await waitFor(() => {
      expect(container.querySelector('[role="alert"]')).toBeTruthy();
      expect(container.querySelector('[aria-label="Retry loading workspace graph"]')).toBeTruthy();
    });
  });

  it('retry button calls workspaceGraph again', async () => {
    api.workspaceGraph.mockRejectedValueOnce(new Error('fail'));
    api.workspaceGraph.mockResolvedValue(GRAPH);

    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    await fireEvent.click(container.querySelector('[data-testid="arch-toggle"]'));

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

  it('Architecture section appears between Specs and Repos (per ui-navigation.md §2)', () => {
    const { container } = render(WorkspaceHome, { props: { workspace: WORKSPACE } });
    const sections = [...container.querySelectorAll('[data-testid^="section-"]')];
    const ids = sections.map(s => s.getAttribute('data-testid'));
    const specsIdx = ids.indexOf('section-specs');
    const archIdx = ids.indexOf('section-architecture');
    const reposIdx = ids.indexOf('section-repos');
    expect(archIdx).toBeGreaterThan(specsIdx);
    expect(archIdx).toBeLessThan(reposIdx);
  });
});
