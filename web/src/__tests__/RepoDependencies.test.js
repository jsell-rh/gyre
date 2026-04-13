import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import RepoDependencies from '../components/RepoDependencies.svelte';

// Mock data
const MOCK_DEPENDENCIES = [
  {
    id: 'dep-1',
    source_repo_id: 'repo-current',
    target_repo_id: 'repo-auth',
    dependency_type: 'Code',
    source_artifact: 'Cargo.toml',
    target_artifact: 'gyre-auth',
    version_pinned: '1.2.3',
    target_version_current: '1.4.0',
    version_drift: 2,
    detection_method: 'CargoToml',
    status: 'Active',
    detected_at: 1700000000,
    last_verified_at: 1700003600,
  },
  {
    id: 'dep-2',
    source_repo_id: 'repo-current',
    target_repo_id: 'repo-core',
    dependency_type: 'Spec',
    source_artifact: 'specs/manifest.yaml',
    target_artifact: 'core-api.md',
    version_pinned: null,
    target_version_current: null,
    version_drift: null,
    detection_method: 'SpecManifest',
    status: 'Stale',
    detected_at: 1700000000,
    last_verified_at: 1700003600,
  },
  {
    id: 'dep-3',
    source_repo_id: 'repo-current',
    target_repo_id: 'repo-payment',
    dependency_type: 'Api',
    source_artifact: 'openapi.yaml',
    target_artifact: 'payment-api',
    version_pinned: '^2.0',
    target_version_current: '3.1.0',
    version_drift: 4,
    detection_method: 'Manual',
    status: 'Breaking',
    detected_at: 1700000000,
    last_verified_at: 1700003600,
  },
];

const MOCK_DEPENDENTS = [
  {
    id: 'dept-1',
    source_repo_id: 'repo-billing',
    target_repo_id: 'repo-current',
    dependency_type: 'Code',
    source_artifact: 'package.json',
    target_artifact: 'gyre-utils',
    version_pinned: '0.9.0',
    target_version_current: '1.0.0',
    version_drift: 1,
    detection_method: 'PackageJson',
    status: 'Active',
    detected_at: 1700000000,
    last_verified_at: 1700003600,
  },
  {
    id: 'dept-2',
    source_repo_id: 'repo-analytics',
    target_repo_id: 'repo-current',
    dependency_type: 'Schema',
    source_artifact: 'schema.proto',
    target_artifact: 'events.proto',
    version_pinned: null,
    target_version_current: null,
    version_drift: 0,
    detection_method: 'ProtoImport',
    status: 'Active',
    detected_at: 1700000000,
    last_verified_at: 1700003600,
  },
];

const MOCK_BLAST_RADIUS = {
  repo_id: 'repo-current',
  direct_dependents: ['repo-billing', 'repo-analytics'],
  transitive_dependents: ['repo-dashboard'],
  total: 3,
};

vi.mock('../lib/api.js', () => ({
  api: {
    repoDependencies: vi.fn().mockResolvedValue([]),
    repoDependents: vi.fn().mockResolvedValue([]),
    repoBlastRadius: vi.fn().mockResolvedValue({
      repo_id: 'repo-current',
      direct_dependents: [],
      transitive_dependents: [],
      total: 0,
    }),
  },
}));

// Import api after mock setup so we can reconfigure per test
import { api } from '../lib/api.js';

beforeEach(() => {
  vi.clearAllMocks();
  api.repoDependencies.mockResolvedValue([]);
  api.repoDependents.mockResolvedValue([]);
});

describe('RepoDependencies', () => {
  // ── Rendering dependency list ──────────────────────────────────────────

  it('renders outgoing dependencies and incoming dependents', async () => {
    api.repoDependencies.mockResolvedValue(MOCK_DEPENDENCIES);
    api.repoDependents.mockResolvedValue(MOCK_DEPENDENTS);

    const { container } = render(RepoDependencies, {
      props: { repoId: 'repo-current' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="deps-outgoing"]')).toBeTruthy();
    });

    const outgoing = container.querySelector('[data-testid="deps-outgoing"]');
    expect(outgoing).toBeTruthy();
    expect(outgoing.textContent).toContain('Dependencies (3)');

    const incoming = container.querySelector('[data-testid="deps-incoming"]');
    expect(incoming).toBeTruthy();
    expect(incoming.textContent).toContain('Dependents (2)');

    // Verify dependency rows are rendered
    const depRows = container.querySelectorAll('[data-testid="dep-row"]');
    expect(depRows.length).toBe(3);

    const deptRows = container.querySelectorAll('[data-testid="dependent-row"]');
    expect(deptRows.length).toBe(2);
  });

  // ── Type badges ───────────────────────────────────────────────────────

  it('shows type badges for each dependency', async () => {
    api.repoDependencies.mockResolvedValue(MOCK_DEPENDENCIES);
    api.repoDependents.mockResolvedValue([]);

    const { container } = render(RepoDependencies, {
      props: { repoId: 'repo-current' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-row"]')).toBeTruthy();
    });

    const badges = container.querySelectorAll('[data-testid="deps-outgoing"] .badge');
    const badgeTexts = Array.from(badges).map(b => b.textContent.trim().toLowerCase());
    expect(badgeTexts).toContain('code');
    expect(badgeTexts).toContain('spec');
    expect(badgeTexts).toContain('api');
  });

  // ── Version drift indicators ──────────────────────────────────────────

  it('shows version drift indicators', async () => {
    api.repoDependencies.mockResolvedValue(MOCK_DEPENDENCIES);
    api.repoDependents.mockResolvedValue([]);

    const { container } = render(RepoDependencies, {
      props: { repoId: 'repo-current' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-row"]')).toBeTruthy();
    });

    const drifts = container.querySelectorAll('.dep-drift');
    expect(drifts.length).toBeGreaterThan(0);

    // dep-1 has drift 2 (yellow)
    const driftTexts = Array.from(drifts).map(d => d.textContent.trim());
    expect(driftTexts.some(t => t.includes('2 behind'))).toBe(true);

    // dep-2 has null drift (shows --)
    expect(driftTexts.some(t => t.includes('--'))).toBe(true);

    // dep-3 has drift 4 (red)
    expect(driftTexts.some(t => t.includes('4 behind'))).toBe(true);
  });

  it('shows version pinned values', async () => {
    api.repoDependencies.mockResolvedValue(MOCK_DEPENDENCIES);
    api.repoDependents.mockResolvedValue([]);

    const { container } = render(RepoDependencies, {
      props: { repoId: 'repo-current' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-row"]')).toBeTruthy();
    });

    const versions = container.querySelectorAll('.dep-version');
    const versionTexts = Array.from(versions).map(v => v.textContent.trim());
    expect(versionTexts).toContain('1.2.3');
    expect(versionTexts).toContain('^2.0');
  });

  // ── Stale/breaking badges ─────────────────────────────────────────────

  it('highlights stale dependencies with warning badge', async () => {
    api.repoDependencies.mockResolvedValue(MOCK_DEPENDENCIES);
    api.repoDependents.mockResolvedValue([]);

    const { container } = render(RepoDependencies, {
      props: { repoId: 'repo-current' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-row"]')).toBeTruthy();
    });

    // The Stale dep-row should have stale class
    const staleRows = container.querySelectorAll('.dep-row-stale');
    expect(staleRows.length).toBe(1);

    // Should have a Stale badge
    const staleBadges = container.querySelectorAll('[data-testid="deps-outgoing"] .badge-warning');
    expect(staleBadges.length).toBeGreaterThan(0);
  });

  it('highlights breaking changes with danger badge', async () => {
    api.repoDependencies.mockResolvedValue(MOCK_DEPENDENCIES);
    api.repoDependents.mockResolvedValue([]);

    const { container } = render(RepoDependencies, {
      props: { repoId: 'repo-current' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-row"]')).toBeTruthy();
    });

    // The Breaking dep-row should have breaking class
    const breakingRows = container.querySelectorAll('.dep-row-breaking');
    expect(breakingRows.length).toBe(1);

    // Should show breaking alert at top
    const breakingAlert = container.querySelector('[data-testid="breaking-alert"]');
    expect(breakingAlert).toBeTruthy();
    expect(breakingAlert.textContent).toContain('1 breaking change detected');
  });

  // ── Breaking change alert ─────────────────────────────────────────────

  it('shows prominent breaking change alert when breaking deps exist', async () => {
    api.repoDependencies.mockResolvedValue(MOCK_DEPENDENCIES);
    api.repoDependents.mockResolvedValue([]);

    const { container } = render(RepoDependencies, {
      props: { repoId: 'repo-current' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="breaking-alert"]')).toBeTruthy();
    });

    const alert = container.querySelector('[data-testid="breaking-alert"]');
    expect(alert.textContent).toContain('1 breaking change');
  });

  it('does not show breaking alert when no breaking deps', async () => {
    api.repoDependencies.mockResolvedValue([MOCK_DEPENDENCIES[0]]); // Only Active dep
    api.repoDependents.mockResolvedValue([]);

    const { container } = render(RepoDependencies, {
      props: { repoId: 'repo-current' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dep-row"]')).toBeTruthy();
    });

    expect(container.querySelector('[data-testid="breaking-alert"]')).toBeNull();
  });

  // ── Summary counts ────────────────────────────────────────────────────

  it('shows summary counts in header', async () => {
    api.repoDependencies.mockResolvedValue(MOCK_DEPENDENCIES);
    api.repoDependents.mockResolvedValue(MOCK_DEPENDENTS);

    const { container } = render(RepoDependencies, {
      props: { repoId: 'repo-current' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="deps-summary"]')).toBeTruthy();
    });

    const summary = container.querySelector('[data-testid="deps-summary"]');
    expect(summary.textContent).toContain('3 dependencies');
    expect(summary.textContent).toContain('2 dependents');
    expect(summary.textContent).toContain('1 stale');
    expect(summary.textContent).toContain('1 breaking');
  });

  // ── Blast radius ──────────────────────────────────────────────────────

  it('renders blast radius tree after clicking Show Impact', async () => {
    api.repoDependencies.mockResolvedValue(MOCK_DEPENDENCIES);
    api.repoDependents.mockResolvedValue(MOCK_DEPENDENTS);
    api.repoBlastRadius.mockResolvedValue(MOCK_BLAST_RADIUS);

    const { container } = render(RepoDependencies, {
      props: { repoId: 'repo-current' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="blast-btn"]')).toBeTruthy();
    });

    const btn = container.querySelector('[data-testid="blast-btn"]');
    expect(btn.textContent.trim()).toBe('Show Impact');

    await fireEvent.click(btn);

    await waitFor(() => {
      expect(container.querySelector('[data-testid="blast-tree"]')).toBeTruthy();
    });

    const tree = container.querySelector('[data-testid="blast-tree"]');
    expect(tree.textContent).toContain('Total blast radius: 3 repos');

    const directItems = container.querySelectorAll('[data-testid="blast-direct"]');
    expect(directItems.length).toBe(2);

    const transitiveItems = container.querySelectorAll('[data-testid="blast-transitive"]');
    expect(transitiveItems.length).toBe(1);
  });

  // ── Empty state ───────────────────────────────────────────────────────

  it('shows empty state when no dependencies', async () => {
    api.repoDependencies.mockResolvedValue([]);
    api.repoDependents.mockResolvedValue([]);

    const { container } = render(RepoDependencies, {
      props: { repoId: 'repo-current' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="deps-empty"]')).toBeTruthy();
    });

    const empty = container.querySelector('[data-testid="deps-empty"]');
    expect(empty.textContent).toContain('No cross-repo dependencies detected');
    expect(empty.textContent).toContain('auto-detected');
  });

  // ── Loading state ─────────────────────────────────────────────────────

  it('shows loading state while fetching', () => {
    // Never-resolving promise to keep loading state
    api.repoDependencies.mockReturnValue(new Promise(() => {}));
    api.repoDependents.mockReturnValue(new Promise(() => {}));

    const { container } = render(RepoDependencies, {
      props: { repoId: 'repo-current' },
    });

    expect(container.textContent).toContain('Loading dependencies');
  });

  // ── Dependent version drift ───────────────────────────────────────────

  it('shows current version marker for zero drift dependents', async () => {
    api.repoDependencies.mockResolvedValue([]);
    api.repoDependents.mockResolvedValue(MOCK_DEPENDENTS);

    const { container } = render(RepoDependencies, {
      props: { repoId: 'repo-current' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="dependent-row"]')).toBeTruthy();
    });

    // dept-2 has version_drift = 0
    const drifts = container.querySelectorAll('[data-testid="deps-incoming"] .dep-drift');
    const driftTexts = Array.from(drifts).map(d => d.textContent.trim());
    expect(driftTexts.some(t => t.includes('current'))).toBe(true);
  });
});
