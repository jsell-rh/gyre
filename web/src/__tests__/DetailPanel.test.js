import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, fireEvent, screen, waitFor, within } from '@testing-library/svelte';
import DetailPanel from '../lib/DetailPanel.svelte';

// Toasts render through the app-level Toast component, not the panel —
// assert on the store calls instead of the DOM.
vi.mock('../lib/toast.svelte.js', () => ({
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
  toastWarning: vi.fn(),
  toastInfo: vi.fn(),
}));

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

  // ── Link direction in spec Links tab ──────────────────────────────────
  describe('Link direction in spec Links tab', () => {
    const specEntityWithRepo = {
      type: 'spec',
      id: 'system/core.md',
      data: { name: 'core.md', repo_id: 'repo-core' },
    };

    const inboundLink = {
      id: 'link-in',
      direction: 'inbound',
      source_path: 'system/auth.md',
      link_type: 'depends_on',
      target_path: 'system/core.md',
      status: 'stale',
      created_at: 1700000000,
    };
    const outboundLink = {
      id: 'link-out',
      direction: 'outbound',
      source_path: 'system/core.md',
      link_type: 'depends_on',
      target_path: 'system/billing.md',
      status: 'active',
      created_at: 1700000000,
    };

    function mockFetchForLinks(links) {
      global.fetch.mockImplementation((url) => {
        const urlStr = typeof url === 'string' ? url : url.toString();
        if (urlStr.includes('/links')) {
          return Promise.resolve({ ok: true, status: 200, json: async () => links });
        }
        // Default: spec content, progress, etc.
        return Promise.resolve({
          ok: true, status: 200,
          json: async () => ({ content: '# Core', total_tasks: 0, completed_tasks: 0 }),
        });
      });
    }

    it('renders the inbound link in the inbound column and the outbound link in the outbound column', async () => {
      mockFetchForLinks([inboundLink, outboundLink]);
      const { container } = render(DetailPanel, { props: { entity: specEntityWithRepo } });
      const linksTab = screen.getByRole('tab', { name: /links/i });
      await fireEvent.click(linksTab);

      await waitFor(() => {
        expect(container.querySelector('.links-list')).toBeTruthy();
      });

      // Diagram: the inbound column shows the SOURCE spec of the inbound
      // link; the outbound column shows the TARGET spec of the outbound link.
      const inboundCol = container.querySelector('.link-column-inbound');
      expect(inboundCol).toBeTruthy();
      expect(inboundCol.textContent).toContain('auth.md');
      expect(inboundCol.textContent).not.toContain('billing.md');

      const outboundCol = container.querySelector('.link-column-outbound');
      expect(outboundCol).toBeTruthy();
      expect(outboundCol.textContent).toContain('billing.md');
      expect(outboundCol.textContent).not.toContain('auth.md');
    });

    it("labels the inbound row '<- from' with the source spec and the outbound row '-> to'", async () => {
      mockFetchForLinks([inboundLink, outboundLink]);
      const { container } = render(DetailPanel, { props: { entity: specEntityWithRepo } });
      const linksTab = screen.getByRole('tab', { name: /links/i });
      await fireEvent.click(linksTab);

      const rows = await waitFor(() => {
        const items = container.querySelectorAll('.link-item');
        expect(items.length).toBe(2);
        return items;
      });

      const inboundRow = [...rows].find((r) => r.textContent.includes('system/auth.md'));
      const outboundRow = [...rows].find((r) => r.textContent.includes('system/billing.md'));
      expect(inboundRow).toBeTruthy();
      expect(outboundRow).toBeTruthy();
      expect(inboundRow.querySelector('.link-direction').textContent).toBe('<- from');
      expect(outboundRow.querySelector('.link-direction').textContent).toBe('-> to');
    });

    it('shows the staleness badge on a stale inbound link', async () => {
      mockFetchForLinks([inboundLink]);
      const { container } = render(DetailPanel, { props: { entity: specEntityWithRepo } });
      const linksTab = screen.getByRole('tab', { name: /links/i });
      await fireEvent.click(linksTab);

      const row = await waitFor(() => {
        const item = container.querySelector('.link-item');
        expect(item).toBeTruthy();
        return item;
      });
      // Staleness still surfaces via the status badge on the link row.
      expect(row.textContent).toContain('stale');
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

    it('a single bubbling Escape from the editor closes only the EditorSplit — no double-fire into onback', async () => {
      global.fetch.mockResolvedValue({
        ok: true,
        status: 200,
        json: async () => ({ content: '# Auth' }),
      });
      const onback = vi.fn();
      const { container } = render(DetailPanel, {
        props: { entity: specEntityWithRepo, onback },
      });
      const editTab = screen.getByRole('tab', { name: /edit/i });
      await fireEvent.click(editTab);
      const previewBtn = await screen.findByRole('button', { name: /preview/i });
      await fireEvent.click(previewBtn);
      // Focus is inside the panel (the normal case — the user was just
      // editing the textarea), so one Escape reaches BOTH the panel div's
      // onkeydown and EditorSplit's window handler on its bubbling path.
      const textarea = await screen.findByTestId('editor-split-textarea');
      await fireEvent.keyDown(textarea, { key: 'Escape' });
      // The editor split closed...
      await waitFor(() => {
        expect(screen.queryByTestId('editor-split-textarea')).toBeNull();
      });
      // ...but the panel did NOT also pop detail history (Esc double-fire).
      expect(onback).not.toHaveBeenCalled();
      // The panel itself collapsed via closeEditorSplit, not via close().
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

describe('DetailPanel — Trace tab span status casing (HSI §3a)', () => {
  // The server serializes span status per the spec's JSON example casing
  // ("Ok"/"Error"). The waterfall bar must color Error spans red regardless
  // of wire casing — a lowercase-only comparison silently renders errors as
  // healthy bars.
  function traceBody(statusError, statusOk) {
    return {
      id: 'trace-1',
      mr_id: 'mr-uuid-1',
      gate_run_id: 'gate-run-1',
      commit_sha: 'abc',
      captured_at: 1,
      span_count: 2,
      spans: [
        { span_id: 's1', parent_span_id: null, operation_name: 'http request', service_name: 'svc', kind: 'Server', start_time: 1, duration_us: 1000, attributes: {}, status: statusError, graph_node_id: null },
        { span_id: 's2', parent_span_id: null, operation_name: 'db query', service_name: 'svc', kind: 'Database', start_time: 1, duration_us: 500, attributes: {}, status: statusOk, graph_node_id: 'n1' },
      ],
      root_spans: ['s1', 's2'],
    };
  }
  function stubTraceFetch(body) {
    vi.stubGlobal('fetch', vi.fn(async (url) => {
      if (String(url).includes('/merge-requests/mr-uuid-1/trace')) {
        return { ok: true, status: 200, json: async () => body };
      }
      return { ok: true, status: 200, json: async () => ({}) };
    }));
  }
  afterEach(() => vi.unstubAllGlobals());

  async function waterfallStyles(body) {
    stubTraceFetch(body);
    const { container } = render(DetailPanel, { props: { entity: mrEntity } });
    await fireEvent.click(screen.getByRole('tab', { name: /trace/i }));
    await waitFor(() => expect(container.querySelector('.trace-waterfall-bar')).toBeTruthy());
    return [...container.querySelectorAll('.trace-waterfall-bar')].map((b) => b.getAttribute('style'));
  }

  it('renders Error-status bars red and Ok-status bars healthy with spec PascalCase values', async () => {
    const styles = await waterfallStyles(traceBody('Error', 'Ok'));
    expect(styles.filter((s) => s.includes('background: var(--color-danger)'))).toHaveLength(1);
    expect(styles.filter((s) => s.includes('background: var(--color-primary)'))).toHaveLength(1);
  });

  it('still renders legacy lowercase error values red', async () => {
    const styles = await waterfallStyles(traceBody('error', 'ok'));
    expect(styles.filter((s) => s.includes('background: var(--color-danger)'))).toHaveLength(1);
  });
});

describe('Spec edit tab — LLM-assisted spec editing (ui-layout.md §3)', () => {
  const specEntityWithRepo = {
    type: 'spec',
    id: 'specs/system/auth.md',
    data: {
      name: 'auth.md',
      repo_id: 'repo-abc',
      title: 'Auth Spec',
    },
  };

  const enc = (s) => new TextEncoder().encode(s);

  /** SSE response body from (event, data) pairs — server wire format. */
  function sseResponse(events) {
    const text = events.map(([evt, data]) => `event: ${evt}\ndata: ${JSON.stringify(data)}\n\n`).join('');
    let sent = false;
    return {
      ok: true,
      status: 200,
      body: {
        getReader: () => ({
          read: () => {
            if (sent) return Promise.resolve({ value: undefined, done: true });
            sent = true;
            return Promise.resolve({ value: enc(text), done: false });
          },
        }),
      },
    };
  }

  const specContent = '# Auth\n\n## Existing\nBody.\n';
  const suggestionDiff = [
    { op: 'add', path: '## Existing', content: '## Error Handling\n\nReturn structured errors.' },
  ];

  afterEach(() => vi.unstubAllGlobals());

  async function openEditTab(assistEvents, saveResult) {
    const fetchMock = vi.fn(async (url) => {
      if (String(url).includes('/specs/assist')) return sseResponse(assistEvents);
      if (String(url).includes('/specs/save')) {
        return { ok: true, status: 200, json: async () => saveResult ?? { branch: 'spec-edit/auth-a1b2', mr_id: 'mr-9' } };
      }
      return { ok: true, status: 200, json: async () => ({ content: specContent, current_sha: 'sha-1' }) };
    });
    vi.stubGlobal('fetch', fetchMock);
    render(DetailPanel, { props: { entity: specEntityWithRepo } });
    await fireEvent.click(screen.getByRole('tab', { name: /edit/i }));
    // Wait for the spec content to load into the editor
    await waitFor(() => {
      expect(screen.getByRole('textbox', { name: /spec editor/i }).value).toContain('# Auth');
    });
    return fetchMock;
  }

  it('shows the "Edit spec" recipient indicator chat input in the edit tab (§3 step 1)', async () => {
    await openEditTab([]);
    expect(await screen.findByText('Edit spec: "Auth Spec" ▸')).toBeTruthy();
    expect(screen.getByTestId('llm-input')).toBeTruthy();
    expect(screen.getByTestId('llm-send-btn')).toBeTruthy();
  });

  it('submits the instruction to specs/assist with the spec path and draft content (§3 step 2)', async () => {
    const fetchMock = await openEditTab([
      ['partial', { text: 'Adding error handling.' }],
      ['complete', { diff: suggestionDiff, explanation: 'Adding error handling.' }],
    ]);
    await fireEvent.input(screen.getByTestId('llm-input'), { target: { value: 'add error handling' } });
    await fireEvent.click(screen.getByTestId('llm-send-btn'));
    const assistCall = fetchMock.mock.calls.find((c) => String(c[0]).includes('/specs/assist'));
    expect(assistCall).toBeTruthy();
    expect(assistCall[0]).toBe('/api/v1/repos/repo-abc/specs/assist');
    expect(JSON.parse(assistCall[1].body)).toEqual({
      spec_path: 'specs/system/auth.md',
      instruction: 'add error handling',
      draft_content: specContent,
    });
  });

  it('renders the suggestion as an inline diff block with Accept/Edit/Dismiss (§3 step 3)', async () => {
    await openEditTab([
      ['complete', { diff: suggestionDiff, explanation: 'Adding error handling.' }],
    ]);
    await fireEvent.input(screen.getByTestId('llm-input'), { target: { value: 'add error handling' } });
    await fireEvent.click(screen.getByTestId('llm-send-btn'));
    const block = await screen.findByTestId('llm-suggestion');
    expect(block.textContent).toContain('Error Handling');
    expect(within(block).getByTestId('accept-suggestion')).toBeTruthy();
    expect(within(block).getByTestId('edit-suggestion')).toBeTruthy();
    expect(within(block).getByTestId('dismiss-suggestion')).toBeTruthy();
  });

  it('Accept applies the diff to the edit-tab content in-memory (§3 step 4)', async () => {
    await openEditTab([
      ['complete', { diff: suggestionDiff, explanation: 'Adding error handling.' }],
    ]);
    await fireEvent.input(screen.getByTestId('llm-input'), { target: { value: 'add error handling' } });
    await fireEvent.click(screen.getByTestId('llm-send-btn'));
    await screen.findByTestId('llm-suggestion');
    await fireEvent.click(screen.getByTestId('accept-suggestion'));
    await waitFor(() => expect(screen.queryByTestId('llm-suggestion')).toBeNull());
    const editor = screen.getByRole('textbox', { name: /spec editor/i });
    expect(editor.value).toContain('## Error Handling');
    // In-memory only: no save request until the human clicks Save.
    const saveCalls = global.fetch.mock.calls.filter((c) => String(c[0]).includes('/specs/save'));
    expect(saveCalls).toHaveLength(0);
  });

  it('Dismiss removes the suggestion block without touching the editor (§3 step 6)', async () => {
    await openEditTab([
      ['complete', { diff: suggestionDiff, explanation: 'Adding error handling.' }],
    ]);
    await fireEvent.input(screen.getByTestId('llm-input'), { target: { value: 'add error handling' } });
    await fireEvent.click(screen.getByTestId('llm-send-btn'));
    await screen.findByTestId('llm-suggestion');
    await fireEvent.click(screen.getByTestId('dismiss-suggestion'));
    await waitFor(() => expect(screen.queryByTestId('llm-suggestion')).toBeNull());
    expect(screen.getByRole('textbox', { name: /spec editor/i }).value).toBe(specContent);
  });

  it('Save commits via specs/save and reports the created MR (§3 save workflow)', async () => {
    await openEditTab([]);
    const saveBtn = await screen.findByRole('button', { name: /save & create mr/i });
    await fireEvent.click(saveBtn);
    await waitFor(() => {
      const saveCall = global.fetch.mock.calls.find((c) => String(c[0]).includes('/specs/save'));
      expect(saveCall).toBeTruthy();
      expect(saveCall[0]).toBe('/api/v1/repos/repo-abc/specs/save');
      const body = JSON.parse(saveCall[1].body);
      expect(body.spec_path).toBe('specs/system/auth.md');
      expect(body.content).toBe(specContent);
      expect(typeof body.message).toBe('string');
    });
    // Success surfaces the created MR (toast with the mr_id from the response).
    const { toastSuccess } = await import('../lib/toast.svelte.js');
    await waitFor(() => {
      expect(toastSuccess).toHaveBeenCalledWith(expect.stringContaining('mr-9'));
    });
  });
});

describe('Spec edit tab — meta-spec bindings panel (agent-runtime.md §2)', () => {
  const specEntityWithRepo = {
    type: 'spec',
    id: 'specs/system/auth.md',
    data: {
      name: 'auth.md',
      repo_id: 'repo-abc',
      title: 'Auth Spec',
    },
  };

  const registry = [
    { id: 'ms-req', name: 'security-standard', kind: 'meta:standard', version: 3, required: true },
    { id: 'ms-opt', name: 'backend-developer', kind: 'meta:persona', version: 2, required: false },
    { id: 'ms-opt2', name: 'reviewer', kind: 'meta:persona', version: 5, required: false },
  ];
  // Currently-saved bindings: backend-developer pinned at v1 (stale — v2 exists).
  const currentBindings = [
    { id: 'b-1', spec_id: 'specs/system/auth.md', meta_spec_id: 'ms-opt', pinned_version: 1, created_at: 1 },
  ];

  let putPayloads;

  async function openEditTab() {
    putPayloads = [];
    const fetchMock = vi.fn(async (url, opts) => {
      const u = String(url);
      if (u.includes('/meta-spec-bindings')) {
        if (opts?.method === 'PUT') {
          putPayloads.push(JSON.parse(opts.body));
          return { ok: true, status: 200, json: async () => currentBindings };
        }
        return { ok: true, status: 200, json: async () => currentBindings };
      }
      if (u.includes('/meta-specs')) {
        return { ok: true, status: 200, json: async () => registry };
      }
      return { ok: true, status: 200, json: async () => ({ content: '# Auth\n\nBody.\n', current_sha: 'sha-1' }) };
    });
    vi.stubGlobal('fetch', fetchMock);
    render(DetailPanel, { props: { entity: specEntityWithRepo } });
    await fireEvent.click(screen.getByRole('tab', { name: /edit/i }));
    // Wait for the bindings panel to load.
    await screen.findByRole('checkbox', { name: /bind meta-spec security-standard/i });
    return fetchMock;
  }

  afterEach(() => vi.unstubAllGlobals());

  it('renders required meta-specs pre-selected and locked; optional bindings editable', async () => {
    await openEditTab();
    // Required: pre-selected and locked (cannot be removed).
    const requiredCheck = screen.getByRole('checkbox', { name: /bind meta-spec security-standard/i });
    expect(requiredCheck.checked).toBe(true);
    expect(requiredCheck.disabled).toBe(true);
    // The bound optional meta-spec is selected at its saved pin (v1) with a
    // version selector offering up to the current version (v2).
    const optCheck = screen.getByRole('checkbox', { name: /bind meta-spec backend-developer/i });
    expect(optCheck.checked).toBe(true);
    expect(optCheck.disabled).toBe(false);
    const versionSelect = screen.getByRole('combobox', { name: /pinned version for meta-spec backend-developer/i });
    expect(versionSelect.value).toBe('1');
    // The unbound optional meta-spec is unselected.
    expect(screen.getByRole('checkbox', { name: /bind meta-spec reviewer/i }).checked).toBe(false);
  });

  it('saves the full binding set via the bindings endpoint when the selection changes', async () => {
    await openEditTab();
    // Select the unbound optional meta-spec (defaults to its current version).
    await fireEvent.click(screen.getByRole('checkbox', { name: /bind meta-spec reviewer/i }));
    // Bump the stale backend-developer pin to the current version.
    const versionSelect = screen.getByRole('combobox', { name: /pinned version for meta-spec backend-developer/i });
    await fireEvent.change(versionSelect, { target: { value: '2' } });
    // Save is enabled only when dirty.
    const saveBtn = screen.getByRole('button', { name: /save bindings/i });
    expect(saveBtn.disabled).toBe(false);
    await fireEvent.click(saveBtn);
    await waitFor(() => expect(putPayloads.length).toBe(1));
    // Declarative full-set replace: the payload IS the desired set —
    // required stays in (locked), the bumped pin, and the new selection.
    const bindings = putPayloads[0].bindings;
    expect(bindings).toHaveLength(3);
    const byId = Object.fromEntries(bindings.map((b) => [b.meta_spec_id, b.pinned_version]));
    expect(byId['ms-req']).toBe(3);
    expect(byId['ms-opt']).toBe(2);
    expect(byId['ms-opt2']).toBe(5);
    const { toastSuccess } = await import('../lib/toast.svelte.js');
    await waitFor(() =>
      expect(toastSuccess).toHaveBeenCalledWith(expect.stringContaining('re-approval'))
    );
  });

  it('unbinding an optional meta-spec removes it from the saved set; required cannot be removed', async () => {
    await openEditTab();
    // Uncheck the bound optional meta-spec.
    await fireEvent.click(screen.getByRole('checkbox', { name: /bind meta-spec backend-developer/i }));
    // The locked required checkbox ignores interaction (disabled).
    await fireEvent.click(screen.getByRole('checkbox', { name: /bind meta-spec security-standard/i }));
    await fireEvent.click(screen.getByRole('button', { name: /save bindings/i }));
    await waitFor(() => expect(putPayloads.length).toBe(1));
    const bindings = putPayloads[0].bindings;
    expect(bindings).toHaveLength(1);
    expect(bindings[0].meta_spec_id).toBe('ms-req');
    expect(bindings[0].pinned_version).toBe(3);
  });

  // A non-Admin spec author (Developer member — allowed to save bindings)
  // cannot call the Admin-only flat registry listing, so the panel falls
  // back to the workspace-scoped listing: the same bindable set, with
  // required meta-specs still pre-selected and locked.
  it('falls back to the workspace-scoped listing when the Admin-only registry listing is forbidden', async () => {
    const entity = {
      ...specEntityWithRepo,
      data: { ...specEntityWithRepo.data, workspace_id: 'ws-1' },
    };
    const fetchMock = vi.fn(async (url, opts) => {
      const u = String(url);
      if (u.includes('/meta-spec-bindings')) {
        return { ok: true, status: 200, json: async () => currentBindings };
      }
      // Workspace-scoped listing (checked before the flat listing —
      // its URL also contains "/meta-specs").
      if (u.includes('/workspaces/ws-1/meta-specs')) {
        return { ok: true, status: 200, json: async () => registry };
      }
      // Flat registry listing: Admin-only → 403 for a Developer author.
      if (u.includes('/meta-specs')) {
        return { ok: false, status: 403, json: async () => ({ error: 'forbidden' }) };
      }
      return { ok: true, status: 200, json: async () => ({ content: '# Auth\n\nBody.\n', current_sha: 'sha-1' }) };
    });
    vi.stubGlobal('fetch', fetchMock);
    render(DetailPanel, { props: { entity } });
    await fireEvent.click(screen.getByRole('tab', { name: /edit/i }));
    // The panel resolves available meta-specs from the workspace listing:
    // required pre-selected and locked, the bound optional entry selected.
    await screen.findByRole('checkbox', { name: /bind meta-spec security-standard/i });
    const requiredCheck = screen.getByRole('checkbox', { name: /bind meta-spec security-standard/i });
    expect(requiredCheck.checked).toBe(true);
    expect(requiredCheck.disabled).toBe(true);
    expect(screen.getByRole('checkbox', { name: /bind meta-spec backend-developer/i }).checked).toBe(true);
    // Both surfaces were exercised: the flat listing first (403, caught),
    // then the workspace-scoped fallback.
    const called = fetchMock.mock.calls.map((c) => String(c[0]));
    expect(called.some((u) => u.endsWith('/api/v1/meta-specs'))).toBe(true);
    expect(called.some((u) => u.includes('/api/v1/workspaces/ws-1/meta-specs'))).toBe(true);
  });
});
