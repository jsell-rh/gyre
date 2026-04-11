import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, screen, waitFor } from '@testing-library/svelte';
import DetailPanel from '../lib/DetailPanel.svelte';

const mrEntity = {
  type: 'mr',
  id: 'mr-uuid-1',
  data: {
    name: 'Fix auth retry',
    status: 'open',
    conversation_sha: null,
  },
};

const agentEntity = {
  type: 'agent',
  id: 'worker-12',
  data: {
    name: 'worker-12',
    status: 'active',
    conversation_sha: 'abc123',
  },
};

const nodeEntity = {
  type: 'node',
  id: 'node-1',
  data: {
    name: 'AuthMiddleware',
    spec_path: 'specs/system/identity-security.md',
    author_agent_id: 'worker-12',
  },
};

const specEntity = {
  type: 'spec',
  id: 'spec-1',
  data: {
    name: 'identity-security.md',
  },
};

const mergedMrEntity = {
  type: 'mr',
  id: 'mr-uuid-2',
  data: {
    name: 'Merged MR',
    status: 'merged',
    conversation_sha: 'deadbeef',
  },
};

describe('DetailPanel', () => {
  it('renders nothing when entity is null', () => {
    const { container } = render(DetailPanel, { props: { entity: null } });
    const header = container.querySelector('.panel-header');
    expect(header).toBeNull();
  });

  it('shows entity name in header', () => {
    render(DetailPanel, { props: { entity: agentEntity } });
    expect(screen.getAllByText('worker-12').length).toBeGreaterThan(0);
  });

  describe('Tab routing by entity type', () => {
    it('MR entity: shows Info, Diff, Gates, Attestation, Ask Why tabs', () => {
      render(DetailPanel, { props: { entity: mrEntity } });
      expect(screen.getByRole('tab', { name: /info/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /diff/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /gates/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /ask why/i })).toBeTruthy();
      // Attestation tab always shown — shows pending explanation for open MRs
      expect(screen.getByRole('tab', { name: /attestation/i })).toBeTruthy();
    });

    it('merged MR: includes Attestation tab', () => {
      render(DetailPanel, { props: { entity: mergedMrEntity } });
      expect(screen.getByRole('tab', { name: /attestation/i })).toBeTruthy();
    });

    it('Ask Why always enabled for MRs (conversation_sha loaded async)', () => {
      render(DetailPanel, { props: { entity: mrEntity } });
      const askWhy = screen.getByRole('tab', { name: /ask why/i });
      // Ask Why is always enabled — conversation_sha is resolved async from attestation
      expect(askWhy.disabled).toBe(false);
    });

    it('Ask Why enabled when conversation_sha is set', () => {
      render(DetailPanel, { props: { entity: mergedMrEntity } });
      const askWhy = screen.getByRole('tab', { name: /ask why/i });
      expect(askWhy.disabled).toBe(false);
    });

    it('agent entity: shows Info, Chat, Logs, Trace tabs', () => {
      render(DetailPanel, { props: { entity: agentEntity } });
      expect(screen.getByRole('tab', { name: /info/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /chat/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /logs/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /trace/i })).toBeTruthy();
    });

    it('graph node with spec_path + author: shows Info, Spec, Chat, History', () => {
      render(DetailPanel, { props: { entity: nodeEntity } });
      expect(screen.getByRole('tab', { name: /info/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /spec/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /chat/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /history/i })).toBeTruthy();
    });

    it('spec entity: shows Content, Edit, Progress, Links, History tabs', () => {
      render(DetailPanel, { props: { entity: specEntity } });
      expect(screen.getByRole('tab', { name: /content/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /edit/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /progress/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /links/i })).toBeTruthy();
      expect(screen.getByRole('tab', { name: /history/i })).toBeTruthy();
      // Info tab should NOT appear for spec entities (subsumed by Content)
      expect(screen.queryByRole('tab', { name: /^info$/i })).toBeNull();
    });
  });

  describe('Info tab content', () => {
    it('shows entity type and ID', () => {
      render(DetailPanel, { props: { entity: agentEntity } });
      expect(screen.getAllByText(/^Agent$/i).length).toBeGreaterThan(0);
      expect(screen.getAllByText('worker-12').length).toBeGreaterThan(0);
    });

    it('shows status when present', () => {
      render(DetailPanel, { props: { entity: agentEntity } });
      expect(screen.getByText('active')).toBeTruthy();
    });
  });

  describe('Close behavior', () => {
    it('calls onclose when ✕ button is clicked', async () => {
      const onclose = vi.fn();
      render(DetailPanel, { props: { entity: agentEntity, onclose } });
      const closeBtn = screen.getByRole('button', { name: /close detail panel/i });
      await fireEvent.click(closeBtn);
      expect(onclose).toHaveBeenCalledOnce();
    });

    it('calls onclose on Escape key', async () => {
      const onclose = vi.fn();
      const { container } = render(DetailPanel, { props: { entity: agentEntity, onclose } });
      const panel = container.querySelector('.detail-panel');
      await fireEvent.keyDown(panel, { key: 'Escape' });
      expect(onclose).toHaveBeenCalledOnce();
    });
  });

  describe('Pop Out behavior', () => {
    it('pop out button is present', () => {
      render(DetailPanel, { props: { entity: agentEntity } });
      expect(screen.getByRole('button', { name: /pop out/i })).toBeTruthy();
    });

    it('toggling expanded changes panel-btn aria-label', async () => {
      render(DetailPanel, { props: { entity: agentEntity } });
      const popBtn = screen.getByRole('button', { name: /pop out/i });
      await fireEvent.click(popBtn);
      expect(screen.getByRole('button', { name: /collapse/i })).toBeTruthy();
    });
  });

  describe('CSS classes', () => {
    it('panel has open class when entity is set', () => {
      const { container } = render(DetailPanel, { props: { entity: agentEntity } });
      expect(container.querySelector('.detail-panel.open')).toBeTruthy();
    });

    it('panel does not have open class when entity is null', () => {
      const { container } = render(DetailPanel, { props: { entity: null } });
      expect(container.querySelector('.detail-panel.open')).toBeNull();
    });
  });

  describe('Ask Why — interrogation button', () => {
    const interrogationEntity = {
      type: 'agent',
      id: 'agent-42',
      data: {
        name: 'agent-42',
        status: 'active',
        conversation_sha: 'deadbeef1234',
        repo_id: 'repo-abc',
        task_id: 'task-xyz',
      },
    };

    it('shows "Ask Why — Spawn Review Agent" button in Ask Why tab', async () => {
      render(DetailPanel, { props: { entity: interrogationEntity } });
      const askWhyTab = screen.getByRole('tab', { name: /ask why/i });
      await fireEvent.click(askWhyTab);
      expect(screen.getByRole('button', { name: /ask why.*spawn review/i })).toBeTruthy();
    });

    it('calls spawnAgent with correct payload on click', async () => {
      render(DetailPanel, { props: { entity: interrogationEntity } });
      const askWhyTab = screen.getByRole('tab', { name: /ask why/i });
      await fireEvent.click(askWhyTab);
      // Clear any background fetches (e.g. agent detail) before the spawn click
      global.fetch.mockClear();
      global.fetch.mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: async () => ({ agent: { id: 'intr-1' }, token: 'tok', worktree_path: '/w', clone_url: 'u', branch: 'b' }),
      });
      const btn = screen.getByRole('button', { name: /ask why.*spawn review/i });
      await fireEvent.click(btn);
      expect(global.fetch).toHaveBeenCalledWith(
        expect.stringContaining('/agents/spawn'),
        expect.objectContaining({ method: 'POST' }),
      );
      const spawnCall = global.fetch.mock.calls.find(c => c[0].includes('/agents/spawn'));
      expect(spawnCall).toBeTruthy();
      const body = JSON.parse(spawnCall[1].body);
      expect(body.agent_type).toBe('interrogation');
      expect(body.conversation_sha).toBe('deadbeef1234');
      expect(body.repo_id).toBe('repo-abc');
      expect(body.task_id).toBe('task-xyz');
    });

    it('shows error toast when repo_id and task_id are missing', async () => {
      const noContextEntity = {
        type: 'agent',
        id: 'agent-43',
        data: { name: 'agent-43', conversation_sha: 'sha999' },
      };
      render(DetailPanel, { props: { entity: noContextEntity } });
      const askWhyTab = screen.getByRole('tab', { name: /ask why/i });
      await fireEvent.click(askWhyTab);
      // Reset fetch mock after any background agent detail fetches
      global.fetch.mockClear();
      const btn = screen.getByRole('button', { name: /ask why.*spawn review/i });
      await fireEvent.click(btn);
      expect(global.fetch).not.toHaveBeenCalled();
    });
  });

  // ── Impact Analysis in Links tab ──────────────────────────────────────────
  describe('Impact analysis in spec Links tab', () => {
    const specEntityWithRepo = {
      type: 'spec',
      id: 'system/core.md',
      data: { name: 'core.md', repo_id: 'repo-core' },
    };

    function mockFetchForImpact() {
      const mockDependents = [
        {
          id: 'link-1',
          source_path: 'system/auth.md',
          source_repo_id: 'repo-auth',
          link_type: 'depends_on',
          target_path: 'system/core.md',
          target_repo_id: 'repo-core',
          status: 'active',
          created_at: 1700000000,
        },
        {
          id: 'link-2',
          source_path: 'system/billing.md',
          source_repo_id: 'repo-billing',
          link_type: 'implements',
          target_path: 'system/core.md',
          target_repo_id: 'repo-core',
          status: 'active',
          created_at: 1700000000,
        },
      ];

      const mockGraph = {
        nodes: [
          { path: 'system/core.md', title: 'Core', approval_status: 'approved' },
          { path: 'system/auth.md', title: 'Auth', approval_status: 'pending' },
          { path: 'system/billing.md', title: 'Billing', approval_status: 'approved' },
          { path: 'system/deep.md', title: 'Deep', approval_status: 'pending' },
        ],
        edges: [
          { source: 'system/auth.md', target: 'system/core.md', link_type: 'depends_on', status: 'active' },
          { source: 'system/billing.md', target: 'system/core.md', link_type: 'implements', status: 'active' },
          { source: 'system/deep.md', target: 'system/auth.md', link_type: 'extends', status: 'active' },
        ],
      };

      global.fetch.mockImplementation((url) => {
        const urlStr = typeof url === 'string' ? url : url.toString();
        if (urlStr.includes('/dependents')) {
          return Promise.resolve({
            ok: true, status: 200, json: async () => mockDependents,
          });
        }
        if (urlStr.includes('/specs/graph')) {
          return Promise.resolve({
            ok: true, status: 200, json: async () => mockGraph,
          });
        }
        if (urlStr.includes('/links')) {
          return Promise.resolve({
            ok: true, status: 200, json: async () => [],
          });
        }
        // Default: spec content, progress, etc.
        return Promise.resolve({
          ok: true, status: 200,
          json: async () => ({ content: '# Core', total_tasks: 0, completed_tasks: 0 }),
        });
      });
    }

    async function openLinksAndClickAnalyze(container) {
      // Switch to Links tab
      const linksTab = screen.getByRole('tab', { name: /links/i });
      await fireEvent.click(linksTab);

      // Wait for the impact analysis section to appear
      await waitFor(() => {
        expect(container.querySelector('[data-testid="impact-analysis-section"]')).toBeTruthy();
      });

      // Click "Analyze Impact" button (Button component doesn't forward data-testid)
      const analyzeBtn = screen.getByRole('button', { name: /analyze impact/i });
      expect(analyzeBtn).toBeTruthy();
      await fireEvent.click(analyzeBtn);

      // Wait for impact tree to render
      await waitFor(() => {
        expect(container.querySelector('[data-testid="impact-tree"]')).toBeTruthy();
      });
    }

    it('renders impact tree after clicking Analyze Impact in Links tab', async () => {
      mockFetchForImpact();
      const { container } = render(DetailPanel, { props: { entity: specEntityWithRepo } });
      await openLinksAndClickAnalyze(container);

      // Header should show correct counts:
      // 3 specs total (auth, billing direct + deep transitive)
      // 2 repos (repo-auth, repo-billing — deep.md has null repo_id, excluded from count)
      const header = container.querySelector('[data-testid="impact-tree-header"]');
      expect(header).toBeTruthy();
      expect(header.textContent).toContain('3 specs');
      expect(header.textContent).toContain('2 repos');
      expect(header.textContent).toContain('would need review');
    });

    it('shows grouped items with link type and approval status badges', async () => {
      mockFetchForImpact();
      const { container } = render(DetailPanel, { props: { entity: specEntityWithRepo } });
      await openLinksAndClickAnalyze(container);

      // 3 items: auth (direct), billing (direct), deep (transitive depth 2)
      const items = container.querySelectorAll('.impact-tree-item');
      expect(items.length).toBe(3);

      // Direct deps (auth, billing) show "direct", transitive dep (deep) shows "depth 2"
      const authItem = container.querySelector('[data-testid="impact-tree-item-system/auth.md"]');
      expect(authItem).toBeTruthy();
      expect(authItem.querySelector('.impact-tree-depth').textContent).toBe('direct');

      const billingItem = container.querySelector('[data-testid="impact-tree-item-system/billing.md"]');
      expect(billingItem).toBeTruthy();
      expect(billingItem.querySelector('.impact-tree-depth').textContent).toBe('direct');

      const deepItem = container.querySelector('[data-testid="impact-tree-item-system/deep.md"]');
      expect(deepItem).toBeTruthy();
      expect(deepItem.querySelector('.impact-tree-depth').textContent).toBe('depth 2');

      // Repo groups: repo-auth, repo-billing, and unknown (for deep.md with null repo_id)
      const repoGroups = container.querySelectorAll('.impact-tree-repo');
      expect(repoGroups.length).toBe(3);

      // The "unknown" group contains the transitive dep with null repo_id
      const unknownGroup = container.querySelector('[data-testid="impact-tree-repo-unknown"]');
      expect(unknownGroup).toBeTruthy();
      expect(unknownGroup.querySelector('[data-testid="impact-tree-item-system/deep.md"]')).toBeTruthy();
    });
  });

  describe('Editor Split pop-out', () => {
    const specEntityWithRepo = {
      type: 'spec',
      id: 'specs/system/auth.md',
      data: {
        name: 'auth.md',
        repo_id: 'repo-abc',
        title: 'Auth Spec',
      },
    };

    it('shows Preview button in Edit tab when entity has repo_id', async () => {
      global.fetch.mockResolvedValue({
        ok: true,
        status: 200,
        json: async () => ({ content: '# Auth\nSpec content' }),
      });
      render(DetailPanel, { props: { entity: specEntityWithRepo } });
      const editTab = screen.getByRole('tab', { name: /edit/i });
      await fireEvent.click(editTab);
      // Wait for spec content to load (async fetch)
      const previewBtn = await screen.findByRole('button', { name: /preview/i });
      expect(previewBtn).toBeTruthy();
    });

    it('Preview button click expands panel and shows EditorSplit', async () => {
      global.fetch.mockResolvedValue({
        ok: true,
        status: 200,
        json: async () => ({ content: '# Auth' }),
      });
      const { container } = render(DetailPanel, {
        props: { entity: specEntityWithRepo, expanded: false },
      });
      const editTab = screen.getByRole('tab', { name: /edit/i });
      await fireEvent.click(editTab);
      const previewBtn = await screen.findByRole('button', { name: /preview/i });
      await fireEvent.click(previewBtn);
      // Panel should now be expanded
      await waitFor(() => {
        expect(container.querySelector('.detail-panel.expanded')).toBeTruthy();
      });
    });

    it('Back button in EditorSplit collapses panel', async () => {
      global.fetch.mockResolvedValue({
        ok: true,
        status: 200,
        json: async () => ({ content: '# Auth' }),
      });
      const { container } = render(DetailPanel, {
        props: { entity: specEntityWithRepo },
      });
      // Open edit tab and click Preview
      const editTab = screen.getByRole('tab', { name: /edit/i });
      await fireEvent.click(editTab);
      const previewBtn = await screen.findByRole('button', { name: /preview/i });
      await fireEvent.click(previewBtn);
      // Now in EditorSplit — click Back
      const backBtn = await screen.findByRole('button', { name: /close editor split/i });
      await fireEvent.click(backBtn);
      // Panel should collapse
      await waitFor(() => {
        expect(container.querySelector('.detail-panel.expanded')).toBeNull();
      });
    });

    it('Esc key closes EditorSplit and collapses panel', async () => {
      global.fetch.mockResolvedValue({
        ok: true,
        status: 200,
        json: async () => ({ content: '# Auth' }),
      });
      const { container } = render(DetailPanel, {
        props: { entity: specEntityWithRepo },
      });
      const editTab = screen.getByRole('tab', { name: /edit/i });
      await fireEvent.click(editTab);
      const previewBtn = await screen.findByRole('button', { name: /preview/i });
      await fireEvent.click(previewBtn);
      // Wait for EditorSplit to be showing
      await screen.findByRole('button', { name: /close editor split/i });
      // Fire Escape on window
      await fireEvent.keyDown(window, { key: 'Escape' });
      await waitFor(() => {
        expect(container.querySelector('.detail-panel.expanded')).toBeNull();
      });
    });

    it('EditorSplit renders editor and arch preview side by side', async () => {
      global.fetch.mockResolvedValue({
        ok: true,
        status: 200,
        json: async () => ({ content: '# Auth', nodes: [], edges: [] }),
      });
      render(DetailPanel, { props: { entity: specEntityWithRepo } });
      const editTab = screen.getByRole('tab', { name: /edit/i });
      await fireEvent.click(editTab);
      const previewBtn = await screen.findByRole('button', { name: /preview/i });
      await fireEvent.click(previewBtn);
      // Both panes should be visible
      await waitFor(() => {
        expect(screen.getByTestId('editor-split-textarea')).toBeTruthy();
        expect(screen.getByTestId('arch-preview-pane')).toBeTruthy();
      });
    });
  });

  describe('Impact Analysis trigger (TASK-052)', () => {
    it('renders Check Impact button for MR entities', () => {
      const { container } = render(DetailPanel, { props: { entity: mrEntity } });
      const impactBtn = container.querySelector('[data-testid="check-impact-btn"]');
      expect(impactBtn).toBeTruthy();
      expect(impactBtn.textContent).toContain('Check Impact');
    });

    it('does not render Check Impact button for non-MR entities', () => {
      const { container } = render(DetailPanel, { props: { entity: agentEntity } });
      const impactBtn = container.querySelector('[data-testid="check-impact-btn"]');
      expect(impactBtn).toBeFalsy();
    });
  });
});
