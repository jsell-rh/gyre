// Notification bell state + real-time SSE subscription
// (user-management.md §In-App Notification UI).
//
// Single shared store: the bell badge, the drawer, and the preferences page
// all read `notificationState`. The SSE stream
// (GET /api/v1/notifications/stream) is consumed with fetch + ReadableStream
// (NOT EventSource — the endpoint requires the Authorization header, which
// EventSource cannot send). Every delivered notification bumps the unread
// count; a read/unread action anywhere refreshes it from the server so the
// count never drifts.

const AUTH_TOKEN_KEY = 'gyre_auth_token';
const API_BASE = '/api/v1';

function getAuthToken() {
  return localStorage.getItem(AUTH_TOKEN_KEY) || 'gyre-dev-token';
}

export const notificationState = $state({
  unread: 0,
  // 'connecting' | 'open' | 'closed' — surfaced so the drawer can show a
  // live-connection indicator.
  streamStatus: 'closed',
  // Last notification delivered by the stream (consumed by the drawer for a
  // "new" flash; kept minimal — the drawer reloads its list on change).
  lastEventAt: 0,
});

let streamController = null;

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
 * Parse one SSE frame (a block of lines terminated by a blank line) into
 * { event, data }. Exposed for tests.
 */
export function parseSseFrame(frame) {
  let event = 'message';
  let data = '';
  for (const line of frame.split('\n')) {
    if (line.startsWith('event:')) event = line.slice(6).trim();
    else if (line.startsWith('data:')) data += (data ? '\n' : '') + line.slice(5).trim();
  }
  return { event, data };
}

/**
 * Start the real-time notification stream. Idempotent — a second call while
 * connected is a no-op. Returns a stop() function.
 */
export function startNotificationStream() {
  if (streamController) return () => stopNotificationStream();
  streamController = new AbortController();
  const controller = streamController;
  notificationState.streamStatus = 'connecting';

  (async () => {
    try {
      const res = await fetch(`${API_BASE}/notifications/stream`, {
        headers: { Authorization: `Bearer ${getAuthToken()}` },
        signal: controller.signal,
      });
      if (!res.ok || !res.body) {
        notificationState.streamStatus = 'closed';
        return;
      }
      notificationState.streamStatus = 'open';

      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buffer = '';
      // eslint-disable-next-line no-constant-condition
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        // Frames are separated by a blank line. The buffer may hold several
        // complete frames plus a partial tail.
        let sep;
        while ((sep = buffer.indexOf('\n\n')) !== -1) {
          const frame = buffer.slice(0, sep);
          buffer = buffer.slice(sep + 2);
          if (!frame.trim() || frame.startsWith(':')) continue; // heartbeat comment
          const { event, data } = parseSseFrame(frame);
          if (event === 'notification' && data) {
            try {
              const notif = JSON.parse(data);
              // The server filters to the caller's user + tenant; unread
              // here means "not read yet" — the bell counts unread.
              if (notif && !notif.read) notificationState.unread += 1;
              notificationState.lastEventAt = Date.now();
            } catch {
              // Malformed payload — ignore, never kill the stream.
            }
          }
        }
      }
    } catch {
      // Aborted or network failure — fall back to polling semantics (the
      // count is refreshed on drawer open and navigation).
    } finally {
      if (!controller.signal.aborted) notificationState.streamStatus = 'closed';
    }
  })();

  return () => stopNotificationStream();
}

export function stopNotificationStream() {
  if (streamController) {
    streamController.abort();
    streamController = null;
  }
  notificationState.streamStatus = 'closed';
}
