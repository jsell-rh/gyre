import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';
import RepoDetail from '../components/RepoDetail.svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    repoBranches: vi.fn().mockResolvedValue([
      { name: 'main', sha: 'abc123def456' },
      { name: 'feat/new-feature', sha: '789def012345' },
    ]),
    repoCommits: vi.fn().mockResolvedValue([
      { sha: 'abc123def456', message: 'Initial commit', author: 'alice', timestamp: Math.floor(Date.now() / 1000) - 300 },
      { sha: '789def012345', message: 'Add feature', author: 'bob', timestamp: Math.floor(Date.now() / 1000) - 7200 },
    ]),
    mergeRequests: vi.fn().mockResolvedValue([
      { id: 'mr-1', title: 'Add login page', status: 'Open', author: 'alice', source_branch: 'feat/login', target_branch: 'main' },
    ]),
    repoSpeculative: vi.fn().mockResolvedValue([
      { branch: 'feat/new-feature', has_conflict: false },
    ]),
    repoAgentCommits: vi.fn().mockResolvedValue([
      { sha: 'abc123def456', agent_id: 'agent-001-long-id' },
    ]),
    commitSignature: vi.fn().mockResolvedValue({}),
    repoHotFiles: vi.fn().mockResolvedValue([
      { path: 'src/main.rs', agent_count: 3 },
      { path: 'src/lib.rs', agent_count: 1 },
    ]),
    repoBlame: vi.fn().mockResolvedValue([
      { agent_id: 'agent-001', content: 'fn main() {' },
      { agent_id: null, content: '  println!("hello");' },
    ]),
    repoAbacPolicy: vi.fn().mockResolvedValue([]),
    repoSpecPolicy: vi.fn().mockResolvedValue({
      require_spec_ref: false, require_approved_spec: false,
      warn_stale_spec: false, require_current_spec: false,
    }),
    setRepoSpecPolicy: vi.fn().mockResolvedValue(null),
    setRepoAbacPolicy: vi.fn().mockResolvedValue(null),
    repoGates: vi.fn().mockResolvedValue([
      { id: 'g-1', name: 'lint', gate_type: 'LintCommand', command: 'cargo clippy' },
    ]),
    repoPushGates: vi.fn().mockResolvedValue({ gates: ['conventional-commit'] }),
    createRepoGate: vi.fn().mockResolvedValue({ id: 'g-2', name: 'test', gate_type: 'TestCommand', command: 'cargo test' }),
    deleteRepoGate: vi.fn().mockResolvedValue(null),
    setRepoPushGates: vi.fn().mockResolvedValue({ gates: ['conventional-commit', 'task-ref'] }),
    jjLog: vi.fn().mockResolvedValue([]),
    jjInit: vi.fn().mockResolvedValue(null),
    repoAibom: vi.fn().mockResolvedValue({
      total_commits: 42,
      agents: [
        { id: 'a-1', name: 'worker-1', commit_count: 30, model: 'claude-3', attestation_level: 'server-verified' },
        { id: 'a-2', name: 'worker-2', commit_count: 12, model: null, attestation_level: 'self-reported' },
      ],
      attested_percentage: 71.4,
      aibom_version: '1.0',
      commits: [
        { sha: 'abc123def456', agent_id: 'a-1', task_id: 't-1', ralph_step: 'implement', attestation_level: 'server-verified', timestamp: Math.floor(Date.now() / 1000) - 600 },
      ],
    }),
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
  toastInfo: vi.fn(),
}));

const baseRepo = {
  id: 'repo-1',
  name: 'my-service',
  default_branch: 'main',
};

describe('RepoDetail', () => {
  const onBack = vi.fn();
  const onSelectMr = vi.fn();

  beforeEach(() => {
    onBack.mockClear();
    onSelectMr.mockClear();
  });

  describe('rendering', () => {
    it('renders without throwing', () => {
      expect(() => render(RepoDetail, {
        props: { repo: baseRepo, onBack, onSelectMr },
      })).not.toThrow();
    });

    it('shows the repo name in the breadcrumb', () => {
      const { getByText } = render(RepoDetail, {
        props: { repo: baseRepo, onBack, onSelectMr },
      });
      expect(getByText('my-service')).toBeTruthy();
    });

    it('shows the default branch badge', () => {
      const { getByText } = render(RepoDetail, {
        props: { repo: baseRepo, onBack, onSelectMr },
      });
      expect(getByText('default: main')).toBeTruthy();
    });

    it('shows clone URL with repo id and name', () => {
      const { container } = render(RepoDetail, {
        props: { repo: baseRepo, onBack, onSelectMr },
      });
      const cloneText = container.querySelector('.clone-url-text');
      expect(cloneText.textContent).toContain('/git/repo-1/my-service.git');
    });

    it('shows all tab labels', () => {
      const { getByText } = render(RepoDetail, {
        props: { repo: baseRepo, onBack, onSelectMr },
      });
      expect(getByText('Branches')).toBeTruthy();
      expect(getByText('Commits')).toBeTruthy();
      expect(getByText('Merge Requests')).toBeTruthy();
      expect(getByText('Activity')).toBeTruthy();
      expect(getByText('Policy')).toBeTruthy();
      expect(getByText('Gates')).toBeTruthy();
      expect(getByText('jj')).toBeTruthy();
      expect(getByText('AIBOM')).toBeTruthy();
    });
  });

  describe('navigation', () => {
    it('calls onBack when the back button is clicked', async () => {
      const { getByLabelText } = render(RepoDetail, {
        props: { repo: baseRepo, onBack, onSelectMr },
      });
      await fireEvent.click(getByLabelText('Back to projects list'));
      expect(onBack).toHaveBeenCalledOnce();
    });
  });

  describe('branches tab', () => {
    it('loads and displays branches', async () => {
      const { api } = await import('../lib/api.js');
      const { getByText } = render(RepoDetail, {
        props: { repo: baseRepo, onBack, onSelectMr },
      });

      await waitFor(() => {
        expect(api.repoBranches).toHaveBeenCalledWith('repo-1');
        expect(getByText('main')).toBeTruthy();
        expect(getByText('feat/new-feature')).toBeTruthy();
      });
    });

    it('shows speculative merge status badge for branches with results', async () => {
      const { getByText } = render(RepoDetail, {
        props: { repo: baseRepo, onBack, onSelectMr },
      });

      await waitFor(() => {
        expect(getByText('clean')).toBeTruthy();
      });
    });
  });

  describe('merge requests tab', () => {
    it('loads merge requests and shows them on tab click', async () => {
      const { api } = await import('../lib/api.js');
      const { getByText, container } = render(RepoDetail, {
        props: { repo: baseRepo, onBack, onSelectMr },
      });

      await fireEvent.click(getByText('Merge Requests'));

      await waitFor(() => {
        expect(api.mergeRequests).toHaveBeenCalledWith({ repository_id: 'repo-1' });
        expect(getByText('Add login page')).toBeTruthy();
        // Branch info is combined in one cell
        expect(container.textContent).toContain('feat/login');
      });
    });

    it('calls onSelectMr when clicking a merge request row', async () => {
      const { getByText } = render(RepoDetail, {
        props: { repo: baseRepo, onBack, onSelectMr },
      });

      await fireEvent.click(getByText('Merge Requests'));

      await waitFor(() => expect(getByText('Add login page')).toBeTruthy());
      await fireEvent.click(getByText('Add login page').closest('tr'));

      expect(onSelectMr).toHaveBeenCalledWith(
        expect.objectContaining({ id: 'mr-1', title: 'Add login page' })
      );
    });
  });

  describe('gates tab', () => {
    it('loads and shows existing gates', async () => {
      const { getByText } = render(RepoDetail, {
        props: { repo: baseRepo, onBack, onSelectMr },
      });

      await fireEvent.click(getByText('Gates'));

      await waitFor(() => {
        expect(getByText('lint')).toBeTruthy();
        expect(getByText('cargo clippy')).toBeTruthy();
      });
    });

    it('shows + New Gate button', async () => {
      const { getByText } = render(RepoDetail, {
        props: { repo: baseRepo, onBack, onSelectMr },
      });

      await fireEvent.click(getByText('Gates'));

      await waitFor(() => {
        expect(getByText('+ New Gate')).toBeTruthy();
      });
    });

    it('toggles the gate creation form', async () => {
      const { getByText, container } = render(RepoDetail, {
        props: { repo: baseRepo, onBack, onSelectMr },
      });

      await fireEvent.click(getByText('Gates'));
      await waitFor(() => expect(getByText('+ New Gate')).toBeTruthy());

      await fireEvent.click(getByText('+ New Gate'));

      await waitFor(() => {
        expect(container.querySelector('.gate-form')).toBeTruthy();
        expect(getByText('Cancel')).toBeTruthy();
      });
    });

    it('shows push gates with toggle checkboxes', async () => {
      const { getByText } = render(RepoDetail, {
        props: { repo: baseRepo, onBack, onSelectMr },
      });

      await fireEvent.click(getByText('Gates'));

      await waitFor(() => {
        expect(getByText('conventional-commit')).toBeTruthy();
        expect(getByText('task-ref')).toBeTruthy();
        expect(getByText('no-em-dash')).toBeTruthy();
      });
    });
  });

  describe('AIBOM tab', () => {
    it('loads and shows AIBOM statistics', async () => {
      const { getByRole, container } = render(RepoDetail, {
        props: { repo: baseRepo, onBack, onSelectMr },
      });

      const aibomTab = getByRole('tab', { name: /AIBOM/ });
      await fireEvent.click(aibomTab);

      await waitFor(() => {
        const statValues = Array.from(container.querySelectorAll('.aibom-stat-value')).map(e => e.textContent);
        expect(statValues).toContain('42');
        expect(statValues).toContain('2');
        expect(statValues).toContain('71.4%');
      });
    });

    it('shows agent contribution table', async () => {
      const { getByRole, container } = render(RepoDetail, {
        props: { repo: baseRepo, onBack, onSelectMr },
      });

      const aibomTab = getByRole('tab', { name: /AIBOM/ });
      await fireEvent.click(aibomTab);

      await waitFor(() => {
        expect(container.textContent).toContain('worker-1');
        expect(container.textContent).toContain('worker-2');
        expect(container.textContent).toContain('claude-3');
      });
    });
  });

  describe('policy tab', () => {
    it('loads policy settings on tab switch', async () => {
      const { api } = await import('../lib/api.js');
      const { getByText } = render(RepoDetail, {
        props: { repo: baseRepo, onBack, onSelectMr },
      });

      await fireEvent.click(getByText('Policy'));

      await waitFor(() => {
        expect(api.repoAbacPolicy).toHaveBeenCalledWith('repo-1');
        expect(api.repoSpecPolicy).toHaveBeenCalledWith('repo-1');
        expect(getByText('Spec Policy')).toBeTruthy();
        expect(getByText('ABAC Policies')).toBeTruthy();
      });
    });

    it('shows save button for spec policy', async () => {
      const { getByText } = render(RepoDetail, {
        props: { repo: baseRepo, onBack, onSelectMr },
      });

      await fireEvent.click(getByText('Policy'));

      await waitFor(() => {
        expect(getByText('Save Spec Policy')).toBeTruthy();
      });
    });

    it('saves spec policy on button click', async () => {
      const { api } = await import('../lib/api.js');
      const { toastSuccess } = await import('../lib/toast.svelte.js');
      const { getByText } = render(RepoDetail, {
        props: { repo: baseRepo, onBack, onSelectMr },
      });

      await fireEvent.click(getByText('Policy'));
      await waitFor(() => expect(getByText('Save Spec Policy')).toBeTruthy());

      await fireEvent.click(getByText('Save Spec Policy'));

      await waitFor(() => {
        expect(api.setRepoSpecPolicy).toHaveBeenCalledWith('repo-1', expect.objectContaining({
          require_spec_ref: false,
        }));
        expect(toastSuccess).toHaveBeenCalledWith('Spec policy saved.');
      });
    });
  });

  describe('error handling', () => {
    it('shows error message when branches fail to load', async () => {
      const { api } = await import('../lib/api.js');
      api.repoBranches.mockRejectedValueOnce(new Error('Network error'));

      const { getByText, getByRole } = render(RepoDetail, {
        props: { repo: baseRepo, onBack, onSelectMr },
      });

      await waitFor(() => {
        expect(getByRole('alert')).toBeTruthy();
        expect(getByText('Retry')).toBeTruthy();
      });
    });
  });
});

describe('RepoDetail pure functions', () => {
  // Test the pure helper functions by rendering and checking their output

  it('shortSha truncates SHA to 8 characters', async () => {
    const { container } = render(RepoDetail, {
      props: {
        repo: baseRepo,
        onBack: vi.fn(),
        onSelectMr: vi.fn(),
      },
    });

    await waitFor(() => {
      const shaElements = container.querySelectorAll('.sha');
      if (shaElements.length > 0) {
        // SHA should be 8 chars (abc123de from abc123def456)
        expect(shaElements[0].textContent.length).toBe(8);
      }
    });
  });

  it('relativeTime formats timestamps correctly in the commit display', async () => {
    const { api } = await import('../lib/api.js');
    // Override with a commit from 30 seconds ago
    api.repoCommits.mockResolvedValueOnce([
      { sha: 'aaa111bbb222', message: 'Recent', author: 'dev', timestamp: Math.floor(Date.now() / 1000) - 30 },
    ]);

    const { getByText, container } = render(RepoDetail, {
      props: {
        repo: baseRepo,
        onBack: vi.fn(),
        onSelectMr: vi.fn(),
      },
    });

    // Switch to commits tab
    await fireEvent.click(getByText('Commits'));

    await waitFor(() => {
      const cells = container.querySelectorAll('.secondary-cell');
      const timeTexts = Array.from(cells).map(c => c.textContent);
      // Should show "30s ago" or similar
      const hasRelativeTime = timeTexts.some(t => /\d+s ago/.test(t));
      expect(hasRelativeTime).toBe(true);
    });
  });
});
