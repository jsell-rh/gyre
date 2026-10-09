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
    notificationCount: vi.fn(),
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
  oidc_issuer: 'https://idp.example.com/realms/gyre',
  last_login_at: 1759000000,
};

const WORKSPACES = [
  { id: 'ws-1', name: 'Payments', slug: 'payments', trust_level: 'autonomous', role: 'admin' },
  { id: 'ws-2', name: 'Platform', slug: 'platform', trust_level: null, role: 'member' },
];

// Real GET /users/me/notifications wire shape: NotificationResponse items.
// Read state is resolved_at/dismissed_at (both null = unread), NOT a `read` bool.
const NOW = Math.floor(Date.now() / 1000);
const NOTIFICATIONS = [
  { id: 'n-1', notification_type: 'SpecPendingApproval', title: 'Spec approved', resolved_at: null, dismissed_at: null, created_at: (NOW - 300) },
  { id: 'n-2', notification_type: 'GateFailure', title: 'Gate crashed', resolved_at: null, dismissed_at: NOW - 100, created_at: (NOW - 7200) },
];

// Real GET /users/me/judgments wire shape: JudgmentEntryResponse items.
const JUDGMENTS = [
  { judgment_type: 'approval', entity_ref: 'specs/auth.md', workspace_id: 'ws-1', timestamp: NOW - 60, detail: null },
  { judgment_type: 'gate', entity_ref: 'mr-9', workspace_id: 'ws-1', timestamp: NOW - 86400, detail: 'gate test_command overridden: failed -> overridden' },
];

const CTX = new Map([['navigate', vi.fn()], ['goToWorkspaceHome', vi.fn()], ['openDetailPanel', vi.fn()]]);
const r = (props = {}) => render(UserProfile, { props, context: CTX });

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  api.me.mockResolvedValue({ ...ME });
  api.myNotifications.mockResolvedValue({ notifications: [...NOTIFICATIONS] });
  api.myJudgments.mockResolvedValue({ judgments: [...JUDGMENTS] });
  api.myAgents.mockResolvedValue([]);
  api.myTasks.mockResolvedValue([]);
  api.myMrs.mockResolvedValue([]);
  api.workspaces.mockResolvedValue([...WORKSPACES]);
  api.updateMe.mockResolvedValue({ ...ME, display_name: 'Updated Name' });
  api.markNotificationRead.mockResolvedValue({});
  api.notificationCount.mockResolvedValue(1);
  api.getNotificationPreferences.mockResolvedValue({ preferences: [] });
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

  it('shows read-only OIDC issuer in Profile tab', async () => {
    const { findByText } = r();
    expect(await findByText('https://idp.example.com/realms/gyre')).toBeTruthy();
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

  it('badge count comes from the server-side count endpoint, not the list page', async () => {
    const { findByText } = r();
    // notificationCount resolves 7 while the list mock holds only 2 items —
    // a client-side `list.filter(unread)` badge would show a different number.
    api.notificationCount.mockResolvedValue(7);
    expect(await findByText('7')).toBeTruthy();
    expect(api.notificationCount).toHaveBeenCalledTimes(1);
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

  it('shows judgment entries with real wire fields in Judgment Ledger tab', async () => {
    const { findByText } = r();
    const ledgerTab = await findByText('Judgment Ledger');
    await fireEvent.click(ledgerTab);
    // judgment_type / entity_ref are the JudgmentEntryResponse field names.
    expect(await findByText('approval')).toBeTruthy();
    expect(await findByText('gate')).toBeTruthy();
    expect(await findByText('specs/auth.md')).toBeTruthy();
    expect(await findByText('mr-9')).toBeTruthy();
  });

  it('shows judgment detail line when present', async () => {
    const { findByText } = r();
    const ledgerTab = await findByText('Judgment Ledger');
    await fireEvent.click(ledgerTab);
    expect(await findByText('gate test_command overridden: failed -> overridden')).toBeTruthy();
  });

  it('shows empty state when no judgments', async () => {
    api.myJudgments.mockResolvedValue({ judgments: [] });
    const { findByText } = r();
    const ledgerTab = await findByText('Judgment Ledger');
    await fireEvent.click(ledgerTab);
    expect(await findByText(/No activity recorded/)).toBeTruthy();
  });

  it('shows a toggle for every canonical NotificationType (22)', async () => {
    const { findByText, container } = r();
    const prefsTab = await findByText('Notification Preferences');
    await fireEvent.click(prefsTab);
    // Spot-check canonical NotificationType::as_str() names.
    expect(await findByText('Gate Failures')).toBeTruthy();
    expect(await findByText('Specs Pending Approval')).toBeTruthy();
    expect(await findByText('Merge Queue Escalations')).toBeTruthy();
    const checkboxes = container.querySelectorAll('.pref-checkbox');
    expect(checkboxes.length).toBe(22);
  });

  it('loads server-side prefs rows into the toggles', async () => {
    api.getNotificationPreferences.mockResolvedValue({
      preferences: [
        { notification_type: 'GateFailure', enabled: false },
        { notification_type: 'BudgetWarning', enabled: true },
      ],
    });
    const { findByText, container } = r();
    const prefsTab = await findByText('Notification Preferences');
    await fireEvent.click(prefsTab);
    await findByText('Gate Failures');
    const boxes = container.querySelectorAll('.pref-checkbox');
    const gate = [...boxes].find(b => b.getAttribute('aria-label')?.includes('Gate Failures'));
    expect(gate?.checked).toBe(false);
  });

  it('saves prefs with the server wire format and canonical names', async () => {
    const { findByText } = r();
    const prefsTab = await findByText('Notification Preferences');
    await fireEvent.click(prefsTab);
    const saveBtn = await findByText('Save Preferences');
    await fireEvent.click(saveBtn);
    expect(api.updateNotificationPreferences).toHaveBeenCalledTimes(1);
    const arg = api.updateNotificationPreferences.mock.calls[0][0];
    // Backend PUT expects { preferences: [{ notification_type, enabled }] }
    // with canonical names — anything else is a 400.
    expect(Array.isArray(arg.preferences)).toBe(true);
    expect(arg.preferences.length).toBe(22);
    const types = arg.preferences.map(p => p.notification_type);
    expect(types).toContain('GateFailure');
    expect(types).toContain('MergeQueueEscalation');
    // Fabricated names must be gone.
    expect(types).not.toContain('SpecApproval');
    expect(types).not.toContain('MergeRequestMerged');
    for (const p of arg.preferences) {
      expect(typeof p.enabled).toBe('boolean');
    }
  });

  it('shows notifications in Notifications tab', async () => {
    const { findByText } = r();
    const notifTab = await findByText('Notifications');
    await fireEvent.click(notifTab);
    expect(await findByText('Spec approved')).toBeTruthy();
    expect(await findByText('Gate crashed')).toBeTruthy();
  });

  it('treats resolved/dismissed notifications as read', async () => {
    const { findByText, container } = r();
    const notifTab = await findByText('Notifications');
    await fireEvent.click(notifTab);
    await findByText('Spec approved');
    const markBtns = container.querySelectorAll('.mark-read-btn');
    expect(markBtns.length).toBe(1); // n-2 has dismissed_at set
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
    api.myNotifications.mockResolvedValue({ notifications: [] });
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
