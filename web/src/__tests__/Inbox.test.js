import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';
import Inbox from '../components/Inbox.svelte';

// Notification fixtures covering all 10 types
const makeNotification = (overrides = {}) => ({
  id: 'notif-1',
  notification_type: 'agent_clarification',
  priority: 1,
  title: 'Agent needs clarification',
  body: JSON.stringify({
    message: 'Token refresh not in spec.',
    spec_path: 'specs/system/identity-security.md',
    agent_id: 'worker-8',
    persona: 'backend-dev v4',
    mr_title: 'auth-refactor',
  }),
  entity_ref: 'worker-8',
  workspace_id: 'ws-1',
  repo_id: null,
  resolved_at: null,
  dismissed_at: null,
  created_at: new Date(Date.now() - 2 * 60 * 1000).toISOString(),
  ...overrides,
});

const specApprovalNotif = makeNotification({
  id: 'notif-2',
  notification_type: 'spec_approval',
  priority: 2,
  title: 'Spec pending approval',
  body: JSON.stringify({
    spec_path: 'specs/system/api-conventions.md',
    spec_sha: 'abc123def456abc123def456abc123def456abc1',
    diff_summary: '+45 lines',
  }),
  entity_ref: 'specs/system/api-conventions.md',
});

const gateFailureNotif = makeNotification({
  id: 'notif-3',
  notification_type: 'gate_failure',
  priority: 3,
  title: 'Gate failure: lint',
  body: JSON.stringify({
    mr_id: 'mr-uuid-42',
    mr_title: 'feat: rate limiting',
    gate_name: 'lint',
    output: 'error: unused import',
  }),
  entity_ref: 'mr-uuid-42',
});

const dismissedNotif = makeNotification({
  id: 'notif-dismissed',
  notification_type: 'trust_suggestion',
  priority: 8,
  title: 'Consider increasing trust',
  body: JSON.stringify({ message: '0 failures in 30 days.' }),
  dismissed_at: new Date().toISOString(),
});

// Mock the api module
vi.mock('../lib/api.js', () => ({
  api: {
    myNotifications: vi.fn().mockResolvedValue([]),
    approveSpec: vi.fn().mockResolvedValue({}),
    revokeSpec: vi.fn().mockResolvedValue({}),
    enqueue: vi.fn().mockResolvedValue({}),
    markNotificationRead: vi.fn().mockResolvedValue({}),
  },
}));

import { api } from '../lib/api.js';

describe('Inbox', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.clearAllMocks();
    api.myNotifications.mockResolvedValue([]);
    api.approveSpec.mockResolvedValue({});
    api.revokeSpec.mockResolvedValue({});
    api.enqueue.mockResolvedValue({});
    api.markNotificationRead.mockResolvedValue({});
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

  it('shows Show Dismissed toggle', () => {
    const { getByText } = render(Inbox);
    expect(getByText(/Show Dismissed/)).toBeTruthy();
  });

  it('shows refresh button', () => {
    const { getByText } = render(Inbox);
    expect(getByText('Refresh')).toBeTruthy();
  });

  it('renders notification cards from API data', async () => {
    api.myNotifications.mockResolvedValue([makeNotification()]);
    const { findByText } = render(Inbox);
    expect(await findByText('Agent needs clarification')).toBeTruthy();
  });

  it('renders mock data when API returns empty', async () => {
    api.myNotifications.mockResolvedValue([]);
    const { findByText } = render(Inbox);
    // Mock data always has a priority-1 agent_clarification card
    expect(await findByText('Agent needs clarification')).toBeTruthy();
  });

  it('shows priority badge on each card', async () => {
    api.myNotifications.mockResolvedValue([makeNotification({ priority: 1 })]);
    const { findByText } = render(Inbox);
    expect(await findByText('!1')).toBeTruthy();
  });

  it('sorts notifications by priority ascending', async () => {
    api.myNotifications.mockResolvedValue([
      makeNotification({ id: 'b', priority: 3, title: 'Third Priority' }),
      makeNotification({ id: 'a', priority: 1, title: 'First Priority' }),
    ]);
    const { findAllByRole } = render(Inbox);
    const items = await findAllByRole('listitem');
    expect(items[0].textContent).toContain('First Priority');
    expect(items[1].textContent).toContain('Third Priority');
  });

  it('accordion: card body is hidden initially', async () => {
    api.myNotifications.mockResolvedValue([makeNotification()]);
    const { findByText } = render(Inbox);
    await findByText('Agent needs clarification');
    // Card body content should not be in DOM when collapsed
    expect(document.querySelector('.card-body')).toBeNull();
  });

  it('accordion: card expands on click revealing body', async () => {
    api.myNotifications.mockResolvedValue([makeNotification()]);
    const { findByRole } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Agent needs clarification/ });
    await fireEvent.click(header);
    await waitFor(() => {
      expect(document.querySelector('.card-body')).not.toBeNull();
    });
  });

  it('accordion: clicking same card again collapses it', async () => {
    api.myNotifications.mockResolvedValue([makeNotification()]);
    const { findByRole } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Agent needs clarification/ });
    await fireEvent.click(header);
    await waitFor(() => expect(document.querySelector('.card-body')).not.toBeNull());
    await fireEvent.click(header);
    await waitFor(() => expect(document.querySelector('.card-body')).toBeNull());
  });

  it('accordion: expand one collapses another', async () => {
    api.myNotifications.mockResolvedValue([
      makeNotification({ id: 'n1', priority: 1, title: 'Card One' }),
      makeNotification({ id: 'n2', priority: 2, title: 'Card Two' }),
    ]);
    const { findAllByRole } = render(Inbox);
    const headers = await findAllByRole('button', { name: /Expand:/ });
    await fireEvent.click(headers[0]);
    await waitFor(() => expect(document.querySelectorAll('.card-body').length).toBe(1));
    await fireEvent.click(headers[1]);
    await waitFor(() => expect(document.querySelectorAll('.card-body').length).toBe(1));
  });

  it('hides dismissed notifications by default', async () => {
    api.myNotifications.mockResolvedValue([
      makeNotification({ id: 'visible', title: 'Visible Card' }),
      dismissedNotif,
    ]);
    const { findByText, queryByText } = render(Inbox);
    await findByText('Visible Card');
    expect(queryByText('Consider increasing trust')).toBeNull();
  });

  it('shows dismissed notifications when Show Dismissed is toggled', async () => {
    api.myNotifications.mockResolvedValue([
      makeNotification({ id: 'visible', title: 'Visible Card' }),
      dismissedNotif,
    ]);
    const { findByText, findByRole } = render(Inbox);
    await findByText('Visible Card');
    const checkbox = await findByRole('checkbox');
    await fireEvent.click(checkbox);
    expect(await findByText('Consider increasing trust')).toBeTruthy();
  });

  it('agent_clarification: shows Respond to Agent, View Spec, Dismiss when expanded', async () => {
    api.myNotifications.mockResolvedValue([makeNotification()]);
    const { findByRole } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Agent needs clarification/ });
    await fireEvent.click(header);
    await waitFor(() => {
      expect(document.body.textContent).toContain('Respond to Agent');
      expect(document.body.textContent).toContain('View Spec');
      expect(document.body.textContent).toContain('Dismiss');
    });
  });

  it('spec_approval: shows Approve, Reject, Open Spec when expanded', async () => {
    api.myNotifications.mockResolvedValue([specApprovalNotif]);
    const { findByRole } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Spec pending approval/ });
    await fireEvent.click(header);
    await waitFor(() => {
      expect(document.body.textContent).toContain('Approve');
      expect(document.body.textContent).toContain('Reject');
      expect(document.body.textContent).toContain('Open Spec');
    });
  });

  it('gate_failure: shows View Diff, Retry, Override, Close MR when expanded', async () => {
    api.myNotifications.mockResolvedValue([gateFailureNotif]);
    const { findByRole } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Gate failure/ });
    await fireEvent.click(header);
    await waitFor(() => {
      expect(document.body.textContent).toContain('Retry');
      expect(document.body.textContent).toContain('View Diff');
      expect(document.body.textContent).toContain('Override');
      expect(document.body.textContent).toContain('Close MR');
    });
  });

  it('calls approveSpec when Approve is clicked', async () => {
    api.myNotifications.mockResolvedValue([specApprovalNotif]);
    const { findByRole, findByText } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Spec pending approval/ });
    await fireEvent.click(header);
    const approveBtn = await findByText('Approve');
    await fireEvent.click(approveBtn);
    expect(api.approveSpec).toHaveBeenCalledWith(
      'system/api-conventions.md',
      'abc123def456abc123def456abc123def456abc1',
    );
  });

  it('calls revokeSpec when Reject is clicked', async () => {
    api.myNotifications.mockResolvedValue([specApprovalNotif]);
    const { findByRole, findByText } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Spec pending approval/ });
    await fireEvent.click(header);
    const rejectBtn = await findByText('Reject');
    await fireEvent.click(rejectBtn);
    expect(api.revokeSpec).toHaveBeenCalledWith(
      'system/api-conventions.md',
      'Rejected from inbox',
    );
  });

  it('calls enqueue when Retry is clicked for gate_failure', async () => {
    api.myNotifications.mockResolvedValue([gateFailureNotif]);
    const { findByRole, findByText } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Gate failure/ });
    await fireEvent.click(header);
    const retryBtn = await findByText('Retry');
    await fireEvent.click(retryBtn);
    expect(api.enqueue).toHaveBeenCalledWith('mr-uuid-42');
  });

  it('shows Approved feedback after successful approve', async () => {
    api.myNotifications.mockResolvedValue([specApprovalNotif]);
    const { findByRole, findByText } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Spec pending approval/ });
    await fireEvent.click(header);
    const approveBtn = await findByText('Approve');
    await fireEvent.click(approveBtn);
    await waitFor(async () => {
      expect(await findByText('Approved')).toBeTruthy();
    });
  });

  it('shows error feedback when approve fails', async () => {
    api.myNotifications.mockResolvedValue([specApprovalNotif]);
    api.approveSpec.mockRejectedValueOnce(new Error('Not authorized'));
    const { findByRole, findByText } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Spec pending approval/ });
    await fireEvent.click(header);
    const approveBtn = await findByText('Approve');
    await fireEvent.click(approveBtn);
    await waitFor(async () => {
      expect(await findByText('Not authorized')).toBeTruthy();
    });
  });

  it('shows unresolved count badge when there are unresolved items', async () => {
    api.myNotifications.mockResolvedValue([
      makeNotification({ id: 'n1' }),
      makeNotification({ id: 'n2', priority: 2 }),
    ]);
    const { findByText } = render(Inbox);
    await waitFor(async () => {
      expect(await findByText('2')).toBeTruthy();
    });
  });

  it('renders mock data covering all 10 notification types when API is empty', async () => {
    api.myNotifications.mockResolvedValue([]);
    const { findByText } = render(Inbox);
    expect(await findByText('Agent needs clarification')).toBeTruthy();
    expect(await findByText('Spec pending approval')).toBeTruthy();
    expect(await findByText('Gate failure: lint')).toBeTruthy();
    expect(await findByText('Cross-workspace spec change')).toBeTruthy();
    expect(await findByText('Conflicting interpretations detected')).toBeTruthy();
    expect(await findByText('Meta-spec drift detected')).toBeTruthy();
    expect(await findByText('Budget warning: 85% used')).toBeTruthy();
    expect(await findByText('Consider increasing trust level')).toBeTruthy();
    expect(await findByText('Spec assertion failure')).toBeTruthy();
    expect(await findByText('Suggested spec link')).toBeTruthy();
  });

  it('falls back to mock data on API error', async () => {
    api.myNotifications.mockRejectedValue(new Error('Network error'));
    const { findByText } = render(Inbox);
    expect(await findByText('Agent needs clarification')).toBeTruthy();
  });
});
