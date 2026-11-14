/**
 * NotificationDrawer.test.js — slide-out notification panel
 * (user-management.md §UI Pages > Notification Drawer).
 *
 * Covers:
 *   - Opens and loads recent notifications from GET /notifications
 *   - Groups by priority band (Urgent/High/Medium/Low)
 *   - Unread badge count reflects unread items
 *   - Deep link click calls onnavigate with the action_url
 *   - InvitationReceived deep-links to /invite/{workspace_id}
 *   - Mark read / unread calls the task-125 endpoints
 *   - Mark all read calls the bulk endpoint
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';

vi.mock('../lib/api.js', () => ({
  api: {
    notifications: vi.fn(),
    markNotificationRead: vi.fn(),
    markNotificationUnread: vi.fn(),
    markAllNotificationsRead: vi.fn(),
  },
  setAuthToken: vi.fn(),
}));

// Stub svelte-i18n: keys pass through; the unread-count key interpolates
// its {n} so counts are assertable.
vi.mock('svelte-i18n', () => ({
  t: {
    subscribe: (fn) => {
      fn((id, opts) => {
        if (id === 'notification_drawer.unread_count') return `${opts?.values?.n ?? ''} unread`;
        return id.replace(/\{n\}/g, String(opts?.values?.n ?? ''));
      });
      return () => {};
    },
  },
}));

import NotificationDrawer from '../components/NotificationDrawer.svelte';
import { api } from '../lib/api.js';

const NOTIFICATIONS = {
  notifications: [
    { id: 'n-1', title: 'Security finding', body: 'critical', priority_level: 'Urgent', read: false, action_url: '/workspaces/payments/r/api/mrs/mr-1', created_at: 1700000000 },
    { id: 'n-2', title: 'Gate failed', body: 'gate X', priority_level: 'High', read: false, action_url: null, created_at: 1700000000 },
    { id: 'n-3', title: 'Spec drift', body: 'drifted', priority_level: 'Medium', read: true, action_url: null, created_at: 1700000000 },
    { id: 'n-4', title: 'MR merged', body: 'merged', priority_level: 'Low', read: true, action_url: null, created_at: 1700000000 },
    { id: 'n-5', title: 'You have been invited to workspace ws-9', body: null, priority_level: 'Low', notification_type: 'InvitationReceived', workspace_id: 'ws-9', read: false, action_url: null, created_at: 1700000000 },
  ],
};

async function renderOpen(props = {}) {
  const rendered = render(NotificationDrawer, { props: { open: true, ...props } });
  await waitFor(() => expect(api.notifications).toHaveBeenCalled());
  return rendered;
}

describe('NotificationDrawer', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.notifications.mockResolvedValue({ ...NOTIFICATIONS });
    api.markNotificationRead.mockResolvedValue({});
    api.markNotificationUnread.mockResolvedValue({});
    api.markAllNotificationsRead.mockResolvedValue({ marked: 3 });
  });

  it('loads recent notifications when opened', async () => {
    const { container } = await renderOpen();
    expect(api.notifications).toHaveBeenCalledWith({ limit: 50 });
    expect(container.querySelector('[data-testid="notification-drawer"]')).toBeTruthy();
  });

  it('groups notifications by priority band', async () => {
    const { container } = await renderOpen();
    expect(container.querySelector('[data-testid="drawer-group-urgent"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="drawer-group-high"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="drawer-group-medium"]')).toBeTruthy();
    expect(container.querySelector('[data-testid="drawer-group-low"]')).toBeTruthy();
    const urgentItems = container.querySelector('[data-testid="drawer-group-urgent"]')
      .querySelectorAll('[data-testid="drawer-notif-item"]');
    expect(urgentItems.length).toBe(1);
  });

  it('shows the unread count for unread items', async () => {
    const { container } = await renderOpen();
    const badge = container.querySelector('[data-testid="drawer-unread-count"]');
    expect(badge).toBeTruthy();
    expect(badge.textContent).toContain('3');
  });

  it('deep-links via onnavigate with the action_url', async () => {
    const onnavigate = vi.fn();
    const { container } = await renderOpen({ onnavigate });
    const item = container.querySelector('[data-testid="drawer-notif-item"]');
    await fireEvent.click(item.querySelector('.notif-main'));
    expect(onnavigate).toHaveBeenCalledWith('/workspaces/payments/r/api/mrs/mr-1', expect.objectContaining({ id: 'n-1' }));
  });

  it('deep-links InvitationReceived to the invitation accept page', async () => {
    const onnavigate = vi.fn();
    const { container } = await renderOpen({ onnavigate });
    const items = Array.from(container.querySelectorAll('[data-testid="drawer-notif-item"]'));
    const invite = items.find((el) => el.textContent.includes('invited'));
    await fireEvent.click(invite.querySelector('.notif-main'));
    expect(onnavigate).toHaveBeenCalledWith('/invite/ws-9', expect.objectContaining({ id: 'n-5' }));
  });

  it('marks an unread notification read via the API', async () => {
    const { container } = await renderOpen();
    await fireEvent.click(container.querySelector('[data-testid="drawer-toggle-read-n-2"]'));
    await waitFor(() => {
      expect(api.markNotificationRead).toHaveBeenCalledWith('n-2');
    });
  });

  it('marks a read notification unread via the API', async () => {
    const { container } = await renderOpen();
    await fireEvent.click(container.querySelector('[data-testid="drawer-toggle-read-n-3"]'));
    await waitFor(() => {
      expect(api.markNotificationUnread).toHaveBeenCalledWith('n-3');
    });
  });

  it('marks all notifications read in bulk', async () => {
    const { container } = await renderOpen();
    await fireEvent.click(container.querySelector('[data-testid="drawer-mark-all-read"]'));
    await waitFor(() => {
      expect(api.markAllNotificationsRead).toHaveBeenCalledWith();
    });
  });
});
