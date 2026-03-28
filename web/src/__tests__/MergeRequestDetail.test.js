import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

const mockMr = {
  id: 'mr-1',
  title: 'Add authentication module',
  status: 'open',
  source_branch: 'feat/auth',
  target_branch: 'main',
  author_agent_id: 'agent-42',
  created_at: Math.floor(Date.now() / 1000) - 3600,
  has_conflicts: false,
  spec_ref: 'specs/auth/login.md@abc12345',
  atomic_group: null,
  diff_stats: { files_changed: 3, insertions: 120, deletions: 15 },
};

const mockReviews = [
  { id: 'rev-1', reviewer_agent_id: 'reviewer-1', decision: 'approved', body: 'Looks good!', created_at: Math.floor(Date.now() / 1000) - 1800 },
];

const mockComments = [
  { id: 'cmt-1', author_agent_id: 'agent-42', body: 'Ready for review', file_path: 'src/auth.rs', line_number: 42, created_at: Math.floor(Date.now() / 1000) - 900 },
];

const mockGates = [
  { id: 'gate-1', gate_id: 'ci/build', status: 'passed' },
  { id: 'gate-2', gate_id: 'ci/lint', status: 'failed' },
];

const mockDeps = { depends_on: ['mr-2'], dependents: ['mr-3'] };

const mockDiff = {
  files: [
    {
      path: 'src/auth.rs',
      status: 'Modified',
      hunks: [
        {
          header: '@@ -1,5 +1,10 @@',
          lines: [
            { type: 'context', content: 'use std::collections::HashMap;' },
            { type: 'add', content: 'use crate::jwt::validate;' },
            { type: 'delete', content: '// TODO: add auth' },
          ],
        },
      ],
    },
    {
      path: 'src/jwt.rs',
      status: 'Added',
      hunks: [],
    },
  ],
};

vi.mock('../lib/api.js', () => ({
  api: {
    mrReviews: vi.fn().mockResolvedValue([]),
    mrComments: vi.fn().mockResolvedValue([]),
    mrGates: vi.fn().mockResolvedValue([]),
    mrDependencies: vi.fn().mockResolvedValue(null),
    mrDiff: vi.fn().mockResolvedValue(null),
    mergeRequest: vi.fn().mockResolvedValue(null),
    submitReview: vi.fn().mockResolvedValue(null),
    enqueue: vi.fn().mockResolvedValue(null),
    setMrDependencies: vi.fn().mockResolvedValue(null),
    removeMrDependency: vi.fn().mockResolvedValue(null),
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));

// Mock svelte's getContext to return a navigate stub
vi.mock('svelte', async (importOriginal) => {
  const actual = await importOriginal();
  return {
    ...actual,
    getContext: vi.fn().mockReturnValue(vi.fn()),
  };
});

import { api } from '../lib/api.js';
import { toastSuccess, toastError } from '../lib/toast.svelte.js';
import MergeRequestDetail from '../components/MergeRequestDetail.svelte';

describe('MergeRequestDetail', () => {
  const onBack = vi.fn();
  const repo = { id: 'repo-1', name: 'my-service' };

  beforeEach(() => {
    vi.clearAllMocks();
    api.mrReviews.mockResolvedValue([...mockReviews]);
    api.mrComments.mockResolvedValue([...mockComments]);
    api.mrGates.mockResolvedValue([...mockGates]);
    api.mrDependencies.mockResolvedValue({ ...mockDeps, depends_on: [...mockDeps.depends_on], dependents: [...mockDeps.dependents] });
    api.mrDiff.mockResolvedValue(JSON.parse(JSON.stringify(mockDiff)));
  });

  it('renders without throwing', () => {
    expect(() =>
      render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } })
    ).not.toThrow();
  });

  it('shows MR title in header', () => {
    const { getByText } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    expect(getByText('Add authentication module')).toBeTruthy();
  });

  it('shows repo name in back button', () => {
    const { getByText } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    expect(getByText('my-service')).toBeTruthy();
  });

  it('calls onBack when back button is clicked', async () => {
    const { container } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    const backBtn = container.querySelector('.back-btn');
    await fireEvent.click(backBtn);
    expect(onBack).toHaveBeenCalledTimes(1);
  });

  it('shows branch info', () => {
    const { getByText } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    expect(getByText(/feat\/auth/)).toBeTruthy();
  });

  it('shows Overview and Files tabs', () => {
    const { getByText } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    expect(getByText('Overview')).toBeTruthy();
    expect(getByText('Files')).toBeTruthy();
  });

  it('shows diff stats file count in Files tab badge', () => {
    const { container } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    const badge = container.querySelector('.tab-badge');
    expect(badge.textContent).toBe('3');
  });

  it('shows Approve and Request Changes action buttons', () => {
    const { getByText } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    expect(getByText('Approve')).toBeTruthy();
    expect(getByText('Request Changes')).toBeTruthy();
  });

  it('shows timeline steps', () => {
    const { getByText } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    expect(getByText('created')).toBeTruthy();
    expect(getByText('reviewed')).toBeTruthy();
    expect(getByText('approved')).toBeTruthy();
    expect(getByText('queued')).toBeTruthy();
    expect(getByText('merged')).toBeTruthy();
  });

  it('shows reviews section after loading', async () => {
    const { findByText } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    expect(await findByText('reviewer-1')).toBeTruthy();
    expect(await findByText('Looks good!')).toBeTruthy();
  });

  it('shows comments section after loading', async () => {
    const { findByText } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    expect(await findByText('Ready for review')).toBeTruthy();
    expect(await findByText('src/auth.rs:42')).toBeTruthy();
  });

  it('shows quality gates in sidebar', async () => {
    const { findByText } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    expect(await findByText('ci/build')).toBeTruthy();
    expect(await findByText('ci/lint')).toBeTruthy();
  });

  it('shows conflicts status as None', () => {
    const { getByText } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    expect(getByText('None')).toBeTruthy();
  });

  it('shows spec binding when spec_ref is set', () => {
    const { container } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    expect(container.textContent).toContain('specs/auth/login.md');
    expect(container.textContent).toContain('abc12345');
  });

  it('shows diff stats (insertions/deletions)', () => {
    const { getByText } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    expect(getByText('+120')).toBeTruthy();
    expect(getByText('-15')).toBeTruthy();
    expect(getByText('3 files')).toBeTruthy();
  });

  it('shows dependency list after loading', async () => {
    const { findByText, container } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    // Dependencies sidebar card
    await waitFor(() => {
      expect(container.textContent).toContain('Dependencies');
    });
  });

  it('shows "Required by" section for dependents', async () => {
    const { findByText } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    expect(await findByText('Required by')).toBeTruthy();
  });

  it('shows No reviews empty state when none exist', async () => {
    api.mrReviews.mockResolvedValue([]);
    const { findByText } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    expect(await findByText('No reviews yet')).toBeTruthy();
  });

  it('shows No comments empty state when none exist', async () => {
    api.mrComments.mockResolvedValue([]);
    const { findByText } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    expect(await findByText('No comments yet')).toBeTruthy();
  });

  it('shows No dependencies text when depends_on is empty', async () => {
    api.mrDependencies.mockResolvedValue({ depends_on: [], dependents: [] });
    const { findByText } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    expect(await findByText('No dependencies')).toBeTruthy();
  });

  it('disables Approve and Request Changes when MR is merged', () => {
    const mergedMr = { ...mockMr, status: 'merged' };
    const { container } = render(MergeRequestDetail, { props: { mr: mergedMr, repo, onBack } });
    const approveBtn = container.querySelector('.action-btn.approve');
    const changesBtn = container.querySelector('.action-btn.changes');
    expect(approveBtn.disabled).toBe(true);
    expect(changesBtn.disabled).toBe(true);
  });

  it('shows Add to Queue button when status is approved', () => {
    const approvedMr = { ...mockMr, status: 'approved' };
    const { getByText } = render(MergeRequestDetail, { props: { mr: approvedMr, repo, onBack } });
    expect(getByText('Add to Queue')).toBeTruthy();
  });

  it('does not show Add to Queue button when status is open', () => {
    const { container } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    const enqueueBtn = container.querySelector('.action-btn.enqueue');
    expect(enqueueBtn).toBeNull();
  });

  it('calls submitReview on Approve click', async () => {
    api.submitReview.mockResolvedValue({ id: 'rev-new', reviewer_agent_id: 'dashboard', decision: 'approved', body: null, created_at: 0 });
    api.mergeRequest.mockResolvedValue({ ...mockMr, status: 'approved' });
    const { getByText } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });

    // Wait for details to load
    await waitFor(() => expect(api.mrReviews).toHaveBeenCalled());

    await fireEvent.click(getByText('Approve'));

    await waitFor(() => {
      expect(api.submitReview).toHaveBeenCalledWith('mr-1', {
        reviewer_agent_id: 'dashboard',
        decision: 'approved',
      });
      expect(toastSuccess).toHaveBeenCalledWith('MR approved.');
    });
  });

  it('calls submitReview on Request Changes click', async () => {
    api.submitReview.mockResolvedValue({ id: 'rev-new', reviewer_agent_id: 'dashboard', decision: 'changes_requested', body: null, created_at: 0 });
    api.mergeRequest.mockResolvedValue({ ...mockMr });
    const { getByText } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });

    await waitFor(() => expect(api.mrReviews).toHaveBeenCalled());

    await fireEvent.click(getByText('Request Changes'));

    await waitFor(() => {
      expect(api.submitReview).toHaveBeenCalledWith('mr-1', {
        reviewer_agent_id: 'dashboard',
        decision: 'changes_requested',
      });
      expect(toastSuccess).toHaveBeenCalledWith('Changes requested.');
    });
  });

  it('shows error toast when loadDetails fails', async () => {
    api.mrReviews.mockRejectedValue(new Error('Network error'));
    render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });

    await waitFor(() => {
      expect(toastError).toHaveBeenCalledWith(expect.stringContaining('Network error'));
    });
  });

  it('switches to Files tab and loads diff', async () => {
    const { getByText, findByText, container } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });

    await fireEvent.click(getByText('Files'));

    await waitFor(() => {
      expect(api.mrDiff).toHaveBeenCalledWith('mr-1');
    });

    expect(await findByText('Changed Files')).toBeTruthy();
    // File list items appear in the file sidebar
    await waitFor(() => {
      const fileItems = container.querySelectorAll('.file-item');
      expect(fileItems.length).toBe(2);
      expect(fileItems[0].textContent).toContain('src/auth.rs');
      expect(fileItems[1].textContent).toContain('src/jwt.rs');
    });
  });

  it('shows diff lines with add/delete styling', async () => {
    const { getByText, container } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });

    await fireEvent.click(getByText('Files'));
    await waitFor(() => expect(api.mrDiff).toHaveBeenCalled());

    await waitFor(() => {
      const addLines = container.querySelectorAll('.line-add');
      const deleteLines = container.querySelectorAll('.line-delete');
      expect(addLines.length).toBeGreaterThan(0);
      expect(deleteLines.length).toBeGreaterThan(0);
    });
  });

  it('shows hunk header in diff view', async () => {
    const { getByText, findByText } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    await fireEvent.click(getByText('Files'));
    expect(await findByText('@@ -1,5 +1,10 @@')).toBeTruthy();
  });

  it('shows empty diff state when no files changed', async () => {
    api.mrDiff.mockResolvedValue({ files: [] });
    const { getByText, findByText } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    await fireEvent.click(getByText('Files'));
    expect(await findByText('No files changed')).toBeTruthy();
  });

  it('shows file status dots with correct classes', async () => {
    const { getByText, container } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    await fireEvent.click(getByText('Files'));

    await waitFor(() => {
      const dots = container.querySelectorAll('.file-status-dot');
      expect(dots.length).toBe(2);
      // First file is Modified, second is Added
      expect(dots[0].classList.contains('modified')).toBe(true);
      expect(dots[1].classList.contains('added')).toBe(true);
    });
  });

  it('clicking a file in the sidebar selects it', async () => {
    const { getByText, container } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    await fireEvent.click(getByText('Files'));

    await waitFor(() => {
      const fileItems = container.querySelectorAll('.file-item');
      expect(fileItems.length).toBe(2);
    });

    // Click the second file
    const fileItems = container.querySelectorAll('.file-item');
    await fireEvent.click(fileItems[1]);

    await waitFor(() => {
      expect(fileItems[1].classList.contains('selected')).toBe(true);
    });
  });

  it('has accessible tab role attributes', () => {
    const { container } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    const tabs = container.querySelectorAll('[role="tab"]');
    expect(tabs.length).toBe(2);
    expect(tabs[0].getAttribute('aria-selected')).toBe('true');
    expect(tabs[1].getAttribute('aria-selected')).toBe('false');
  });

  it('has accessible tabpanel with aria-busy', () => {
    const { container } = render(MergeRequestDetail, { props: { mr: { ...mockMr }, repo, onBack } });
    const tabpanel = container.querySelector('[role="tabpanel"]');
    expect(tabpanel).toBeTruthy();
    expect(tabpanel.id).toBe('tabpanel-overview');
  });
});
