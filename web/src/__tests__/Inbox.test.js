import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';
import Inbox from '../components/Inbox.svelte';

const mockMr = {
  id: 'mr-uuid-1',
  title: 'Fix something',
  repository_id: 'repo-uuid-1',
  created_at: new Date().toISOString(),
};

const mockSpec = {
  path: 'specs/system/design-principles.md',
  title: 'Design Principles',
  sha: 'abc123def456abc123def456abc123def456abc1',
  updated_at: new Date().toISOString(),
  approval_status: 'Pending',
};

const mockGate = {
  id: 'gate-result-1',
  gate_name: 'lint',
  status: 'failed',
  started_at: new Date().toISOString(),
  finished_at: new Date().toISOString(),
};

// Mock the api module
vi.mock('../lib/api.js', () => ({
  api: {
    mergeRequests: vi.fn().mockResolvedValue([]),
    getPendingSpecs: vi.fn().mockResolvedValue([]),
    mrGates: vi.fn().mockResolvedValue([]),
    approveSpec: vi.fn().mockResolvedValue({}),
    revokeSpec: vi.fn().mockResolvedValue({}),
    enqueue: vi.fn().mockResolvedValue({}),
  },
}));

import { api } from '../lib/api.js';

describe('Inbox', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
    api.mergeRequests.mockResolvedValue([]);
    api.getPendingSpecs.mockResolvedValue([]);
    api.mrGates.mockResolvedValue([]);
  });

  it('renders without throwing', () => {
    expect(() => render(Inbox)).not.toThrow();
  });

  it('mounts and produces DOM output', () => {
    const { container } = render(Inbox);
    expect(container).toBeTruthy();
    expect(container.innerHTML.length).toBeGreaterThan(0);
  });

  it('shows the inbox title', () => {
    const { getByText } = render(Inbox);
    expect(getByText('Inbox')).toBeTruthy();
  });

  it('shows loading skeleton initially', () => {
    const { container } = render(Inbox);
    expect(container.innerHTML).toBeTruthy();
  });

  it('shows refresh button', () => {
    const { getByText } = render(Inbox);
    expect(getByText('Refresh')).toBeTruthy();
  });

  it('shows empty state when no items', async () => {
    const { findByText } = render(Inbox);
    const emptyMsg = await findByText('All caught up!');
    expect(emptyMsg).toBeTruthy();
  });

  it('shows Approve and Reject buttons for pending spec items', async () => {
    api.getPendingSpecs.mockResolvedValue([mockSpec]);
    const { findByText } = render(Inbox);
    expect(await findByText('Approve')).toBeTruthy();
    expect(await findByText('Reject')).toBeTruthy();
  });

  it('does not show Approve/Reject for spec items missing sha', async () => {
    api.getPendingSpecs.mockResolvedValue([{ ...mockSpec, sha: null }]);
    const { queryByText, findByText } = render(Inbox);
    await findByText(/Approve:/);
    expect(queryByText('Approve')).toBeNull();
    expect(queryByText('Reject')).toBeNull();
  });

  it('calls approveSpec when Approve button is clicked', async () => {
    api.getPendingSpecs.mockResolvedValue([mockSpec]);
    const { findByText } = render(Inbox);
    const btn = await findByText('Approve');
    await fireEvent.click(btn);
    expect(api.approveSpec).toHaveBeenCalledWith(mockSpec.path, mockSpec.sha);
  });

  it('calls revokeSpec when Reject button is clicked', async () => {
    api.getPendingSpecs.mockResolvedValue([mockSpec]);
    const { findByText } = render(Inbox);
    const btn = await findByText('Reject');
    await fireEvent.click(btn);
    expect(api.revokeSpec).toHaveBeenCalledWith(mockSpec.path, 'Rejected from inbox');
  });

  it('shows Retry button for gate failure items', async () => {
    api.mergeRequests.mockResolvedValue([mockMr]);
    api.mrGates.mockResolvedValue([mockGate]);
    const { findByText } = render(Inbox);
    expect(await findByText('Retry')).toBeTruthy();
  });

  it('calls enqueue when Retry button is clicked', async () => {
    api.mergeRequests.mockResolvedValue([mockMr]);
    api.mrGates.mockResolvedValue([mockGate]);
    const { findByText } = render(Inbox);
    const btn = await findByText('Retry');
    await fireEvent.click(btn);
    expect(api.enqueue).toHaveBeenCalledWith(mockMr.id);
  });

  it('shows success feedback after approve', async () => {
    api.getPendingSpecs.mockResolvedValue([mockSpec]);
    const { findByText } = render(Inbox);
    const btn = await findByText('Approve');
    await fireEvent.click(btn);
    await waitFor(async () => {
      expect(await findByText('Approved')).toBeTruthy();
    });
  });

  it('shows error feedback when approve fails', async () => {
    api.getPendingSpecs.mockResolvedValue([mockSpec]);
    api.approveSpec.mockRejectedValueOnce(new Error('Not authorized'));
    const { findByText } = render(Inbox);
    const btn = await findByText('Approve');
    await fireEvent.click(btn);
    await waitFor(async () => {
      expect(await findByText('Not authorized')).toBeTruthy();
    });
  });

  it('does not show Retry for gate items without mr_id', async () => {
    // Gate fetched but MR fetch empty = no gate items
    api.mergeRequests.mockResolvedValue([]);
    const { queryByText } = render(Inbox);
    await waitFor(() => {
      expect(queryByText('Retry')).toBeNull();
    });
  });
});
