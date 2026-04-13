import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

// Mock api before importing component
vi.mock('../lib/api.js', () => ({
  api: {
    me: vi.fn(),
    myNotifications: vi.fn(),
    myJudgments: vi.fn(),
    myAgents: vi.fn(),
    myTasks: vi.fn(),
    myMrs: vi.fn(),
    workspaces: vi.fn(),
    updateMe: vi.fn(),
    markNotificationRead: vi.fn(),
    getNotificationPreferences: vi.fn(),
    updateNotificationPreferences: vi.fn(),
  },
}));

// Mock toast
vi.mock('../lib/toast.svelte.js', () => ({
  toast: vi.fn(),
}));

import { api } from '../lib/api.js';
import UserProfile from '../components/UserProfile.svelte';

const ME = {
  username: 'jsell',
  display_name: 'James Sell',
  email: 'jsell@example.com',
  global_role: 'admin',
  timezone: 'America/New_York',
  locale: 'en-US',
  oidc_issuer: null,
};

const WORKSPACES = [
  { id: 'ws-1', name: 'Payments', slug: 'payments', trust_level: 'autonomous', role: 'admin' },
  { id: 'ws-2', name: 'Platform', slug: 'platform', trust_level: null, role: 'member' },
];

const NOTIFICATIONS = [
  { id: 'n-1', notification_type: 'SpecApproval', title: 'Spec approved', read: false, created_at: new Date(Date.now() - 300000).toISOString() },
  { id: 'n-2', notification_type: 'AgentFailure', title: 'Agent crashed', read: true, created_at: new Date(Date.now() - 7200000).toISOString() },
];

const JUDGMENTS = [
  { event_type: 'spec_approved', spec_path: 'specs/auth.md', timestamp: new Date(Date.now() - 60000).toISOString(), workspace_name: 'Payments', sha: 'abc1234567890' },
  { event_type: 'trust_override', spec_path: null, resource_id: 'ws-1', timestamp: new Date(Date.now() - 86400000).toISOString(), workspace_name: 'Platform' },
];

const CTX = new Map([['navigate', vi.fn()], ['goToWorkspaceHome', vi.fn()], ['openDetailPanel', vi.fn()]]);
const r = (props = {}) => render(UserProfile, { props, context: CTX });

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  api.me.mockResolvedValue({ ...ME });
  api.myNotifications.mockResolvedValue([...NOTIFICATIONS]);
  api.myJudgments.mockResolvedValue([...JUDGMENTS]);
  api.myAgents.mockResolvedValue([]);
  api.myTasks.mockResolvedValue([]);
  api.myMrs.mockResolvedValue([]);
  api.workspaces.mockResolvedValue([...WORKSPACES]);
  api.updateMe.mockResolvedValue({ ...ME, display_name: 'Updated Name' });
  api.markNotificationRead.mockResolvedValue({});
  api.getNotificationPreferences.mockResolvedValue({});
  api.updateNotificationPreferences.mockResolvedValue({});
});

describe('UserProfile', () => {
  it('renders without throwing', () => {
    expect(() => r()).not.toThrow();
  });

  it('shows display name and username after loading', async () => {
    const { findAllByText, findByText } = r();
    const nameEls = await findAllByText('James Sell');
    expect(nameEls.length).toBeGreaterThan(0);
    expect(await findByText('@jsell')).toBeTruthy();
  });

  it('shows avatar with first letter of display name', async () => {
    const { container, findAllByText } = r();
    await findAllByText('James Sell');
    const avatar = container.querySelector('.avatar');
    expect(avatar.textContent.trim()).toBe('J');
  });

  it('shows global role badge', async () => {
    const { findAllByText } = r();
    await findAllByText('James Sell');
    const adminEls = await findAllByText('admin');
    expect(adminEls.length).toBeGreaterThan(0);
  });

  it('shows all eight tabs', async () => {
    const { findByText, findAllByText } = r();
    await findAllByText('James Sell');
    expect(await findByText('Profile')).toBeTruthy();
    expect(await findByText('Agents')).toBeTruthy();
    expect(await findByText('Tasks')).toBeTruthy();
    expect(await findByText('MRs')).toBeTruthy();
    expect(await findByText('Workspaces')).toBeTruthy();
    expect(await findByText('Judgment Ledger')).toBeTruthy();
    expect(await findByText('Notification Preferences')).toBeTruthy();
    expect(await findByText('Notifications')).toBeTruthy();
  });

  it('shows profile info fields in Profile tab', async () => {
    const { findByText } = r();
    expect(await findByText('Username')).toBeTruthy();
    expect(await findByText('jsell')).toBeTruthy();
    expect(await findByText('jsell@example.com')).toBeTruthy();
    expect(await findByText('America/New_York')).toBeTruthy();
    expect(await findByText('en-US')).toBeTruthy();
  });

  it('shows Edit button that opens edit form', async () => {
    const { findByText, findByDisplayValue } = r();
    const editBtn = await findByText('Edit');
    await fireEvent.click(editBtn);
    expect(await findByDisplayValue('James Sell')).toBeTruthy();
    expect(await findByText('Cancel')).toBeTruthy();
    expect(await findByText('Save')).toBeTruthy();
  });

  it('Cancel closes the edit form', async () => {
    const { findByText, queryByText } = r();
    await fireEvent.click(await findByText('Edit'));
    await fireEvent.click(await findByText('Cancel'));
    await waitFor(() => {
      expect(queryByText('Save')).toBeNull();
    });
  });

  it('shows unread notification count badge', async () => {
    const { findByText } = r();
    // 1 unread notification
    expect(await findByText('1')).toBeTruthy();
  });

  it('shows workspace memberships in Workspaces tab', async () => {
    const { findByText } = r();
    const wsTab = await findByText('Workspaces');
    await fireEvent.click(wsTab);
    expect(await findByText('Payments')).toBeTruthy();
    expect(await findByText('Platform')).toBeTruthy();
  });

  it('shows Switch button for each workspace', async () => {
    const { findByText, getAllByText } = r();
    const wsTab = await findByText('Workspaces');
    await fireEvent.click(wsTab);
    await waitFor(() => {
      const switches = getAllByText('Switch');
      expect(switches.length).toBe(2);
    });
  });

  it('shows trust level when present on workspace', async () => {
    const { findByText } = r();
    const wsTab = await findByText('Workspaces');
    await fireEvent.click(wsTab);
    expect(await findByText('Trust: autonomous')).toBeTruthy();
  });

  it('shows judgment events in Judgment Ledger tab', async () => {
    const { findByText } = r();
    const ledgerTab = await findByText('Judgment Ledger');
    await fireEvent.click(ledgerTab);
    expect(await findByText('spec_approved')).toBeTruthy();
    expect(await findByText('trust_override')).toBeTruthy();
    expect(await findByText('specs/auth.md')).toBeTruthy();
  });

  it('shows SHA snippets in judgment ledger', async () => {
    const { findByText } = r();
    const ledgerTab = await findByText('Judgment Ledger');
    await fireEvent.click(ledgerTab);
    expect(await findByText('abc1234')).toBeTruthy();
  });

  it('shows empty state when no judgments', async () => {
    api.myJudgments.mockResolvedValue([]);
    const { findByText } = r();
    const ledgerTab = await findByText('Judgment Ledger');
    await fireEvent.click(ledgerTab);
    expect(await findByText(/No activity recorded/)).toBeTruthy();
  });

  it('shows notification preference toggles', async () => {
    const { findByText, container } = r();
    const prefsTab = await findByText('Notification Preferences');
    await fireEvent.click(prefsTab);
    expect(await findByText('Spec Approvals')).toBeTruthy();
    expect(await findByText('Agent Failures')).toBeTruthy();
    expect(await findByText('Trust Suggestions')).toBeTruthy();
    const checkboxes = container.querySelectorAll('.pref-checkbox');
    expect(checkboxes.length).toBe(10);
  });

  it('saves notification preferences via server API', async () => {
    api.updateNotificationPreferences.mockResolvedValue({});
    const { findByText } = r();
    const prefsTab = await findByText('Notification Preferences');
    await fireEvent.click(prefsTab);
    const saveBtn = await findByText('Save Preferences');
    await fireEvent.click(saveBtn);
    expect(api.updateNotificationPreferences).toHaveBeenCalled();
    const arg = api.updateNotificationPreferences.mock.calls[0][0];
    expect(arg.SpecApproval).toBe(true);
  });

  it('shows notifications in Notifications tab', async () => {
    const { findByText } = r();
    const notifTab = await findByText('Notifications');
    await fireEvent.click(notifTab);
    expect(await findByText('Spec approved')).toBeTruthy();
    expect(await findByText('Agent crashed')).toBeTruthy();
  });

  it('shows mark-as-read button for unread notifications', async () => {
    const { findByText, container } = r();
    const notifTab = await findByText('Notifications');
    await fireEvent.click(notifTab);
    await findByText('Spec approved');
    const markBtns = container.querySelectorAll('.mark-read-btn');
    expect(markBtns.length).toBe(1); // only 1 unread
  });

  it('marks notification as read when button is clicked', async () => {
    const { findByText, container } = r();
    const notifTab = await findByText('Notifications');
    await fireEvent.click(notifTab);
    await findByText('Spec approved');
    const markBtn = container.querySelector('.mark-read-btn');
    await fireEvent.click(markBtn);
    expect(api.markNotificationRead).toHaveBeenCalledWith('n-1');
  });

  it('shows empty state when no notifications', async () => {
    api.myNotifications.mockResolvedValue([]);
    const { findByText } = r();
    const notifTab = await findByText('Notifications');
    await fireEvent.click(notifTab);
    expect(await findByText('No notifications')).toBeTruthy();
  });

  it('shows empty state when no workspaces', async () => {
    api.workspaces.mockResolvedValue([]);
    const { findByText } = r();
    const wsTab = await findByText('Workspaces');
    await fireEvent.click(wsTab);
    expect(await findByText('No workspaces')).toBeTruthy();
  });

  it('calls all four API endpoints on mount', async () => {
    r();
    await waitFor(() => {
      expect(api.me).toHaveBeenCalledTimes(1);
      expect(api.myNotifications).toHaveBeenCalledTimes(1);
      expect(api.myJudgments).toHaveBeenCalledTimes(1);
      expect(api.workspaces).toHaveBeenCalledTimes(1);
    });
  });
});
