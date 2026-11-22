import { describe, it, expect, vi, beforeEach } from 'vitest';
import {
  notificationState,
  attachNotificationStream,
  detachNotificationStream,
  refreshUnreadCount,
} from '../lib/notifications.svelte.js';

// A fake ws-store (web/src/lib/ws.js shape): registries of message/status
// listeners the test can drive, exactly like the real socket callbacks.
function fakeWsStore() {
  const messages = new Set();
  const statuses = new Set();
  return {
    onMessage(cb) {
      messages.add(cb);
      return () => messages.delete(cb);
    },
    onStatus(cb) {
      statuses.add(cb);
      return () => statuses.delete(cb);
    },
    emit(msg) {
      messages.forEach((cb) => cb(msg));
    },
    setStatus(s) {
      statuses.forEach((cb) => cb(s));
    },
  };
}

describe('notifications.svelte.js (bell store + WS realtime)', () => {
  beforeEach(() => {
    notificationState.unread = 0;
    notificationState.streamStatus = 'closed';
    notificationState.lastEventAt = 0;
    detachNotificationStream();
    localStorage.clear();
  });

  it('refreshUnreadCount reads the unread-count endpoint with auth', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: () => Promise.resolve({ count: 7 }),
    });
    global.fetch = fetchMock;
    await refreshUnreadCount();
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/v1/notifications/unread-count',
      expect.objectContaining({
        headers: { Authorization: 'Bearer gyre-dev-token' },
      })
    );
    expect(notificationState.unread).toBe(7);
  });

  it('a NotificationPush bumps the unread count for unread notifications only', () => {
    const store = fakeWsStore();
    attachNotificationStream(store);

    store.setStatus('connected');
    expect(notificationState.streamStatus).toBe('open');

    store.emit({ type: 'UserPresence', user_id: 'someone-else' });
    store.emit({ type: 'NotificationPush', notification: { id: 'n-1', read: false } });
    store.emit({ type: 'NotificationPush', notification: { id: 'n-2', read: true } });
    store.emit({ type: 'NotificationPush' }); // malformed — no notification payload
    store.emit({ type: 'Ping' });

    expect(notificationState.unread).toBe(1); // n-1 only
    expect(notificationState.lastEventAt).toBeGreaterThan(0);
  });

  it('connection status maps to the live indicator', () => {
    const store = fakeWsStore();
    attachNotificationStream(store);

    store.setStatus('connected');
    expect(notificationState.streamStatus).toBe('open');
    store.setStatus('disconnected');
    expect(notificationState.streamStatus).toBe('closed');
    store.setStatus('error');
    expect(notificationState.streamStatus).toBe('closed');
  });

  it('detach stops updates and attaching twice shares one subscription', () => {
    const store = fakeWsStore();
    const detach = attachNotificationStream(store);
    // Second attach on another store is a no-op while attached.
    const other = fakeWsStore();
    const detach2 = attachNotificationStream(other);
    expect(detach2).toBe(detach);

    detach();
    store.emit({ type: 'NotificationPush', notification: { id: 'n-3', read: false } });
    expect(notificationState.unread).toBe(0);
    expect(notificationState.streamStatus).toBe('closed');

    // After detach, a new attach works and delivers again.
    const store3 = fakeWsStore();
    attachNotificationStream(store3);
    store3.emit({ type: 'NotificationPush', notification: { id: 'n-4', read: false } });
    expect(notificationState.unread).toBe(1);
  });
});
