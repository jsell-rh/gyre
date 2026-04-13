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

const MOCK_EDGES = [
  {
    id: 'edge-1',
    source_repo_id: 'repo-billing',
    target_repo_id: 'repo-main',
    dependency_type: 'runtime',
    source_artifact: 'package.json',
    target_artifact: 'main-service',
    version_pinned: 'v1.4.0',
    target_version_current: 'v1.5.0',
    version_drift: 1,
    detection_method: 'package_json',
    status: 'active',
    detected_at: 1700000000,
    last_verified_at: 1700001000,
  },
  {
    id: 'edge-2',
    source_repo_id: 'repo-auth',
    target_repo_id: 'repo-main',
    dependency_type: 'runtime',
    source_artifact: 'package.json',
    target_artifact: 'main-service',
    version_pinned: 'v1.2.0',
    target_version_current: 'v1.5.0',
    version_drift: 3,
    detection_method: 'package_json',
    status: 'stale',
    detected_at: 1700000000,
    last_verified_at: 1700001000,
  },
];

const MOCK_CASCADE_RESULTS = [
  { repo_id: 'repo-billing', status: 'passed' },
  { repo_id: 'repo-auth', status: 'failed' },
];

// ── API mock ──────────────────────────────────────────────────────────

vi.mock('../lib/api.js', () => ({
  api: {
    repoBlastRadius: vi.fn().mockResolvedValue(null),
    breakingChanges: vi.fn().mockResolvedValue([]),
    repoDependents: vi.fn().mockResolvedValue([]),
    repo: vi.fn().mockResolvedValue({ workspace_id: 'ws-1' }),
    workspaceDependencyPolicy: vi.fn().mockResolvedValue(null),
    acknowledgeBreakingChange: vi.fn().mockResolvedValue(undefined),
    cascadeTestResults: vi.fn().mockResolvedValue(null),
    triggerCascadeTests: vi.fn().mockResolvedValue(undefined),
  },
}));

import { api } from '../lib/api.js';

beforeEach(() => {
  vi.clearAllMocks();
  api.repoBlastRadius.mockResolvedValue(null);
  api.breakingChanges.mockResolvedValue([]);
  api.repoDependents.mockResolvedValue([]);
  api.repo.mockResolvedValue({ workspace_id: 'ws-1' });
  api.workspaceDependencyPolicy.mockResolvedValue(null);
  api.acknowledgeBreakingChange.mockResolvedValue(undefined);
  api.cascadeTestResults.mockResolvedValue(null);
  api.triggerCascadeTests.mockResolvedValue(undefined);
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

  it('shows cascade test status with functional trigger button when not configured', async () => {
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
    // Button is enabled (functional) — not permanently disabled
    expect(triggerBtn.disabled).toBe(false);
  });

  it('shows cascade test results per dependent when available', async () => {
    api.repoBlastRadius.mockResolvedValue(MOCK_BLAST_RADIUS);
    api.cascadeTestResults.mockResolvedValue(MOCK_CASCADE_RESULTS);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="cascade-results"]')).toBeTruthy();
    });

    const results = container.querySelectorAll('[data-testid="cascade-result"]');
    expect(results.length).toBe(2);

    // Verify pass/fail badges render
    const cascade = container.querySelector('[data-testid="cascade-section"]');
    expect(cascade.textContent).toContain('Pass');
    expect(cascade.textContent).toContain('Fail');
  });

  it('trigger cascade tests button calls API on click', async () => {
    api.repoBlastRadius.mockResolvedValue(MOCK_BLAST_RADIUS);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="trigger-cascade-btn"]')).toBeTruthy();
    });

    const triggerBtn = container.querySelector('[data-testid="trigger-cascade-btn"]');
    expect(triggerBtn).toBeTruthy();
    expect(triggerBtn.disabled).toBe(false);
    await fireEvent.click(triggerBtn);
    expect(api.triggerCascadeTests).toHaveBeenCalledWith('repo-main');
  });

  // ── Breaking change badge ─────────────────────────────────────────

  it('shows breaking change badge when breaking changes exist', async () => {
    api.repoBlastRadius.mockResolvedValue(MOCK_BLAST_RADIUS);
    api.breakingChanges.mockResolvedValue(MOCK_BREAKING_CHANGES);
    api.repoDependents.mockResolvedValue(MOCK_EDGES);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service', workspaceId: 'ws-1' },
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
    api.repoDependents.mockResolvedValue(MOCK_EDGES);
    api.workspaceDependencyPolicy.mockResolvedValue(MOCK_POLICY_BLOCK);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service', workspaceId: 'ws-1' },
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
    api.repoDependents.mockResolvedValue(MOCK_EDGES);
    api.workspaceDependencyPolicy.mockResolvedValue(MOCK_POLICY_BLOCK);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service', workspaceId: 'ws-1' },
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
    api.repoDependents.mockResolvedValue([
      {
        id: 'edge-1',
        source_repo_id: 'repo-billing',
        target_repo_id: 'repo-main',
        dependency_type: 'runtime',
        source_artifact: 'package.json',
        target_artifact: 'main-service',
        version_pinned: 'v1.4.0',
        target_version_current: 'v1.5.0',
        version_drift: 1,
        detection_method: 'package_json',
        status: 'active',
        detected_at: 1700000000,
        last_verified_at: 1700001000,
      },
    ]);
    api.workspaceDependencyPolicy.mockResolvedValue(MOCK_POLICY_BLOCK);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service', workspaceId: 'ws-1' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="health-table"]')).toBeTruthy();
    });

    const ackBtn = container.querySelector('[data-testid="acknowledge-btn"]');
    expect(ackBtn).toBeTruthy();
    await fireEvent.click(ackBtn);
    expect(api.acknowledgeBreakingChange).toHaveBeenCalledWith('bc-single');
  });

  // ── Per-dependent breaking change correlation (F1) ──────────────────

  it('maps different breaking changes to their specific dependent repos via edges', async () => {
    api.repoBlastRadius.mockResolvedValue(MOCK_BLAST_RADIUS);
    api.breakingChanges.mockResolvedValue(MOCK_BREAKING_CHANGES);
    api.repoDependents.mockResolvedValue(MOCK_EDGES);
    api.workspaceDependencyPolicy.mockResolvedValue(MOCK_POLICY_BLOCK);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service', workspaceId: 'ws-1' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="health-table"]')).toBeTruthy();
    });

    // With 5 dependents, edge-1 maps to repo-billing, edge-2 maps to repo-auth
    // bc-1 (unacknowledged, edge-1) should show an "Acknowledge" button for repo-billing
    // bc-2 (acknowledged, edge-2) should show "Acknowledged" badge for repo-auth
    // The other 3 transitive dependents have no matching edges/BCs — should show "—"
    const ackBtns = container.querySelectorAll('[data-testid="acknowledge-btn"]');
    const ackedBadges = container.querySelectorAll('.health-ack');

    // There should be exactly 1 unacknowledged button (for repo-billing)
    expect(ackBtns.length).toBe(1);

    // The table should show the Acknowledged badge for repo-auth (bc-2)
    const ackColumn = Array.from(ackedBadges).map(td => td.textContent.trim());
    expect(ackColumn.some(t => t.includes('Acknowledged'))).toBe(true);
  });

  it('shows version drift data from dependency edges in health table', async () => {
    api.repoBlastRadius.mockResolvedValue({
      repo_id: 'repo-main',
      direct_dependents: ['repo-billing', 'repo-auth'],
      transitive_dependents: [],
      total: 2,
    });
    api.repoDependents.mockResolvedValue(MOCK_EDGES);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="health-table"]')).toBeTruthy();
    });

    const table = container.querySelector('[data-testid="health-table"]');
    // Should show version info from edges
    expect(table.textContent).toContain('v1.4.0');
    expect(table.textContent).toContain('v1.2.0');
    expect(table.textContent).toContain('1 behind');
    expect(table.textContent).toContain('3 behind');
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

  it('shows cascade tests as not configured when API returns null', async () => {
    api.repoBlastRadius.mockResolvedValue(MOCK_BLAST_RADIUS);
    api.cascadeTestResults.mockResolvedValue(null);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="cascade-status"]')).toBeTruthy();
    });

    expect(container.querySelector('[data-testid="cascade-status"]').textContent).toContain('not configured');
    // No cascade results section — data not available
    expect(container.querySelector('[data-testid="cascade-results"]')).toBeFalsy();
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
    api.repoDependents.mockResolvedValue(MOCK_EDGES);
    api.workspaceDependencyPolicy.mockResolvedValue(MOCK_POLICY_WARN);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service', workspaceId: 'ws-1' },
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
    api.repoDependents.mockResolvedValue(MOCK_EDGES);
    api.workspaceDependencyPolicy.mockResolvedValue(MOCK_POLICY_BLOCK);

    const { container } = render(ImpactAnalysisModal, {
      props: { open: true, repoId: 'repo-main', repoName: 'Main Service', workspaceId: 'ws-1' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="block-notice"]')).toBeTruthy();
    });

    const notice = container.querySelector('[data-testid="block-notice"]');
    expect(notice.textContent).toContain('merge unblocked');
  });
});
