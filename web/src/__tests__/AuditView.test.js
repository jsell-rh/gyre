import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    auditEvents: vi.fn(),
    auditStats: vi.fn(),
  },
}));

import { api } from '../lib/api.js';
import AuditView from '../components/AuditView.svelte';

// Suppress fetch errors from SSE connection attempts
global.fetch = vi.fn().mockRejectedValue(new Error('not available'));

describe('AuditView', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.auditEvents.mockResolvedValue({ events: [] });
    api.auditStats.mockResolvedValue({ total: 0, denied: 0 });
    global.fetch = vi.fn().mockRejectedValue(new Error('not available'));
  });

  it('renders without throwing', () => {
    expect(() => render(AuditView)).not.toThrow();
  });

  it('shows Audit Events heading', async () => {
    const { getByText } = render(AuditView);
    expect(getByText('Audit Events')).toBeTruthy();
  });

  it('shows Live Stream and History tabs', async () => {
    const { getByText } = render(AuditView);
    expect(getByText('Live Stream')).toBeTruthy();
    expect(getByText('History')).toBeTruthy();
  });

  it('shows SSE status indicator on live tab', async () => {
    const { findByText } = render(AuditView);
    // SSE will fail in test env, so it should show Disconnected
    expect(await findByText('Disconnected')).toBeTruthy();
  });

  it('shows clear button on live tab', () => {
    const { getByText } = render(AuditView);
    expect(getByText('Clear')).toBeTruthy();
  });

  it('switches to history tab when clicked', async () => {
    const { getByText, findByText } = render(AuditView);
    const historyTab = getByText('History');
    await fireEvent.click(historyTab);

    // History tab shows filter bar with Search button
    expect(await findByText('Search')).toBeTruthy();
  });

  it('shows event type filter on history tab', async () => {
    const { getByText, getByLabelText } = render(AuditView);
    await fireEvent.click(getByText('History'));

    const select = getByLabelText('Filter by event type');
    expect(select).toBeTruthy();
  });

  it('shows agent ID filter on history tab', async () => {
    const { getByText, getByLabelText } = render(AuditView);
    await fireEvent.click(getByText('History'));

    const input = getByLabelText('Filter by agent ID');
    expect(input).toBeTruthy();
  });

  it('shows empty state when no events in history', async () => {
    api.auditEvents.mockResolvedValue({ events: [] });
    const { getByText, findByText } = render(AuditView);
    await fireEvent.click(getByText('History'));

    expect(await findByText('No audit events')).toBeTruthy();
  });

  it('renders audit events when API returns data', async () => {
    api.auditEvents.mockResolvedValue({
      events: [
        { id: 'ev-1', event_type: 'FileAccess', agent_id: 'agent-01', details: 'Read /etc/passwd', timestamp: 1700000000 },
        { id: 'ev-2', event_type: 'SyscallDenied', agent_id: 'agent-02', details: 'Denied ptrace', timestamp: 1700000100 },
      ],
    });

    const { getByText, findByText } = render(AuditView);
    await fireEvent.click(getByText('History'));

    expect(await findByText('Read /etc/passwd')).toBeTruthy();
    expect(await findByText('Denied ptrace')).toBeTruthy();
    expect(await findByText('FileAccess')).toBeTruthy();
    expect(await findByText('SyscallDenied')).toBeTruthy();
  });

  it('shows audit stats when available', async () => {
    api.auditStats.mockResolvedValue({ total: 42, denied: 5 });
    const { findByText } = render(AuditView);

    expect(await findByText('42')).toBeTruthy();
    expect(await findByText('5')).toBeTruthy();
  });

  it('shows error state when API fails', async () => {
    api.auditEvents.mockRejectedValue(new Error('Database error'));
    const { getByText, findByText } = render(AuditView);
    await fireEvent.click(getByText('History'));

    expect(await findByText('Database error')).toBeTruthy();
  });

  it('shows waiting for events on live tab with no events', async () => {
    const { findByText } = render(AuditView);
    expect(await findByText('Waiting for events')).toBeTruthy();
  });

  it('live tab active by default', () => {
    const { container } = render(AuditView);
    const liveTab = container.querySelector('#tab-live');
    expect(liveTab.getAttribute('aria-selected')).toBe('true');
  });

  it('history tab not selected by default', () => {
    const { container } = render(AuditView);
    const histTab = container.querySelector('#tab-history');
    expect(histTab.getAttribute('aria-selected')).toBe('false');
  });
});
