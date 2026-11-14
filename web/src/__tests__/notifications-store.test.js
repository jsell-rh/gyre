import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  notificationState,
  parseSseFrame,
  refreshUnreadCount,
  startNotificationStream,
  stopNotificationStream,
} from '../lib/notifications.svelte.js';

// A ReadableStream that yields the given chunks (simulating SSE frames
// arriving over the wire, possibly split mid-frame). The stream never
// closes — a live SSE connection stays open until aborted.
function streamOf(chunks) {
  const encoder = new TextEncoder();
  let i = 0;
  return new ReadableStream({
    pull(controller) {
      if (i >= chunks.length) return; // pending read: stream stays open
      controller.enqueue(encoder.encode(chunks[i++]));
    },
  });
}

function sseResponse(chunks) {
  return {
    ok: true,
    body: streamOf(chunks),
  };
}

describe('notifications.svelte.js (bell store + SSE)', () => {
  beforeEach(() => {
    notificationState.unread = 0;
    notificationState.streamStatus = 'closed';
    notificationState.lastEventAt = 0;
    stopNotificationStream();
    localStorage.clear();
  });

  afterEach(() => {
    stopNotificationStream();
  });

  it('parseSseFrame extracts event and data lines', () => {
    const frame = parseSseFrame('event: notification\ndata: {"id":"n-1"}');
    expect(frame.event).toBe('notification');
    expect(frame.data).toBe('{"id":"n-1"}');
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

  it('the live stream bumps the unread count for unread notifications only', async () => {
    const frames = [
      ': heartbeat\n\n',
      'event: notification\ndata: {"id":"n-1","read":false}\n\n',
      'event: notification\ndata: {"id":"n-2","read":true}\n\n',
      // A frame split across two network chunks must still parse.
      'event: notification\ndata: {"id":"n-3","rea',
      'd":false}\n\n',
    ];
    global.fetch = vi.fn().mockResolvedValue(sseResponse(frames));

    const stop = startNotificationStream();
    expect(typeof stop).toBe('function');
    await vi.waitFor(() => {
      expect(notificationState.streamStatus).toBe('open');
      expect(notificationState.unread).toBe(2); // n-1 and n-3, not n-2
      expect(notificationState.lastEventAt).toBeGreaterThan(0);
    });
    stop();
  });

  it('a failed stream does not throw and marks the status closed', async () => {
    global.fetch = vi.fn().mockRejectedValue(new Error('network down'));
    startNotificationStream();
    await vi.waitFor(() => expect(notificationState.streamStatus).toBe('closed'));
    expect(notificationState.unread).toBe(0);
  });
});
