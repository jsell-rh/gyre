import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor, fireEvent } from '@testing-library/svelte';
import NotificationBell from '../components/NotificationBell.svelte';
import { notificationState } from '../lib/notifications.svelte.js';

vi.mock('../lib/api.js', () => ({
  api: {
    notifications: vi.fn().mockResolvedValue({ notifications: [] }),
    markNotificationRead: vi.fn(),
    markNotificationUnread: vi.fn(),
    markAllNotificationsRead: vi.fn(),
    workspaceRepos: vi.fn().mockResolvedValue([]),
  },
}));
vi.mock('../lib/toast.svelte.js', () => ({ toastError: vi.fn(), toast: vi.fn() }));

describe('NotificationBell', () => {
  beforeEach(() => {
    notificationState.unread = 0;
    notificationState.streamStatus = 'closed';
    notificationState.lastEventAt = 0;
    // The bell starts the SSE stream on mount — neutralize it.
    global.fetch = vi.fn().mockResolvedValue({ ok: false, body: null });
  });

  it('shows no badge when there is nothing unread', async () => {
    const { container } = render(NotificationBell, { workspaces: [] });
    await waitFor(() => expect(container.querySelector('[data-testid="notification-bell"]')).toBeTruthy());
    expect(container.querySelector('[data-testid="notification-badge"]')).toBeFalsy();
  });

  it('shows the unread count badge from the shared store', async () => {
    const { container } = render(NotificationBell, { workspaces: [] });
    notificationState.unread = 3;
    await waitFor(() => {
      expect(container.querySelector('[data-testid="notification-badge"]').textContent).toBe('3');
    });
    // 99+ cap for wide counts.
    notificationState.unread = 150;
    await waitFor(() => {
      expect(container.querySelector('[data-testid="notification-badge"]').textContent).toBe('99+');
    });
  });

  it('clicking the bell opens the notification drawer', async () => {
    const { container } = render(NotificationBell, { workspaces: [] });
    await waitFor(() => expect(container.querySelector('[data-testid="notification-bell"]')).toBeTruthy());

    await fireEvent.click(container.querySelector('[data-testid="notification-bell"]'));
    await waitFor(() => {
      expect(container.querySelector('[data-testid="notification-drawer"]')).toBeTruthy();
    });
    expect(
      container.querySelector('[data-testid="notification-bell"]').getAttribute('aria-expanded')
    ).toBe('true');
  });
});
