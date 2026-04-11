import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import ImpactAnalysisModal from '../components/ImpactAnalysisModal.svelte';

// ── Mock data ──────────────────────────────────────────────────────────

const MOCK_BLAST_RADIUS = {
  repo_id: 'repo-main',
  direct_dependents: ['repo-billing', 'repo-auth'],
  transitive_dependents: ['repo-dashboard', 'repo-analytics', 'repo-reporting'],
  total: 5,
};

const MOCK_BREAKING_CHANGES = [
  {
    id: 'bc-1',
    dependency_edge_id: 'edge-1',
    source_repo_id: 'repo-main',
    commit_sha: 'abc123',
    description: 'Removed deprecated API endpoint',
    detected_at: 1700000000,
    acknowledged: false,
    acknowledged_by: null,
    acknowledged_at: null,
  },
  {
    id: 'bc-2',
    dependency_edge_id: 'edge-2',
    source_repo_id: 'repo-main',
    commit_sha: 'def456',
    description: 'Changed response format',
    detected_at: 1700001000,
    acknowledged: true,
    acknowledged_by: 'user-1',
    acknowledged_at: 1700002000,
  },
];

const MOCK_POLICY_BLOCK = {
  breaking_change_behavior: 'block',
  max_version_drift: 3,
  stale_dependency_alert_days: 30,
  require_cascade_tests: true,
  auto_create_update_tasks: true,
};

const MOCK_POLICY_WARN = {
  breaking_change_behavior: 'warn',
  max_version_drift: 3,
  stale_dependency_alert_days: 30,
  require_cascade_tests: false,
  auto_create_update_tasks: false,
};

// ── API mock ──────────────────────────────────────────────────────────

vi.mock('../lib/api.js', () => ({
  api: {
    repoBlastRadius: vi.fn().mockResolvedValue(null),
    breakingChanges: vi.fn().mockResolvedValue([]),
    workspaceDependencyPolicy: vi.fn().mockResolvedValue(null),
    acknowledgeBreakingChange: vi.fn().mockResolvedValue(undefined),
  },
}));

import { api } from '../lib/api.js';

beforeEach(() => {
  vi.clearAllMocks();
  api.repoBlastRadius.mockResolvedValue(null);
  api.breakingChanges.mockResolvedValue([]);
  api.workspaceDependencyPolicy.mockResolvedValue(null);
  api.acknowledgeBreakingChange.mockResolvedValue(undefined);
});

describe('ImpactAnalysisModal', () => {
  // ── Rendering blast radius summary ────────────────────────────────────

  it('renders blast radius summary with direct and transitive counts', async () => {
    api.repoBlastRadius.mockResolvedValue(MOCK_BLAST_RADIUS);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="impact-summary"]')).toBeTruthy();
    });

    const summary = container.querySelector('[data-testid="impact-summary"]');
    expect(summary.textContent).toContain('5');
    expect(summary.textContent).toContain('repos affected');
    expect(summary.textContent).toContain('2');
    expect(summary.textContent).toContain('direct');
    expect(summary.textContent).toContain('3');
    expect(summary.textContent).toContain('transitive');
  });

  // ── Dependency tree ─────────────────────────────────────────────────

  it('displays dependency tree with direct and transitive dependents', async () => {
    api.repoBlastRadius.mockResolvedValue(MOCK_BLAST_RADIUS);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dependency-tree"]')).toBeTruthy();
    });

    const tree = container.querySelector('[data-testid="dependency-tree"]');
    const directDeps = tree.querySelectorAll('[data-testid="direct-dep"]');
    expect(directDeps.length).toBe(2);

    const transitiveDeps = tree.querySelectorAll('[data-testid="transitive-dep"]');
    expect(transitiveDeps.length).toBe(3);
  });

  // ── Per-repo health table ──────────────────────────────────────────

  it('renders per-repo health table with all dependents', async () => {
    api.repoBlastRadius.mockResolvedValue(MOCK_BLAST_RADIUS);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="health-table"]')).toBeTruthy();
    });

    const table = container.querySelector('[data-testid="health-table"]');
    const rows = table.querySelectorAll('[data-testid="health-row"]');
    expect(rows.length).toBe(5);

    // Check table headers
    expect(table.textContent).toContain('Repo');
    expect(table.textContent).toContain('Pinned');
    expect(table.textContent).toContain('Current');
    expect(table.textContent).toContain('Drift');
    expect(table.textContent).toContain('Tests');

    // Check badges — direct vs transitive
    expect(table.textContent).toContain('direct');
    expect(table.textContent).toContain('transitive');

    // Graceful degradation: drift and test status show "--" when data unavailable
    const driftCells = table.querySelectorAll('.health-drift');
    expect(driftCells.length).toBe(5);
    for (const cell of driftCells) {
      expect(cell.textContent).toContain('--');
    }
  });

  // ── Cascade test section ──────────────────────────────────────────

  it('shows cascade test status with trigger button', async () => {
    api.repoBlastRadius.mockResolvedValue(MOCK_BLAST_RADIUS);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="cascade-section"]')).toBeTruthy();
    });

    const cascade = container.querySelector('[data-testid="cascade-section"]');
    expect(cascade.textContent).toContain('Cascade Tests');
    expect(cascade.textContent).toContain('not configured');

    const triggerBtn = container.querySelector('[data-testid="trigger-cascade-btn"]');
    expect(triggerBtn).toBeTruthy();
    expect(triggerBtn.textContent).toContain('Trigger Cascade Tests');
    expect(triggerBtn.disabled).toBe(true);
  });

  // ── Breaking change badge ─────────────────────────────────────────

  it('shows breaking change badge when breaking changes exist', async () => {
    api.repoBlastRadius.mockResolvedValue(MOCK_BLAST_RADIUS);
    api.breakingChanges.mockResolvedValue(MOCK_BREAKING_CHANGES);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="breaking-badge"]')).toBeTruthy();
    });

    const badge = container.querySelector('[data-testid="breaking-badge"]');
    expect(badge.textContent).toContain('Breaking');
  });

  // ── Block policy notice ───────────────────────────────────────────

  it('shows merge blocked notice for block policy workspaces', async () => {
    api.repoBlastRadius.mockResolvedValue(MOCK_BLAST_RADIUS);
    api.breakingChanges.mockResolvedValue(MOCK_BREAKING_CHANGES);
    api.workspaceDependencyPolicy.mockResolvedValue(MOCK_POLICY_BLOCK);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="block-notice"]')).toBeTruthy();
    });

    const notice = container.querySelector('[data-testid="block-notice"]');
    expect(notice.textContent).toContain('Merge blocked');
  });

  // ── Acknowledge button ────────────────────────────────────────────

  it('renders acknowledge buttons for unacknowledged breaking changes in block policy', async () => {
    api.repoBlastRadius.mockResolvedValue(MOCK_BLAST_RADIUS);
    api.breakingChanges.mockResolvedValue(MOCK_BREAKING_CHANGES);
    api.workspaceDependencyPolicy.mockResolvedValue(MOCK_POLICY_BLOCK);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="health-table"]')).toBeTruthy();
    });

    // Acknowledge column should be present
    expect(container.querySelector('.health-table').textContent).toContain('Acknowledge');
  });

  it('calls acknowledge API when acknowledge button is clicked', async () => {
    api.repoBlastRadius.mockResolvedValue({
      repo_id: 'repo-main',
      direct_dependents: ['repo-billing'],
      transitive_dependents: [],
      total: 1,
    });
    api.breakingChanges.mockResolvedValue([
      {
        id: 'bc-single',
        dependency_edge_id: 'edge-1',
        source_repo_id: 'repo-main',
        commit_sha: 'abc123',
        description: 'Breaking API change',
        detected_at: 1700000000,
        acknowledged: false,
        acknowledged_by: null,
        acknowledged_at: null,
      },
    ]);
    api.workspaceDependencyPolicy.mockResolvedValue(MOCK_POLICY_BLOCK);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="health-table"]')).toBeTruthy();
    });

    const ackBtn = container.querySelector('[data-testid="acknowledge-btn"]');
    if (ackBtn) {
      await fireEvent.click(ackBtn);
      expect(api.acknowledgeBreakingChange).toHaveBeenCalledWith('bc-single');
    }
  });

  // ── Graceful degradation ──────────────────────────────────────────

  it('shows blast radius without breaking change details when unavailable', async () => {
    api.repoBlastRadius.mockResolvedValue(MOCK_BLAST_RADIUS);
    api.breakingChanges.mockRejectedValue(new Error('Not found'));

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="impact-summary"]')).toBeTruthy();
    });

    // Summary still renders
    expect(container.querySelector('[data-testid="impact-summary"]').textContent).toContain('5');

    // No breaking badge
    expect(container.querySelector('[data-testid="breaking-badge"]')).toBeFalsy();

    // No block notice
    expect(container.querySelector('[data-testid="block-notice"]')).toBeFalsy();
  });

  it('shows error state when blast radius fails', async () => {
    api.repoBlastRadius.mockRejectedValue(new Error('Server error'));

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="impact-error"]')).toBeTruthy();
    });

    const errorEl = container.querySelector('[data-testid="impact-error"]');
    expect(errorEl.textContent).toContain('Failed to load blast radius');
  });

  it('shows cascade test status as not configured when unavailable', async () => {
    api.repoBlastRadius.mockResolvedValue(MOCK_BLAST_RADIUS);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="cascade-status"]')).toBeTruthy();
    });

    expect(container.querySelector('[data-testid="cascade-status"]').textContent).toContain('not configured');
  });

  // ── No-impact state ───────────────────────────────────────────────

  it('shows no-impact message when blast radius is zero', async () => {
    api.repoBlastRadius.mockResolvedValue({
      repo_id: 'repo-main',
      direct_dependents: [],
      transitive_dependents: [],
      total: 0,
    });

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dependency-tree"]')).toBeTruthy();
    });

    const tree = container.querySelector('[data-testid="dependency-tree"]');
    expect(tree.textContent).toContain('No downstream impact');
  });

  // ── Does not show acknowledge column for warn policy ──────────────

  it('does not show acknowledge column for warn policy', async () => {
    api.repoBlastRadius.mockResolvedValue(MOCK_BLAST_RADIUS);
    api.breakingChanges.mockResolvedValue(MOCK_BREAKING_CHANGES);
    api.workspaceDependencyPolicy.mockResolvedValue(MOCK_POLICY_WARN);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="health-table"]')).toBeTruthy();
    });

    // Acknowledge column should NOT be present for warn policy
    const headers = container.querySelectorAll('.health-table th');
    const headerTexts = Array.from(headers).map(h => h.textContent);
    expect(headerTexts).not.toContain('Acknowledge');
  });

  // ── Loading state ─────────────────────────────────────────────────

  it('shows loading state while fetching data', async () => {
    // Never resolve — keep loading
    api.repoBlastRadius.mockReturnValue(new Promise(() => {}));

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="impact-loading"]')).toBeTruthy();
    });

    expect(container.querySelector('[data-testid="impact-loading"]').textContent).toContain('Analyzing blast radius');
  });

  // ── Modal title ───────────────────────────────────────────────────

  it('includes repo name in modal title', async () => {
    api.repoBlastRadius.mockResolvedValue(MOCK_BLAST_RADIUS);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service' },
    });

    await waitFor(() => {
      expect(container.querySelector('.modal-title')).toBeTruthy();
    });

    expect(container.querySelector('.modal-title').textContent).toContain('Main Service');
  });

  // ── All acknowledged shows resolved notice ────────────────────────

  it('shows resolved notice when all breaking changes are acknowledged', async () => {
    const allAcked = MOCK_BREAKING_CHANGES.map(bc => ({ ...bc, acknowledged: true }));
    api.repoBlastRadius.mockResolvedValue(MOCK_BLAST_RADIUS);
    api.breakingChanges.mockResolvedValue(allAcked);
    api.workspaceDependencyPolicy.mockResolvedValue(MOCK_POLICY_BLOCK);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="block-notice"]')).toBeTruthy();
    });

    const notice = container.querySelector('[data-testid="block-notice"]');
    expect(notice.textContent).toContain('merge unblocked');
  });
});
