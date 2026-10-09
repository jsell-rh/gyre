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
    mr_id: 'mr-uuid-42',
    mr_title: 'Spec edit: specs/system/api-conventions.md',
    repo_id: 'repo-1',
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
    getSpec: vi.fn().mockResolvedValue({ path: 'system/api-conventions.md', current_sha: 'fetchedsha000000000000000000000000000000' }),
    approveSpec: vi.fn().mockResolvedValue({}),
    revokeSpec: vi.fn().mockResolvedValue({}),
    enqueue: vi.fn().mockResolvedValue({}),
    markNotificationRead: vi.fn().mockResolvedValue({}),
    resolveNotification: vi.fn().mockResolvedValue({}),
    agent: vi.fn().mockResolvedValue({ name: 'test-agent' }),
    task: vi.fn().mockResolvedValue({ title: 'test-task' }),
    mrDiff: vi.fn().mockResolvedValue({
      files: [
        {
          path: 'specs/system/api-conventions.md',
          status: 'modified',
          hunks: [
            {
              header: '@@ -1,3 +1,4 @@',
              lines: [
                { type: 'context', content: '# API Conventions' },
                { type: 'delete', content: 'Old rule text' },
                { type: 'add', content: 'New rule text' },
              ],
            },
          ],
        },
      ],
    }),
    workspaces: vi.fn().mockResolvedValue([]),
    mrStatus: vi.fn().mockResolvedValue({ status: 'closed' }),
    submitReview: vi.fn().mockResolvedValue({}),
    pauseMergeQueue: vi.fn().mockResolvedValue({ queue_paused: true }),
    agents: vi.fn().mockResolvedValue([]),
    sendAgentMessage: vi.fn().mockResolvedValue({}),
    mergeRequests: vi.fn().mockResolvedValue([]),
    revertMr: vi.fn().mockResolvedValue({ repo_id: 'repo-1', mr_id: 'mr-lose', revert_commit_sha: 'rev123' }),
    createTask: vi.fn().mockResolvedValue({ id: 'task-42', title: 'Reconcile' }),
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
    api.getSpec.mockResolvedValue({ path: 'system/api-conventions.md', current_sha: 'fetchedsha000000000000000000000000000000' });
  });

  it('renders without throwing', () => {
    expect(() => render(Inbox)).not.toThrow();
  });

  it('mounts and produces DOM output', () => {
    const { container } = render(Inbox);
    expect(container).toBeTruthy();
    expect(container.innerHTML.length).toBeGreaterThan(0);
  });

  it('shows the decisions title', () => {
    const { getByText } = render(Inbox);
    expect(getByText('Decisions')).toBeTruthy();
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

  it('renders empty state when API returns empty', async () => {
    api.myNotifications.mockResolvedValue([]);
    const { findByText } = render(Inbox);
    expect(await findByText('All caught up!')).toBeTruthy();
  });

  it('shows priority badge on each card', async () => {
    api.myNotifications.mockResolvedValue([makeNotification({ priority: 1 })]);
    const { findByText } = render(Inbox);
    expect(await findByText('P1')).toBeTruthy();
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

  it('spec_approval: shows Approve, Reject, View Full Spec when expanded', async () => {
    api.myNotifications.mockResolvedValue([specApprovalNotif]);
    const { findByRole } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Spec pending approval/ });
    await fireEvent.click(header);
    await waitFor(() => {
      expect(document.body.textContent).toContain('Approve');
      expect(document.body.textContent).toContain('Reject');
      expect(document.body.textContent).toContain('View Full Spec');
    });
  });

  // ui-layout.md §7 Item Structure: at tenant scope each card shows the
  // workspace name so the human knows where the decision belongs.
  it('shows workspace name badge on cards at tenant scope (ui-layout §7)', async () => {
    api.workspaces = vi.fn().mockResolvedValue([{ id: 'ws-1', name: 'Payments' }]);
    api.myNotifications.mockResolvedValue([specApprovalNotif]);
    const { findByText } = render(Inbox, { props: { scope: 'tenant' } });
    await waitFor(() => {
      expect(api.workspaces).toHaveBeenCalled();
    });
    expect(await findByText('Payments')).toBeTruthy();
  });

  it('gate_failure: shows View Diff, View Output, Retry Gate, Override, Close MR when expanded (HSI §8 P3)', async () => {
    api.myNotifications.mockResolvedValue([gateFailureNotif]);
    const { findByRole } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Gate failure/ });
    await fireEvent.click(header);
    await waitFor(() => {
      expect(document.body.textContent).toContain('View Diff');
      expect(document.body.textContent).toContain('View Output');
      expect(document.body.textContent).toContain('Retry Gate');
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

  // Legacy shape: notifications created before the server populated the body
  // (body = null, spec path only in the title). Approve must still work by
  // parsing the title and fetching the SHA from the spec ledger.
  const legacySpecApprovalNotif = makeNotification({
    id: 'notif-legacy-approval',
    notification_type: 'spec_approval',
    priority: 2,
    title: 'Spec pending approval: system/api-conventions.md',
    body: null,
    entity_ref: 'mr-uuid-legacy',
  });

  it('approves a legacy body-less spec_approval notification by fetching the SHA', async () => {
    api.myNotifications.mockResolvedValue([legacySpecApprovalNotif]);
    const { findByRole, findByText } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Spec pending approval/ });
    await fireEvent.click(header);
    const approveBtn = await findByText('Approve');
    await fireEvent.click(approveBtn);
    expect(api.getSpec).toHaveBeenCalledWith('system/api-conventions.md');
    expect(api.approveSpec).toHaveBeenCalledWith(
      'system/api-conventions.md',
      'fetchedsha000000000000000000000000000000',
    );
    await waitFor(() => {
      expect(document.body.textContent).toContain('Approved');
    });
  });

  it('rejects a legacy body-less spec_approval notification by parsing the title', async () => {
    api.myNotifications.mockResolvedValue([legacySpecApprovalNotif]);
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

  it('calls enqueue when Retry Gate is clicked for gate_failure', async () => {
    api.myNotifications.mockResolvedValue([gateFailureNotif]);
    const { findByRole, findByText } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Gate failure/ });
    await fireEvent.click(header);
    const retryBtn = await findByText('Retry Gate');
    await fireEvent.click(retryBtn);
    expect(api.enqueue).toHaveBeenCalledWith('mr-uuid-42');
  });

  it('calls submitReview with approved decision when Override is clicked for gate_failure', async () => {
    api.myNotifications.mockResolvedValue([gateFailureNotif]);
    const { findByRole, findByText } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Gate failure/ });
    await fireEvent.click(header);
    const overrideBtn = await findByText('Override');
    await fireEvent.click(overrideBtn);
    expect(api.submitReview).toHaveBeenCalledWith(
      'mr-uuid-42',
      expect.objectContaining({ decision: 'approved' }),
    );
    await waitFor(() => {
      expect(document.body.textContent).toContain('Override submitted');
    });
  });

  it('calls mrStatus with closed when Close MR is clicked for gate_failure', async () => {
    api.myNotifications.mockResolvedValue([gateFailureNotif]);
    const { findByRole, findByText } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Gate failure/ });
    await fireEvent.click(header);
    const closeBtn = await findByText('Close MR');
    await fireEvent.click(closeBtn);
    expect(api.mrStatus).toHaveBeenCalledWith('mr-uuid-42', 'closed');
    await waitFor(() => {
      expect(document.body.textContent).toContain('MR closed');
    });
  });

  it('View Diff and View Output open the MR detail panel with the right tab', async () => {
    const openDetailPanel = vi.fn();
    api.myNotifications.mockResolvedValue([gateFailureNotif]);
    const { findByRole, findByText } = render(Inbox, {
      context: new Map([['openDetailPanel', openDetailPanel]]),
    });
    const header = await findByRole('button', { name: /Expand: Gate failure/ });
    await fireEvent.click(header);
    await fireEvent.click(await findByText('View Diff'));
    expect(openDetailPanel).toHaveBeenCalledWith(
      expect.objectContaining({ type: 'mr', id: 'mr-uuid-42' }),
    );
    expect(openDetailPanel.mock.calls[0][0].data._openTab).toBe('diff');
    await fireEvent.click(await findByText('View Output'));
    const lastCall = openDetailPanel.mock.calls[openDetailPanel.mock.calls.length - 1];
    expect(lastCall[0].data._openTab).toBe('gates');
  });

  it('budget_warning: shows Increase Limit and Pause Work when expanded (HSI §8 P7)', async () => {
    api.myNotifications.mockResolvedValue([
      makeNotification({
        id: 'notif-budget',
        notification_type: 'budget_warning',
        priority: 7,
        title: 'Budget warning',
        body: JSON.stringify({ message: 'Workspace at 90% of budget.' }),
        repo_id: 'repo-1',
      }),
    ]);
    const { findByRole } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Budget warning/ });
    await fireEvent.click(header);
    await waitFor(() => {
      expect(document.body.textContent).toContain('Increase Limit');
      expect(document.body.textContent).toContain('Pause Work');
    });
  });

  it('calls pauseMergeQueue when Pause Work is clicked for repo-scoped budget_warning', async () => {
    api.myNotifications.mockResolvedValue([
      makeNotification({
        id: 'notif-budget',
        notification_type: 'budget_warning',
        priority: 7,
        title: 'Budget warning',
        body: JSON.stringify({ message: 'Workspace at 90% of budget.' }),
        repo_id: 'repo-1',
      }),
    ]);
    const { findByRole, findByText } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Budget warning/ });
    await fireEvent.click(header);
    const pauseBtn = await findByText('Pause Work');
    await fireEvent.click(pauseBtn);
    expect(api.pauseMergeQueue).toHaveBeenCalledWith('repo-1', expect.any(String));
  });

  it('sends pause_requested to active agents when Pause Work is clicked for workspace-scoped budget_warning', async () => {
    api.agents.mockResolvedValue([
      { id: 'agent-a', status: 'active' },
      { id: 'agent-b', status: 'stopped' },
    ]);
    api.myNotifications.mockResolvedValue([
      makeNotification({
        id: 'notif-budget-ws',
        notification_type: 'budget_warning',
        priority: 7,
        title: 'Budget warning',
        body: JSON.stringify({ message: 'Workspace at 90% of budget.' }),
        repo_id: null,
      }),
    ]);
    const { findByRole, findByText } = render(Inbox, {
      props: { workspaceId: 'ws-1', scope: 'workspace' },
    });
    const header = await findByRole('button', { name: /Expand: Budget warning/ });
    await fireEvent.click(header);
    const pauseBtn = await findByText('Pause Work');
    await fireEvent.click(pauseBtn);
    await waitFor(() => {
      expect(api.sendAgentMessage).toHaveBeenCalledTimes(1);
    });
    expect(api.sendAgentMessage).toHaveBeenCalledWith(
      'ws-1',
      'agent-a',
      expect.objectContaining({
        kind: 'status_update',
        tier: 'directed',
        payload: expect.objectContaining({ status: 'pause_requested' }),
      }),
    );
  });

  // ── HSI §8 remaining priority types ──────────────────────────────────────

  it('cross_workspace_change: shows Review Changes and Dismiss when expanded (HSI §8 P4)', async () => {
    api.myNotifications.mockResolvedValue([
      makeNotification({
        id: 'notif-xw',
        notification_type: 'cross_workspace_change',
        priority: 4,
        title: 'Cross-workspace change',
        body: JSON.stringify({
          message: 'platform-core updated idempotent-api.md',
          spec_path: 'specs/system/idempotent-api.md',
          change_summary: 'Your payment-retry.md depends on it.',
        }),
        entity_ref: 'specs/system/idempotent-api.md',
      }),
    ]);
    const { findByRole } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Cross-workspace change/ });
    await fireEvent.click(header);
    await waitFor(() => {
      expect(document.body.textContent).toContain('Review Changes');
      expect(document.body.textContent).toContain('Dismiss');
    });
    expect(document.body.textContent).not.toContain('coming soon');
  });

  it('meta_spec_drift: shows View Results and Adjust Meta-spec when expanded (HSI §8 P6)', async () => {
    api.myNotifications.mockResolvedValue([
      makeNotification({
        id: 'notif-drift',
        notification_type: 'meta_spec_drift',
        priority: 6,
        title: 'Meta-spec drift detected',
        body: JSON.stringify({
          message: 'Implementation drift on meta-spec.',
          meta_spec_path: 'meta/testing-standards.md',
        }),
        entity_ref: 'meta/testing-standards.md',
      }),
    ]);
    const { findByRole } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Meta-spec drift detected/ });
    await fireEvent.click(header);
    await waitFor(() => {
      expect(document.body.textContent).toContain('View Results');
      expect(document.body.textContent).toContain('Adjust Meta-spec');
    });
  });

  it('meta_spec_drift: Adjust Meta-spec navigates to agent rules', async () => {
    api.myNotifications.mockResolvedValue([
      makeNotification({
        id: 'notif-drift-nav',
        notification_type: 'meta_spec_drift',
        priority: 6,
        title: 'Meta-spec drift detected',
        body: JSON.stringify({
          message: 'Implementation drift on meta-spec.',
          meta_spec_path: 'meta/testing-standards.md',
        }),
        entity_ref: 'meta/testing-standards.md',
      }),
    ]);
    const goToAgentRules = vi.fn();
    const { findByRole, findByText } = render(Inbox, {
      context: new Map([['goToAgentRules', goToAgentRules]]),
    });
    const header = await findByRole('button', { name: /Expand: Meta-spec drift detected/ });
    await fireEvent.click(header);
    await fireEvent.click(await findByText('Adjust Meta-spec'));
    expect(goToAgentRules).toHaveBeenCalledTimes(1);
  });

  it('spec_assertion_failure: shows View Code and Update Spec when expanded (HSI §8 P9)', async () => {
    api.myNotifications.mockResolvedValue([
      makeNotification({
        id: 'notif-assert',
        notification_type: 'spec_assertion_failure',
        priority: 9,
        title: 'Spec assertion failure',
        body: JSON.stringify({
          message: 'Code no longer matches spec assertion.',
          repo_id: 'repo-1',
          spec_path: 'specs/system/payments.md',
        }),
        entity_ref: 'repo-1',
      }),
    ]);
    const { findByRole } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Spec assertion failure/ });
    await fireEvent.click(header);
    await waitFor(() => {
      expect(document.body.textContent).toContain('View Code');
      expect(document.body.textContent).toContain('Update Spec');
    });
  });

  it('spec_assertion_failure: View Code opens the repo detail panel', async () => {
    const openDetailPanel = vi.fn();
    api.myNotifications.mockResolvedValue([
      makeNotification({
        id: 'notif-assert-nav',
        notification_type: 'spec_assertion_failure',
        priority: 9,
        title: 'Spec assertion failure',
        body: JSON.stringify({
          message: 'Code no longer matches spec assertion.',
          repo_id: 'repo-1',
          spec_path: 'specs/system/payments.md',
        }),
        entity_ref: 'repo-1',
      }),
    ]);
    const { findByRole, findByText } = render(Inbox, {
      context: new Map([['openDetailPanel', openDetailPanel]]),
    });
    const header = await findByRole('button', { name: /Expand: Spec assertion failure/ });
    await fireEvent.click(header);
    await fireEvent.click(await findByText('View Code'));
    expect(openDetailPanel).toHaveBeenCalledWith(
      expect.objectContaining({ type: 'repo', id: 'repo-1' })
    );
  });

  it('suggested_link: shows Confirm and Dismiss when expanded (HSI §8 P10)', async () => {
    api.myNotifications.mockResolvedValue([
      makeNotification({
        id: 'notif-link',
        notification_type: 'suggested_link',
        priority: 10,
        title: 'Suggested spec link',
        body: JSON.stringify({ message: 'payment-retry may depend on idempotent-api.' }),
        entity_ref: 'specs/system/payment-retry.md',
      }),
    ]);
    const { findByRole } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Suggested spec link/ });
    await fireEvent.click(header);
    await waitFor(() => {
      expect(document.body.textContent).toContain('Confirm');
      expect(document.body.textContent).toContain('Dismiss');
    });
    expect(document.body.textContent).not.toContain('coming soon');
  });

  it('suggested_link: Confirm resolves the notification', async () => {
    api.myNotifications.mockResolvedValue([
      makeNotification({
        id: 'notif-link-confirm',
        notification_type: 'suggested_link',
        priority: 10,
        title: 'Suggested spec link',
        body: JSON.stringify({ message: 'payment-retry may depend on idempotent-api.' }),
        entity_ref: 'specs/system/payment-retry.md',
      }),
    ]);
    const { findByRole, findByText } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Suggested spec link/ });
    await fireEvent.click(header);
    await fireEvent.click(await findByText('Confirm'));
    await waitFor(() => {
      expect(api.resolveNotification).toHaveBeenCalledWith('notif-link-confirm');
    });
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

  it('shows empty state when API returns empty array', async () => {
    api.myNotifications.mockResolvedValue([]);
    const { findByText } = render(Inbox);
    expect(await findByText('All caught up!')).toBeTruthy();
  });

  it('shows error banner on API error', async () => {
    api.myNotifications.mockRejectedValue(new Error('Network error'));
    const { findByText } = render(Inbox);
    expect(await findByText('Network error')).toBeTruthy();
  });

  it('renders the conflict diff view when a SpecConflict card is expanded (HSI §7 item 3)', async () => {
    const specConflictNotif = makeNotification({
      id: 'notif-conflict',
      notification_type: 'SpecConflict',
      priority: 2,
      title: 'Spec edit conflict: specs/system/payments.md',
      body: JSON.stringify({
        spec_path: 'specs/system/payments.md',
        base_sha: 'client-sha-1',
        current_sha: 'server-sha-2',
        diff_summary: '+1 / -1 lines',
        diff: [
          { op: 'context', text: '# Payments' },
          { op: 'remove', text: 'old server line' },
          { op: 'add', text: 'my local line' },
        ],
      }),
      entity_ref: 'specs/system/payments.md',
    });
    api.myNotifications.mockResolvedValue([specConflictNotif]);
    const { findByRole, container } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Spec edit conflict/ });
    await fireEvent.click(header);
    await waitFor(() => {
      expect(container.querySelector('[data-testid="spec-diff-view"]')).not.toBeNull();
    });
    // The diff view must show both the removed (server) and added (mine) lines.
    const rows = container.querySelectorAll('.diff-row');
    expect(rows.length).toBe(3);
    const removeCell = container.querySelector('.diff-cell.diff-remove.diff-left');
    expect(removeCell.textContent).toContain('old server line');
    const addCell = container.querySelector('.diff-cell.diff-add.diff-right');
    expect(addCell.textContent).toContain('my local line');
  });

  // HSI §8 P5: conflicting interpretations — View Both, Pick A / Pick B, Reconcile
  const conflictNotif = makeNotification({
    id: 'notif-conflict-p5',
    notification_type: 'conflicting_interpretations',
    priority: 5,
    title: 'Conflicting spec interpretations: system/payments.md',
    repo_id: 'repo-1',
    entity_ref: 'system/payments.md',
    body: JSON.stringify({
      spec_ref: 'system/payments.md',
      agent_a: 'agent-1',
      commit_sha_a: 'aaaa000000000000000000000000000000000000',
      agent_b: 'agent-2',
      commit_sha_b: 'bbbb000000000000000000000000000000000000',
      conflicting_nodes: ['PaymentService', 'RetryPolicy'],
      conflict_count: 2,
      resolution_options: [
        { label: 'Pick A', action: 'revert_b' },
        { label: 'Pick B', action: 'revert_a' },
        { label: 'Reconcile', action: 'create_reconciliation_task' },
      ],
    }),
  });

  it('conflicting_interpretations: shows View Both Specs, Pick A, Pick B, Reconcile when expanded (HSI §8 P5)', async () => {
    api.myNotifications.mockResolvedValue([conflictNotif]);
    const { findByRole } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Conflicting spec interpretations/ });
    await fireEvent.click(header);
    await waitFor(() => {
      expect(document.body.textContent).toContain('View Both Specs');
      expect(document.body.textContent).toContain('Pick A');
      expect(document.body.textContent).toContain('Pick B');
      expect(document.body.textContent).toContain('Reconcile');
    });
    // No more "coming soon" placeholder — the actions must be real.
    expect(document.body.textContent).not.toContain('coming soon');
  });

  it('conflicting_interpretations: expanded card lists the conflicting nodes and both agents', async () => {
    api.myNotifications.mockResolvedValue([conflictNotif]);
    const { findByRole, container } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Conflicting spec interpretations/ });
    await fireEvent.click(header);
    await waitFor(() => {
      expect(document.body.textContent).toContain('PaymentService');
      expect(document.body.textContent).toContain('RetryPolicy');
    });
    const nodes = container.querySelectorAll('.conflict-node');
    expect(nodes.length).toBe(2);
  });

  it('Pick B reverts agent A\'s merged MR via the recovery endpoint', async () => {
    api.mergeRequests.mockResolvedValue([
      { id: 'mr-a', repository_id: 'repo-1', status: 'merged', merge_commit_sha: 'aaaa000000000000000000000000000000000000' },
      { id: 'mr-b', repository_id: 'repo-1', status: 'merged', merge_commit_sha: 'bbbb000000000000000000000000000000000000' },
    ]);
    api.myNotifications.mockResolvedValue([conflictNotif]);
    const { findByRole, findByText } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Conflicting spec interpretations/ });
    await fireEvent.click(header);
    await fireEvent.click(await findByText('Pick B'));
    await waitFor(() => {
      expect(api.revertMr).toHaveBeenCalledWith('repo-1', 'mr-a');
    });
    await waitFor(() => {
      expect(document.body.textContent).toContain("Picked — reverted agent-1's conflicting MR");
    });
  });

  it('Pick A reverts agent B\'s merged MR via the recovery endpoint', async () => {
    api.mergeRequests.mockResolvedValue([
      { id: 'mr-a', repository_id: 'repo-1', status: 'merged', merge_commit_sha: 'aaaa000000000000000000000000000000000000' },
      { id: 'mr-b', repository_id: 'repo-1', status: 'merged', merge_commit_sha: 'bbbb000000000000000000000000000000000000' },
    ]);
    api.myNotifications.mockResolvedValue([conflictNotif]);
    const { findByRole, findByText } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Conflicting spec interpretations/ });
    await fireEvent.click(header);
    await fireEvent.click(await findByText('Pick A'));
    await waitFor(() => {
      expect(api.revertMr).toHaveBeenCalledWith('repo-1', 'mr-b');
    });
  });

  // HSI §8 P2: the expanded spec_approval accordion shows the spec-edit/*
  // branch diff inline so the human can review before approving.
  it('spec_approval: expanding fetches the MR diff and renders it inline (HSI §8 P2)', async () => {
    api.myNotifications.mockResolvedValue([specApprovalNotif]);
    const { findByRole, container } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Spec pending approval/ });
    await fireEvent.click(header);
    await waitFor(() => {
      expect(api.mrDiff).toHaveBeenCalledWith('mr-uuid-42');
    });
    await waitFor(() => {
      const view = container.querySelector('[data-testid="spec-diff-view"]');
      expect(view).not.toBeNull();
      // SpecDiffView renders side-by-side rows — the changed lines must appear
      expect(view.textContent).toContain('Old rule text');
      expect(view.textContent).toContain('New rule text');
    });
  });

  it('spec_approval: View Full Spec opens the detail panel Content tab', async () => {
    const openDetailPanel = vi.fn();
    api.myNotifications.mockResolvedValue([specApprovalNotif]);
    const { findByRole, findByText } = render(Inbox, {
      context: new Map([['openDetailPanel', openDetailPanel]]),
    });
    const header = await findByRole('button', { name: /Expand: Spec pending approval/ });
    await fireEvent.click(header);
    await fireEvent.click(await findByText('View Full Spec'));
    expect(openDetailPanel).toHaveBeenCalledWith(
      expect.objectContaining({ type: 'spec', id: 'system/api-conventions.md' })
    );
  });

  it('Pick shows a targeted error when the losing commit has no merged MR', async () => {
    api.mergeRequests.mockResolvedValue([]);
    api.myNotifications.mockResolvedValue([conflictNotif]);
    const { findByRole, findByText } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Conflicting spec interpretations/ });
    await fireEvent.click(header);
    await fireEvent.click(await findByText('Pick A'));
    await waitFor(() => {
      expect(document.body.textContent).toContain('Could not find the merged MR for commit');
    });
    expect(api.revertMr).not.toHaveBeenCalled();
  });

  it('Reconcile creates a reconciliation task scoped to the contested spec', async () => {
    api.myNotifications.mockResolvedValue([conflictNotif]);
    const { findByRole, findByText } = render(Inbox);
    const header = await findByRole('button', { name: /Expand: Conflicting spec interpretations/ });
    await fireEvent.click(header);
    await fireEvent.click(await findByText('Reconcile'));
    await waitFor(() => {
      expect(api.createTask).toHaveBeenCalledWith(
        expect.objectContaining({
          task_type: 'implementation',
          spec_path: 'system/payments.md',
          workspace_id: 'ws-1',
          labels: ['reconciliation'],
        }),
      );
    });
    await waitFor(() => {
      expect(document.body.textContent).toContain('Reconciliation task created (task-42)');
    });
  });
});
