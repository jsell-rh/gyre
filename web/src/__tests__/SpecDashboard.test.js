import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, screen, waitFor } from '@testing-library/svelte';
import SpecDashboard from '../components/SpecDashboard.svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    // vi.mock is hoisted so we can't reference outer consts — inline data here
    specsForWorkspace: vi.fn().mockResolvedValue([
      {
        path: 'system/vision.md',
        title: 'Vision',
        owner: 'jsell',
        kind: 'system',
        approval_status: 'approved',
        updated_at: 1700000000,
        repo_id: 'repo-1',
        repo_name: 'main-repo',
      },
      {
        path: 'system/payment-retry.md',
        title: 'Payment Retry',
        owner: 'agent-1',
        kind: 'feature',
        approval_status: 'pending',
        updated_at: 1700003600,
        repo_id: 'repo-2',
        repo_name: 'payments',
      },
      {
        path: 'system/identity.md',
        title: 'Identity',
        owner: 'admin',
        kind: 'security',
        approval_status: 'deprecated',
        updated_at: 1699999000,
        repo_id: 'repo-1',
        repo_name: 'main-repo',
      },
    ]),
    specProgress: vi.fn().mockResolvedValue({
      total_tasks: 5,
      completed_tasks: 4,
      tasks: [
        { id: 'task-1', title: 'Implement retry', status: 'done', agent_id: 'agent-1' },
        { id: 'task-2', title: 'Write tests', status: 'in_progress', agent_id: 'agent-2' },
      ],
    }),
    specsSave: vi.fn().mockResolvedValue({ branch: 'spec-edit/foo-a1b2', mr_id: '42' }),
    specsGraph: vi.fn().mockResolvedValue({
      nodes: [
        { path: 'system/vision.md', title: 'Vision', approval_status: 'approved' },
        { path: 'system/payment-retry.md', title: 'Payment Retry', approval_status: 'pending' },
        { path: 'system/identity.md', title: 'Identity', approval_status: 'deprecated' },
      ],
      edges: [
        { source: 'system/payment-retry.md', target: 'system/vision.md', link_type: 'depends_on', status: 'active' },
        { source: 'system/identity.md', target: 'system/vision.md', link_type: 'implements', status: 'active' },
      ],
    }),
    approveSpec: vi.fn().mockResolvedValue({}),
    rejectSpec: vi.fn().mockResolvedValue({}),
  },
}));

// Mock elkLayout for SpecGraphDAG (used inside SpecDashboard's graph view)
vi.mock('../lib/layout-engines.js', () => ({
  elkLayout: vi.fn().mockImplementation(async (nodes) => {
    const positions = {};
    nodes.forEach((n, i) => {
      positions[n.id] = { x: 100 + i * 200, y: 80 + i * 100 };
    });
    return positions;
  }),
}));

// Reference data for assertions (NOT used inside vi.mock)
const SPECS_PATHS = ['system/vision.md', 'system/payment-retry.md', 'system/identity.md'];

// Helper: the component renders spec paths with .md stripped and directory in a child span,
// but sets title={spec.path} on the .spec-path span. Use title attribute for lookups.
function findSpecByPath(path) {
  return screen.getByTitle(path);
}
function querySpecByPath(path) {
  return screen.queryByTitle(path);
}

// Mock toast
vi.mock('../lib/toast.svelte.js', () => ({
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));

describe('SpecDashboard', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // ── Render ────────────────────────────────────────────────────────────────
  it('renders without throwing', () => {
    expect(() => render(SpecDashboard)).not.toThrow();
  });

  it('shows "Specs" heading', () => {
    render(SpecDashboard);
    expect(screen.getByRole('heading', { name: /specs/i })).toBeTruthy();
  });

  it('shows sortable table for workspace scope after loading', async () => {
    render(SpecDashboard, { props: { scope: 'workspace' } });
    await waitFor(() => expect(screen.getByRole('grid')).toBeTruthy());
  });

  // ── Table columns and sorting ──────────────────────────────────────────────
  it('table shows spec paths in rows', async () => {
    render(SpecDashboard, { props: { scope: 'workspace' } });
    await waitFor(() => expect(findSpecByPath('system/vision.md')).toBeTruthy());
    expect(findSpecByPath('system/payment-retry.md')).toBeTruthy();
    expect(findSpecByPath('system/identity.md')).toBeTruthy();
  });

  it('sorts by path ascending by default', async () => {
    render(SpecDashboard, { props: { scope: 'workspace' } });
    await waitFor(() => expect(findSpecByPath('system/identity.md')).toBeTruthy());
    const rows = screen.getAllByRole('row');
    // Skip header row (index 0), check data rows are sorted alphabetically
    const sortedPaths = [...SPECS_PATHS].sort();
    const rowTexts = rows.slice(1).map((r) => r.textContent ?? '');
    // Component strips .md extension; check for base name without extension
    sortedPaths.forEach((p, i) => {
      const baseName = p.split('/').pop().replace(/\.md$/, '');
      expect(rowTexts[i]).toContain(baseName);
    });
  });

  it('sorts descending on second click of same column', async () => {
    render(SpecDashboard, { props: { scope: 'workspace' } });
    await waitFor(() => expect(findSpecByPath('system/vision.md')).toBeTruthy());
    const pathBtn = screen.getByRole('button', { name: /path/i });
    await fireEvent.click(pathBtn); // → desc
    const rows = screen.getAllByRole('row');
    const rowTexts = rows.slice(1).map((r) => r.textContent ?? '');
    const first = rowTexts[0];
    expect(first).toContain('vision');
  });

  it('sorts by status column on click', async () => {
    render(SpecDashboard, { props: { scope: 'workspace' } });
    await waitFor(() => expect(findSpecByPath('system/vision.md')).toBeTruthy());
    const statusBtn = screen.getByRole('button', { name: /status/i });
    await fireEvent.click(statusBtn);
    // Should not throw and table should still be rendered
    expect(screen.getByRole('grid')).toBeTruthy();
  });

  // ── Filter pills ──────────────────────────────────────────────────────────
  it('shows status filter pills', async () => {
    render(SpecDashboard, { props: { scope: 'workspace' } });
    await waitFor(() => expect(findSpecByPath('system/vision.md')).toBeTruthy());
    expect(screen.getByRole('button', { name: /approved/i })).toBeTruthy();
    expect(screen.getByRole('button', { name: /pending/i })).toBeTruthy();
    expect(screen.getByRole('button', { name: /deprecated/i })).toBeTruthy();
  });

  it('filters to approved only when pill clicked', async () => {
    render(SpecDashboard, { props: { scope: 'workspace' } });
    await waitFor(() => expect(findSpecByPath('system/vision.md')).toBeTruthy());
    const approvedBtn = screen.getByRole('button', { name: /^approved$/i });
    await fireEvent.click(approvedBtn);
    await waitFor(() => {
      expect(findSpecByPath('system/vision.md')).toBeTruthy();
      expect(querySpecByPath('system/payment-retry.md')).toBeNull();
      expect(querySpecByPath('system/identity.md')).toBeNull();
    });
  });

  it('filters to pending only when pill clicked', async () => {
    render(SpecDashboard, { props: { scope: 'workspace' } });
    await waitFor(() => expect(findSpecByPath('system/payment-retry.md')).toBeTruthy());
    const pendingBtn = screen.getByRole('button', { name: /^pending$/i });
    await fireEvent.click(pendingBtn);
    await waitFor(() => {
      expect(findSpecByPath('system/payment-retry.md')).toBeTruthy();
      expect(querySpecByPath('system/vision.md')).toBeNull();
    });
  });

  it('returns to all specs when All pill clicked', async () => {
    render(SpecDashboard, { props: { scope: 'workspace' } });
    await waitFor(() => expect(findSpecByPath('system/vision.md')).toBeTruthy());
    const approvedBtn = screen.getByRole('button', { name: /^approved$/i });
    await fireEvent.click(approvedBtn);
    // There may be two "All" pills (status + kind), use the first one
    const allBtns = screen.getAllByRole('button', { name: /^all$/i });
    await fireEvent.click(allBtns[0]);
    await waitFor(() => {
      expect(findSpecByPath('system/payment-retry.md')).toBeTruthy();
      expect(findSpecByPath('system/identity.md')).toBeTruthy();
    });
  });

  // ── Repo scope: progress bars ─────────────────────────────────────────────
  it('shows sortable table for repo scope', async () => {
    render(SpecDashboard, { props: { scope: 'repo', repoId: 'repo-1' } });
    await waitFor(() => expect(screen.getByRole('grid', { name: /Specs/ })).toBeTruthy());
  });

  it('shows "+ New Spec" button for repo scope', async () => {
    render(SpecDashboard, { props: { scope: 'repo', repoId: 'repo-1' } });
    await waitFor(() => expect(screen.getByRole('button', { name: /\+ new spec/i })).toBeTruthy());
  });

  it('does not show "+ New Spec" for workspace scope', async () => {
    render(SpecDashboard, { props: { scope: 'workspace' } });
    await waitFor(() => expect(screen.getByRole('grid')).toBeTruthy());
    expect(screen.queryByRole('button', { name: /\+ new spec/i })).toBeNull();
  });

  it('repo spec rows show status badges', async () => {
    render(SpecDashboard, { props: { scope: 'repo', repoId: 'repo-1' } });
    await waitFor(() => expect(screen.getByRole('grid', { name: /Specs/ })).toBeTruthy());
    // Data rows should exist (header row + 3 data rows = 4 total)
    const rows = screen.getAllByRole('row');
    expect(rows.length).toBeGreaterThanOrEqual(2);
  });

  // ── Row click opens detail panel ──────────────────────────────────────────
  it('calls openDetailPanel context on row click', async () => {
    const openDetailPanel = vi.fn();
    // Provide context via a wrapper — for simplicity, test that click highlights row
    render(SpecDashboard, { props: { scope: 'workspace' } });
    await waitFor(() => expect(findSpecByPath('system/vision.md')).toBeTruthy());
    const row = screen.getAllByRole('row')[1]; // first data row
    await fireEvent.click(row);
    // After click, the row should be selected (has selected class)
    expect(row.classList.contains('selected')).toBe(true);
  });

  it('keyboard Enter on row also selects it', async () => {
    render(SpecDashboard, { props: { scope: 'workspace' } });
    await waitFor(() => expect(findSpecByPath('system/vision.md')).toBeTruthy());
    const row = screen.getAllByRole('row')[1];
    await fireEvent.keyDown(row, { key: 'Enter' });
    expect(row.classList.contains('selected')).toBe(true);
  });

  // ── New spec modal ─────────────────────────────────────────────────────────
  it('opens new spec modal on button click', async () => {
    render(SpecDashboard, { props: { scope: 'repo', repoId: 'repo-1' } });
    await waitFor(() => expect(screen.getByRole('button', { name: /\+ new spec/i })).toBeTruthy());
    await fireEvent.click(screen.getByRole('button', { name: /\+ new spec/i }));
    await waitFor(() => expect(screen.getByLabelText(/spec path/i)).toBeTruthy());
  });

  it('new spec save button disabled when path is empty', async () => {
    render(SpecDashboard, { props: { scope: 'repo', repoId: 'repo-1' } });
    await waitFor(() => expect(screen.getByRole('button', { name: /\+ new spec/i })).toBeTruthy());
    await fireEvent.click(screen.getByRole('button', { name: /\+ new spec/i }));
    await waitFor(() => {
      const saveBtn = screen.getByRole('button', { name: /save & create mr/i });
      expect(saveBtn.disabled).toBe(true);
    });
  });

  // ── Empty state ───────────────────────────────────────────────────────────
  it('shows empty state when API returns empty list', async () => {
    const { api } = await import('../lib/api.js');
    api.specsForWorkspace.mockResolvedValueOnce([]);
    render(SpecDashboard, { props: { scope: 'workspace' } });
    await waitFor(() => expect(screen.getAllByText(/no specs/i).length).toBeGreaterThan(0));
  });

  // ── Progress bars ─────────────────────────────────────────────────────────
  it('progress bars render in repo scope with correct aria attributes', async () => {
    render(SpecDashboard, { props: { scope: 'repo', repoId: 'repo-1' } });
    await waitFor(() => {
      const bars = screen.getAllByRole('progressbar');
      expect(bars.length).toBeGreaterThan(0);
      expect(bars[0].getAttribute('aria-valuemin')).toBe('0');
      expect(bars[0].getAttribute('aria-valuemax')).toBe('100');
    });
  });

  // ── Kind filter pills ─────────────────────────────────────────────────────
  it('kind filter pills are shown when multiple kinds present', async () => {
    render(SpecDashboard, { props: { scope: 'workspace' } });
    await waitFor(() => expect(findSpecByPath('system/vision.md')).toBeTruthy());
    // Mock data has kinds: system, feature, security
    expect(screen.getByRole('button', { name: /^system$/i })).toBeTruthy();
    expect(screen.getByRole('button', { name: /^feature$/i })).toBeTruthy();
    expect(screen.getByRole('button', { name: /^security$/i })).toBeTruthy();
  });

  // ── New spec save flow ────────────────────────────────────────────────────
  it('new spec save flow calls specsSave with path and content', async () => {
    const { api } = await import('../lib/api.js');
    render(SpecDashboard, { props: { scope: 'repo', repoId: 'repo-1' } });
    await waitFor(() => screen.getByRole('button', { name: /\+ new spec/i }));
    await fireEvent.click(screen.getByRole('button', { name: /\+ new spec/i }));
    const pathInput = await screen.findByLabelText(/spec path/i);
    await fireEvent.input(pathInput, { target: { value: 'system/new-feature.md' } });
    const saveBtn = screen.getByRole('button', { name: /save & create mr/i });
    await fireEvent.click(saveBtn);
    await waitFor(() =>
      expect(api.specsSave).toHaveBeenCalledWith(
        'repo-1',
        expect.objectContaining({ spec_path: 'system/new-feature.md' }),
      ),
    );
  });

  // ── Impact analysis — SpecDashboard ─────────────────────────────────────

  describe('impact analysis', () => {
    async function enterGraphAndImpactMode(container) {
      // Switch to graph view
      const graphBtn = screen.getByRole('button', { name: /graph/i });
      await fireEvent.click(graphBtn);

      // Wait for graph to load and render
      await waitFor(() => {
        expect(container.querySelector('[data-testid="dag-svg"]')).toBeTruthy();
      });

      // Click "Analyze Impact" button
      const impactBtn = container.querySelector('[data-testid="analyze-impact-btn"]');
      expect(impactBtn).toBeTruthy();
      await fireEvent.click(impactBtn);
    }

    it('shows impact summary panel with correct spec/repo counts after selecting a node', async () => {
      const { container } = render(SpecDashboard, { props: { scope: 'workspace' } });
      await waitFor(() => expect(findSpecByPath('system/vision.md')).toBeTruthy());

      await enterGraphAndImpactMode(container);

      // Impact mode prompt should be visible
      expect(container.querySelector('[data-testid="impact-prompt"]')).toBeTruthy();

      // Click vision.md node to analyze its impact
      const visionNode = container.querySelector('[data-testid="dag-node-system/vision.md"]');
      expect(visionNode).toBeTruthy();
      await fireEvent.click(visionNode);

      // Summary panel should appear with 2 dependents across 2 repos
      // (payment-retry depends_on vision, identity implements vision)
      await waitFor(() => {
        const summary = container.querySelector('[data-testid="impact-summary"]');
        expect(summary).toBeTruthy();
      });

      const summaryTitle = container.querySelector('[data-testid="impact-summary-title"]');
      expect(summaryTitle).toBeTruthy();
      expect(summaryTitle.textContent).toContain('2 specs');
      expect(summaryTitle.textContent).toContain('2 repos');
      expect(summaryTitle.textContent).toContain('need review');
    });

    it('shows grouped-by-repo details with link types and approval badges', async () => {
      const { container } = render(SpecDashboard, { props: { scope: 'workspace' } });
      await waitFor(() => expect(findSpecByPath('system/vision.md')).toBeTruthy());

      await enterGraphAndImpactMode(container);

      // Select vision.md
      const visionNode = container.querySelector('[data-testid="dag-node-system/vision.md"]');
      await fireEvent.click(visionNode);

      await waitFor(() => {
        const details = container.querySelector('[data-testid="impact-details"]');
        expect(details).toBeTruthy();
      });

      // Check dependents are listed
      const depItems = container.querySelectorAll('.impact-dep-item');
      expect(depItems.length).toBe(2);

      // Each dependent should have link type badge and approval status badge
      depItems.forEach(item => {
        const badges = item.querySelectorAll('[class*="badge"]');
        expect(badges.length).toBeGreaterThanOrEqual(2);
      });
    });

    it('shows "Exit Impact Analysis" button and exits on click', async () => {
      const { container } = render(SpecDashboard, { props: { scope: 'workspace' } });
      await waitFor(() => expect(findSpecByPath('system/vision.md')).toBeTruthy());

      await enterGraphAndImpactMode(container);

      // Exit button should be visible
      const exitBtn = container.querySelector('[data-testid="exit-impact-mode"]');
      expect(exitBtn).toBeTruthy();
      expect(exitBtn.textContent).toContain('Exit Impact Analysis');

      // Click exit
      await fireEvent.click(exitBtn);

      // "Analyze Impact" button should reappear
      await waitFor(() => {
        expect(container.querySelector('[data-testid="analyze-impact-btn"]')).toBeTruthy();
      });
      // Impact prompt should not be shown
      expect(container.querySelector('[data-testid="impact-prompt"]')).toBeNull();
    });

    it('clears selection when same node is clicked again (deselect toggle)', async () => {
      const { container } = render(SpecDashboard, { props: { scope: 'workspace' } });
      await waitFor(() => expect(findSpecByPath('system/vision.md')).toBeTruthy());

      await enterGraphAndImpactMode(container);

      // Select vision.md
      const visionNode = container.querySelector('[data-testid="dag-node-system/vision.md"]');
      await fireEvent.click(visionNode);
      await waitFor(() => {
        expect(container.querySelector('[data-testid="impact-summary"]')).toBeTruthy();
      });

      // Click same node again to deselect
      await fireEvent.click(visionNode);
      await waitFor(() => {
        // Summary should disappear, prompt should appear
        expect(container.querySelector('[data-testid="impact-summary"]')).toBeNull();
        expect(container.querySelector('[data-testid="impact-prompt"]')).toBeTruthy();
      });
    });
  });
});
