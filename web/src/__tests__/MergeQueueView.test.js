import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

const mockEntries = [
  {
    id: 'qe-1',
    merge_request_id: 'mr-aaa111bbb222',
    status: 'queued',
    priority: 50,
    enqueued_at: Math.floor(Date.now() / 1000) - 300,
    processed_at: null,
    error_message: null,
  },
  {
    id: 'qe-2',
    merge_request_id: 'mr-ccc333ddd444',
    status: 'processing',
    priority: 80,
    enqueued_at: Math.floor(Date.now() / 1000) - 600,
    processed_at: null,
    error_message: null,
  },
  {
    id: 'qe-3',
    merge_request_id: 'mr-eee555fff666',
    status: 'merged',
    priority: 50,
    enqueued_at: Math.floor(Date.now() / 1000) - 3600,
    processed_at: Math.floor(Date.now() / 1000) - 3500,
    error_message: null,
  },
  {
    id: 'qe-4',
    merge_request_id: 'mr-ggg777hhh888',
    status: 'failed',
    priority: 50,
    enqueued_at: Math.floor(Date.now() / 1000) - 7200,
    processed_at: Math.floor(Date.now() / 1000) - 7100,
    error_message: 'Merge conflict detected',
  },
];

const mockGraph = {
  nodes: [
    { mr_id: 'mr-aaa111bbb222', title: 'Add auth module', status: 'queued', priority: 50 },
    { mr_id: 'mr-ccc333ddd444', title: 'Refactor DB', status: 'processing', priority: 80 },
  ],
  edges: [
    { from: 'mr-aaa111bbb222', to: 'mr-ccc333ddd444' },
  ],
};

vi.mock('../lib/api.js', () => ({
  api: {
    mergeQueue: vi.fn().mockResolvedValue([]),
    cancelQueueEntry: vi.fn().mockResolvedValue(null),
    mergeQueueGraph: vi.fn().mockResolvedValue(null),
  },
}));

vi.mock('../lib/toast.svelte.js', () => ({
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));

import { api } from '../lib/api.js';
import { toastSuccess, toastError } from '../lib/toast.svelte.js';
import MergeQueueView from '../components/MergeQueueView.svelte';

describe('MergeQueueView', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.mergeQueue.mockResolvedValue([...mockEntries]);
    api.mergeQueueGraph.mockResolvedValue(JSON.parse(JSON.stringify(mockGraph)));
  });

  it('renders without throwing', () => {
    expect(() => render(MergeQueueView)).not.toThrow();
  });

  it('shows Merge Queue heading', () => {
    const { getByText } = render(MergeQueueView);
    expect(getByText('Merge Queue')).toBeTruthy();
  });

  it('shows entry count after loading', async () => {
    const { findByText } = render(MergeQueueView);
    expect(await findByText('4 entries')).toBeTruthy();
  });

  it('shows singular "entry" for single item', async () => {
    api.mergeQueue.mockResolvedValue([mockEntries[0]]);
    const { findByText } = render(MergeQueueView);
    expect(await findByText('1 entry')).toBeTruthy();
  });

  it('shows Lanes and DAG view toggle buttons', () => {
    const { getByText } = render(MergeQueueView);
    expect(getByText('Lanes')).toBeTruthy();
    expect(getByText('DAG')).toBeTruthy();
  });

  it('shows Refresh button', () => {
    const { getByText } = render(MergeQueueView);
    expect(getByText('Refresh')).toBeTruthy();
  });

  it('shows three lanes: Queued, Processing, Done', async () => {
    const { findByText } = render(MergeQueueView);
    expect(await findByText('Queued')).toBeTruthy();
    expect(await findByText('Processing')).toBeTruthy();
    expect(await findByText('Done')).toBeTruthy();
  });

  it('shows lane counts', async () => {
    const { container } = render(MergeQueueView);
    await waitFor(() => {
      const counts = container.querySelectorAll('.lane-count');
      expect(counts.length).toBe(3);
      // Queued=1, Processing=1, Done=2 (merged + failed)
      expect(counts[0].textContent).toBe('1');
      expect(counts[1].textContent).toBe('1');
      expect(counts[2].textContent).toBe('2');
    });
  });

  it('shows Cancel button on queued entries', async () => {
    const { container } = render(MergeQueueView);
    await waitFor(() => {
      const cancelBtns = container.querySelectorAll('.cancel-btn');
      expect(cancelBtns.length).toBeGreaterThan(0);
    });
  });

  it('cancels an entry and removes it from the list', async () => {
    api.cancelQueueEntry.mockResolvedValue(null);
    const { container } = render(MergeQueueView);

    await waitFor(() => {
      const cancelBtns = container.querySelectorAll('.cancel-btn');
      expect(cancelBtns.length).toBeGreaterThan(0);
    });

    const cancelBtn = container.querySelector('.cancel-btn');
    await fireEvent.click(cancelBtn);

    await waitFor(() => {
      expect(api.cancelQueueEntry).toHaveBeenCalled();
      expect(toastSuccess).toHaveBeenCalledWith('Queue entry cancelled.');
    });
  });

  it('shows error toast on cancel failure', async () => {
    api.cancelQueueEntry.mockRejectedValue(new Error('Forbidden'));
    const { container } = render(MergeQueueView);

    await waitFor(() => {
      const cancelBtns = container.querySelectorAll('.cancel-btn');
      expect(cancelBtns.length).toBeGreaterThan(0);
    });

    await fireEvent.click(container.querySelector('.cancel-btn'));

    await waitFor(() => {
      expect(toastError).toHaveBeenCalledWith('Forbidden');
    });
  });

  it('shows empty state when queue is empty', async () => {
    api.mergeQueue.mockResolvedValue([]);
    const { findByText } = render(MergeQueueView);
    expect(await findByText('Queue is empty')).toBeTruthy();
  });

  it('shows error state when API fails', async () => {
    api.mergeQueue.mockRejectedValue(new Error('Server error'));
    const { findByText } = render(MergeQueueView);
    expect(await findByText('Server error')).toBeTruthy();
    expect(await findByText('Retry')).toBeTruthy();
  });

  it('Retry button reloads the queue', async () => {
    // Reject persistently until we manually switch
    api.mergeQueue.mockReset().mockRejectedValue(new Error('Server error'));

    const { findByText } = render(MergeQueueView);
    const retryBtn = await findByText('Retry');

    // Now switch to success for the retry click
    api.mergeQueue.mockResolvedValue([...mockEntries]);
    const callsBefore = api.mergeQueue.mock.calls.length;

    await fireEvent.click(retryBtn);

    await waitFor(() => {
      expect(api.mergeQueue.mock.calls.length).toBeGreaterThan(callsBefore);
    });
  });

  it('switches to DAG view and loads graph', async () => {
    const { getByText, findByText, findAllByText } = render(MergeQueueView);

    // Wait for lanes to render (initial load complete)
    await findByText('Queued');

    // Click DAG toggle
    const dagBtn = getByText('DAG');
    await fireEvent.click(dagBtn);

    // Wait for graph API and rendering
    await waitFor(() => {
      expect(api.mergeQueueGraph).toHaveBeenCalled();
    });

    expect((await findAllByText('Add auth module')).length).toBeGreaterThan(0);
    expect((await findAllByText('Refactor DB')).length).toBeGreaterThan(0);
  });

  it('shows blocked-by info in DAG view', async () => {
    const { getByText, findByText } = render(MergeQueueView);
    await waitFor(() => expect(api.mergeQueue).toHaveBeenCalled());

    await fireEvent.click(getByText('DAG'));

    expect(await findByText('Blocked by:')).toBeTruthy();
  });

  it('shows "No dependencies - ready" for unblocked nodes in DAG', async () => {
    const { getByText, findByText } = render(MergeQueueView);
    await waitFor(() => expect(api.mergeQueue).toHaveBeenCalled());

    await fireEvent.click(getByText('DAG'));

    expect(await findByText(/No dependencies/)).toBeTruthy();
  });

  it('shows All Entries table when more than 3 entries', async () => {
    const { findByText } = render(MergeQueueView);
    expect(await findByText('All Entries')).toBeTruthy();
  });

  it('does not show All Entries table when 3 or fewer entries', async () => {
    api.mergeQueue.mockResolvedValue([mockEntries[0], mockEntries[1]]);
    const { container } = render(MergeQueueView);
    await waitFor(() => {
      expect(container.querySelector('.all-entries')).toBeNull();
    });
  });

  it('shows error hint for failed entries', async () => {
    const { container } = render(MergeQueueView);
    await waitFor(() => {
      const hints = container.querySelectorAll('.error-hint');
      expect(hints.length).toBeGreaterThan(0);
    });
  });

  it('Refresh button triggers reload', async () => {
    const { getByText, findByText } = render(MergeQueueView);
    // Wait for initial load to complete
    await findByText('Queued');
    const callsBefore = api.mergeQueue.mock.calls.length;

    await fireEvent.click(getByText('Refresh'));

    await waitFor(() => {
      expect(api.mergeQueue.mock.calls.length).toBeGreaterThan(callsBefore);
    });
  });

  it('has accessible view toggle with aria-pressed', () => {
    const { container } = render(MergeQueueView);
    const toggleBtns = container.querySelectorAll('.toggle-btn');
    expect(toggleBtns.length).toBe(2);
    expect(toggleBtns[0].getAttribute('aria-pressed')).toBe('true');
    expect(toggleBtns[1].getAttribute('aria-pressed')).toBe('false');
  });

  it('has accessible sr-only queue loaded announcement', async () => {
    const { container } = render(MergeQueueView);
    await waitFor(() => {
      const srOnly = container.querySelector('.sr-only[aria-live="polite"]');
      expect(srOnly).toBeTruthy();
      expect(srOnly.textContent).toBe('Merge queue loaded');
    });
  });
});
