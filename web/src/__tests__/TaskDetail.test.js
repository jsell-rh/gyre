import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

// Mock api before importing component
vi.mock('../lib/api.js', () => ({
  api: {
    task: vi.fn(),
  },
  safeHref: vi.fn((url) => url),
}));

import { api } from '../lib/api.js';
import TaskDetail from '../components/TaskDetail.svelte';

// Helper: wrap a value in a delayed promise to avoid Svelte effect_update_depth_exceeded
// when the mock resolves synchronously within an $effect cycle.
const delayed = (val) => new Promise((r) => setTimeout(() => r(val), 10));
const delayedErr = (msg) => new Promise((_, r) => setTimeout(() => r(new Error(msg)), 10));

const TASK_STUB = { id: 'task-42', title: 'Fix auth flow' };

const TASK_DETAIL = {
  id: 'task-42',
  title: 'Fix auth flow',
  status: 'InProgress',
  priority: 'High',
  assigned_to: 'agent-7',
  description: 'The OAuth token refresh is broken in production.',
  labels: ['spec-auth', 'ralph-session-12', 'frontend'],
  pr_link: 'https://github.com/org/repo/pull/99',
  parent_task_id: 'task-10',
  created_at: '2026-03-25T10:00:00Z',
  updated_at: '2026-03-26T14:30:00Z',
};

const TASK_MINIMAL = {
  id: 'task-43',
  title: 'Simple task',
  status: 'Backlog',
  priority: 'Low',
  assigned_to: null,
  description: null,
  labels: [],
  pr_link: null,
  parent_task_id: null,
  created_at: '2026-03-27T00:00:00Z',
  updated_at: '2026-03-27T00:00:00Z',
};

const CTX = new Map([['navigate', vi.fn()]]);
const renderOpts = (props) => ({ props, context: CTX });

beforeEach(() => {
  vi.clearAllMocks();
  api.task.mockImplementation(() => delayed({ ...TASK_DETAIL }));
});

describe('TaskDetail', () => {
  it('renders without throwing', () => {
    expect(() => render(TaskDetail, renderOpts({ task: TASK_STUB }))).not.toThrow();
  });

  it('shows loading skeletons initially', () => {
    api.task.mockImplementation(() => new Promise(() => {})); // never resolves
    const { container } = render(TaskDetail, renderOpts({ task: TASK_STUB }));
    expect(container.querySelector('[aria-busy="true"]')).toBeTruthy();
  });

  it('calls api.task with correct id', async () => {
    render(TaskDetail, renderOpts({ task: TASK_STUB }));
    await waitFor(() => {
      expect(api.task).toHaveBeenCalledWith('task-42');
    });
  });

  it('shows task title after loading', async () => {
    const { findByText } = render(TaskDetail, renderOpts({ task: TASK_STUB }));
    expect(await findByText('Fix auth flow')).toBeTruthy();
  });

  it('shows status and priority badges', async () => {
    const { container } = render(TaskDetail, renderOpts({ task: TASK_STUB }));
    // Badge may format "InProgress" as "In Progress"
    await waitFor(() => {
      const text = container.textContent;
      expect(text).toMatch(/In.?Progress/);
      expect(text).toContain('High');
    });
  });

  it('shows Back button', async () => {
    const { findByText } = render(TaskDetail, renderOpts({ task: TASK_STUB }));
    expect(await findByText(/Back/)).toBeTruthy();
  });

  it('calls onBack when Back button is clicked', async () => {
    const onBack = vi.fn();
    const { findByText } = render(TaskDetail, renderOpts({ task: TASK_STUB, onBack }));
    const btn = await findByText(/Back/);
    await fireEvent.click(btn);
    expect(onBack).toHaveBeenCalled();
  });

  it('shows Info and Artifacts tabs', async () => {
    const { findByText } = render(TaskDetail, renderOpts({ task: TASK_STUB }));
    expect(await findByText('Info')).toBeTruthy();
    expect(await findByText('Artifacts')).toBeTruthy();
  });

  it('shows info fields: ID, Status, Priority, Assigned To', async () => {
    const { findByText } = render(TaskDetail, renderOpts({ task: TASK_STUB }));
    await findByText('Fix auth flow');
    expect(await findByText('task-42')).toBeTruthy();
    expect(await findByText('agent-7')).toBeTruthy();
  });

  it('shows description when present', async () => {
    const { findByText } = render(TaskDetail, renderOpts({ task: TASK_STUB }));
    expect(await findByText(/OAuth token refresh/)).toBeTruthy();
  });

  it('shows labels as pills', async () => {
    const { findAllByText, findByText } = render(TaskDetail, renderOpts({ task: TASK_STUB }));
    // spec-auth and ralph-session-12 appear in both labels and artifacts, so use findAllByText
    const specAuths = await findAllByText('spec-auth');
    expect(specAuths.length).toBeGreaterThan(0);
    const ralphs = await findAllByText('ralph-session-12');
    expect(ralphs.length).toBeGreaterThan(0);
    expect(await findByText('frontend')).toBeTruthy();
  });

  it('shows PR link in Artifacts tab', async () => {
    const { findByText } = render(TaskDetail, renderOpts({ task: TASK_STUB }));
    const artifactsTab = await findByText('Artifacts');
    await fireEvent.click(artifactsTab);
    expect(await findByText('Pull Request')).toBeTruthy();
  });

  it('shows Ralph Loop Refs for spec- and ralph- labels', async () => {
    const { findByText, findAllByText } = render(TaskDetail, renderOpts({ task: TASK_STUB }));
    const artifactsTab = await findByText('Artifacts');
    await fireEvent.click(artifactsTab);
    expect(await findByText('Ralph Loop Refs')).toBeTruthy();
    // These labels also appear in Info tab label pills
    const specAuths = await findAllByText('spec-auth');
    expect(specAuths.length).toBeGreaterThanOrEqual(2); // Info labels + Artifacts refs
    const ralphs = await findAllByText('ralph-session-12');
    expect(ralphs.length).toBeGreaterThanOrEqual(2);
  });

  it('shows "No artifacts" when task has no PR or ralph refs', async () => {
    api.task.mockImplementation(() => delayed({ ...TASK_MINIMAL }));
    const { findByText } = render(TaskDetail, renderOpts({ task: { id: 'task-43' } }));
    const artifactsTab = await findByText('Artifacts');
    await fireEvent.click(artifactsTab);
    expect(await findByText('No artifacts')).toBeTruthy();
  });

  it('shows error state when API fails', async () => {
    api.task.mockImplementation(() => delayedErr('Server error'));
    const { findByText } = render(TaskDetail, renderOpts({ task: TASK_STUB }));
    expect(await findByText('Failed to load task')).toBeTruthy();
    expect(await findByText('Server error')).toBeTruthy();
  });

  it('shows Retry button on error and retries on click', async () => {
    api.task.mockImplementationOnce(() => delayedErr('Network error'));
    const { findByText } = render(TaskDetail, renderOpts({ task: TASK_STUB }));
    const retryBtn = await findByText('Retry');
    api.task.mockImplementation(() => delayed({ ...TASK_DETAIL }));
    await fireEvent.click(retryBtn);
    expect(await findByText('Fix auth flow')).toBeTruthy();
    expect(api.task).toHaveBeenCalledTimes(2);
  });

  it('shows parent task ID when present', async () => {
    const { findByText } = render(TaskDetail, renderOpts({ task: TASK_STUB }));
    expect(await findByText('Parent Task')).toBeTruthy();
    expect(await findByText('task-10')).toBeTruthy();
  });

  it('does not show parent task row when absent', async () => {
    api.task.mockImplementation(() => delayed({ ...TASK_MINIMAL }));
    const { findByText, queryByText } = render(TaskDetail, renderOpts({ task: { id: 'task-43' } }));
    await findByText('Simple task');
    expect(queryByText('Parent Task')).toBeNull();
  });

  it('shows created and updated dates', async () => {
    const { findByText } = render(TaskDetail, renderOpts({ task: TASK_STUB }));
    expect(await findByText('Created')).toBeTruthy();
    expect(await findByText('Updated')).toBeTruthy();
  });

  it('hides assigned row when no assignee', async () => {
    api.task.mockImplementation(() => delayed({ ...TASK_MINIMAL }));
    const { findByText, queryByText } = render(TaskDetail, renderOpts({ task: { id: 'task-43' } }));
    await findByText('Simple task');
    expect(queryByText('Assigned To')).toBeNull();
  });
});
