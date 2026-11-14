// Notification bell state + real-time subscription
// (user-management.md §In-App Notification UI).
//
// Single shared store: the bell badge, the drawer, and the preferences page
// all read `notificationState`. Real-time delivery rides the app's ONE
// authenticated /ws WebSocket connection (created in App.svelte and shared
// with presence/liveness): the server pushes every notification the moment
// it is persisted as a `NotificationPush` message, targeted at connections
// whose verified identity is the notification's owner.
//
// GET /api/v1/notifications/stream remains the specced SSE HTTP API; the
// SPA deliberately does not hold a second always-open HTTP stream per tab —
// one realtime channel per tab, and a read/unread action anywhere refreshes
// the count from the server so it never drifts.

const AUTH_TOKEN_KEY = 'gyre_auth_token';
const API_BASE = '/api/v1';

function getAuthToken() {
  return localStorage.getItem(AUTH_TOKEN_KEY) || 'gyre-dev-token';
}

export const notificationState = $state({
  unread: 0,
  // 'open' | 'closed' — 'open' while the shared /ws connection is
  // authenticated; surfaced so the drawer can show a live-connection
  // indicator.
  streamStatus: 'closed',
  // Last notification delivered in real time (consumed by the drawer for a
  // "new" flash; kept minimal — the drawer reloads its list on change).
  lastEventAt: 0,
});

let detachStream = null;

export async function refreshUnreadCount(workspaceId = null) {
  try {
    const qs = workspaceId ? `?workspace_id=${encodeURIComponent(workspaceId)}` : '';
    const res = await fetch(`${API_BASE}/notifications/unread-count${qs}`, {
      headers: { Authorization: `Bearer ${getAuthToken()}` },
    });
    if (!res.ok) return;
    const data = await res.json();
    notificationState.unread = data?.count ?? 0;
  } catch {
    // Offline / logged out — keep the last known count.
  }
}

/**
 * Subscribe the shared store to a ws-store connection
 * (web/src/lib/ws.js). Every `NotificationPush` delivered on the socket
 * bumps the unread count (unread notifications only); the connection
 * status drives `streamStatus`. Idempotent — attaching while already
 * attached is a no-op. Returns a detach function.
 */
export function attachNotificationStream(store) {
  if (detachStream) return detachStream;

  const offMessage = store.onMessage((msg) => {
    if (!msg || msg.type !== 'NotificationPush' || !msg.notification) return;
    const notif = msg.notification;
    // The server targets connections by verified user identity, so this
    // is the caller's own notification. Unread here means "not read yet" —
    // the bell counts unread.
    if (!notif.read) notificationState.unread += 1;
    notificationState.lastEventAt = Date.now();
  });

  const offStatus = store.onStatus((s) => {
    // ws-store statuses: 'connected' | 'disconnected' | 'auth-failed' |
    // 'error'. Only an authenticated connection receives pushes.
    notificationState.streamStatus = s === 'connected' ? 'open' : 'closed';
  });

  detachStream = () => {
    offMessage();
    offStatus();
    detachStream = null;
    notificationState.streamStatus = 'closed';
  };
  return detachStream;
}

/** Detach the real-time subscription (App teardown). */
export function detachNotificationStream() {
  if (detachStream) detachStream();
}
