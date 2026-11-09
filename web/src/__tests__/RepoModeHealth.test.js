import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

// Mock child components to isolate RepoMode testing (same set as
// RepoMode.test.js — none of them render on the MRs tab, but they are
// imported at module level).
vi.mock('../components/ExplorerView.svelte', () => ({ default: function Stub() {} }));
vi.mock('../components/SpecDashboard.svelte', () => ({ default: function Stub() {} }));
vi.mock('../components/Inbox.svelte', () => ({ default: function Stub() {} }));
vi.mock('../components/ExplorerCodeTab.svelte', () => ({ default: function Stub() {} }));
vi.mock('../components/RepoDependencies.svelte', () => ({ default: function Stub() {} }));
vi.mock('../components/RepoSettings.svelte', () => ({
  default: function RepoSettingsStub(opts) {
    const el = document.createElement('div');
    el.setAttribute('data-testid', 'repo-settings');
    if (opts?.target) opts.target.appendChild(el);
    return { destroy() {} };
  },
}));

vi.mock('../lib/api.js', () => ({
  api: {
    myNotifications: vi.fn(),
    agents: vi.fn(),
    repoDependencies: vi.fn(),
    repoDependents: vi.fn(),
    mergeRequests: vi.fn(),
    mrGates: vi.fn(),
    mrDiff: vi.fn(),
    repoSpeculative: vi.fn(),
    mergeQueue: vi.fn(),
    repoStatus: vi.fn(),
    pauseRepoQueue: vi.fn(),
    resumeRepoQueue: vi.fn(),
  },
}));

import { api } from '../lib/api.js';
import RepoMode from '../components/RepoMode.svelte';

// Repo health + merge queue pause state — platform-model.md §6 UI:
// "Repo detail page shows main branch health (green/red indicator)" and
// "Merge queue shows paused state with reason".
describe('RepoMode — main health + merge queue pause state', () => {
  const ws = { id: 'ws-1', name: 'Payments' };
  const repo = { id: 'repo-1', name: 'payment-api' };

  const greenStatus = {
    repo_id: 'repo-1',
    queue_paused: false,
    pause_reason: null,
    main_green: true,
    post_merge_gates: [
      { name: 'post-merge-tests', gate_type: 'test_command', command: 'cargo test', required: true, timeout_secs: 300 },
    ],
  };

  beforeEach(() => {
    vi.clearAllMocks();
    api.myNotifications.mockResolvedValue([]);
    api.agents.mockResolvedValue([]);
    api.repoDependencies.mockResolvedValue([]);
    api.repoDependents.mockResolvedValue([]);
    api.mergeRequests.mockResolvedValue([]);
    api.mrGates.mockResolvedValue([]);
    api.mrDiff.mockResolvedValue({});
    api.repoSpeculative.mockResolvedValue([]);
    api.mergeQueue.mockResolvedValue([]);
    api.repoStatus.mockResolvedValue(greenStatus);
    api.pauseRepoQueue.mockResolvedValue({
      repo_id: 'repo-1', queue_paused: true, pause_reason: 'paused via web UI', main_green: null, post_merge_gates: [],
    });
    api.resumeRepoQueue.mockResolvedValue({
      repo_id: 'repo-1', queue_paused: false, pause_reason: null, main_green: null, post_merge_gates: [],
    });
  });

  it('shows a green main-health badge when post-merge validation passes', async () => {
    const { container } = render(RepoMode, {
      props: { workspace: ws, repo, activeTab: 'mrs' },
    });

    await waitFor(() => {
      const badge = container.querySelector('[data-testid="main-health-badge"]');
      expect(badge).toBeTruthy();
    });
    const badge = container.querySelector('[data-testid="main-health-badge"]');
    expect(badge.classList.contains('main-health-green')).toBe(true);
    expect(badge.classList.contains('main-health-red')).toBe(false);
    expect(badge.textContent).toContain('main: green');
    expect(api.repoStatus).toHaveBeenCalledWith('repo-1');
  });

  it('shows a red main-health badge and broken-main text when post-merge validation fails', async () => {
    api.repoStatus.mockResolvedValue({ ...greenStatus, main_green: false });

    const { container } = render(RepoMode, {
      props: { workspace: ws, repo, activeTab: 'mrs' },
    });

    await waitFor(() => {
      const badge = container.querySelector('[data-testid="main-health-badge"]');
      expect(badge).toBeTruthy();
      expect(badge.classList.contains('main-health-red')).toBe(true);
    });
    expect(container.querySelector('[data-testid="main-health-badge"]').textContent)
      .toContain('main: broken');
    const health = container.querySelector('[data-testid="queue-state-health"]');
    expect(health.textContent).toContain('Main is broken');
  });

  it('shows an unknown (muted) badge when no post-merge gates are configured', async () => {
    api.repoStatus.mockResolvedValue({
      repo_id: 'repo-1', queue_paused: false, pause_reason: null, main_green: null, post_merge_gates: [],
    });

    const { container } = render(RepoMode, {
      props: { workspace: ws, repo, activeTab: 'mrs' },
    });

    await waitFor(() => {
      const badge = container.querySelector('[data-testid="main-health-badge"]');
      expect(badge).toBeTruthy();
      expect(badge.classList.contains('main-health-unknown')).toBe(true);
    });
    expect(container.querySelector('[data-testid="queue-state-health"]').textContent)
      .toContain('No post-merge gates configured');
  });

  it('shows the paused banner with reason and a resume button when the queue is paused', async () => {
    api.repoStatus.mockResolvedValue({
      ...greenStatus, main_green: false, queue_paused: true, pause_reason: 'post-merge validation failed',
    });

    const { container } = render(RepoMode, {
      props: { workspace: ws, repo, activeTab: 'mrs' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="queue-paused-reason"]')).toBeTruthy();
    });
    const reason = container.querySelector('[data-testid="queue-paused-reason"]');
    expect(reason.getAttribute('role')).toBe('status');
    expect(reason.textContent).toContain('Merge queue paused');
    expect(reason.textContent).toContain('post-merge validation failed');
    // Paused → resume offered, pause not offered.
    expect(container.querySelector('[data-testid="queue-resume-btn"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="queue-pause-btn"]')).toBeNull();
    expect(container.querySelector('[data-testid="queue-running"]')).toBeNull();
  });

  it('shows running state with a pause button when the queue is not paused', async () => {
    const { container } = render(RepoMode, {
      props: { workspace: ws, repo, activeTab: 'mrs' },
    });

    await waitFor(() => {
      expect(container.querySelector('[data-testid="queue-running"]')).toBeTruthy();
    });
    expect(container.querySelector('[data-testid="queue-pause-btn"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="queue-resume-btn"]')).toBeNull();
    expect(container.querySelector('[data-testid="queue-paused-reason"]')).toBeNull();
  });

  it('pause button pauses the queue and flips the banner to paused', async () => {
    const { container } = render(RepoMode, {
      props: { workspace: ws, repo, activeTab: 'mrs' },
    });

    const pauseBtn = await waitFor(() => {
      const btn = container.querySelector('[data-testid="queue-pause-btn"]');
      expect(btn).toBeTruthy();
      return btn;
    });
    await fireEvent.click(pauseBtn);

    expect(api.pauseRepoQueue).toHaveBeenCalledWith('repo-1', expect.any(String));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="queue-paused-reason"]')).toBeTruthy();
    });
    // The health reading from the initial status fetch is preserved — the
    // pause response does not re-run gates (main_green: null).
    expect(container.querySelector('[data-testid="main-health-badge"]').classList.contains('main-health-green')).toBe(true);
  });

  it('resume button resumes a paused queue', async () => {
    api.repoStatus.mockResolvedValue({
      ...greenStatus, queue_paused: true, pause_reason: 'post-merge validation failed',
    });

    const { container } = render(RepoMode, {
      props: { workspace: ws, repo, activeTab: 'mrs' },
    });

    const resumeBtn = await waitFor(() => {
      const btn = container.querySelector('[data-testid="queue-resume-btn"]');
      expect(btn).toBeTruthy();
      return btn;
    });
    await fireEvent.click(resumeBtn);

    expect(api.resumeRepoQueue).toHaveBeenCalledWith('repo-1');
    await waitFor(() => {
      expect(container.querySelector('[data-testid="queue-running"]')).toBeTruthy();
    });
  });

  it('scopes merge queue entries to this repo via the MR join', async () => {
    // The /merge-queue response carries no repo_id — entries must be scoped
    // by joining with this repo's MRs, not by a field the API never sends.
    api.mergeQueue.mockResolvedValue([
      { id: 'e1', merge_request_id: 'mr-1', priority: 50, status: 'queued' },
      { id: 'e2', merge_request_id: 'mr-other-repo', priority: 50, status: 'queued' },
    ]);
    api.mergeRequests.mockResolvedValue([
      { id: 'mr-1', title: 'Add auth middleware', status: 'open', source_branch: 'feat/auth', target_branch: 'main' },
    ]);

    const { container } = render(RepoMode, {
      props: { workspace: ws, repo, activeTab: 'mrs' },
    });

    await waitFor(() => {
      const section = container.querySelector('.merge-queue-section');
      expect(section).toBeTruthy();
      expect(section.textContent).toContain('Merge Queue (1)');
    });
    const section = container.querySelector('.merge-queue-section');
    expect(section.textContent).toContain('Add auth middleware');
    expect(section.textContent).not.toContain('mr-other-repo');
  });
});
