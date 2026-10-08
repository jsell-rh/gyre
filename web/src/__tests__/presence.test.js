import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  createPresenceHeartbeat,
  sendEditingPresence,
  specEditingEntity,
} from '../lib/presence.js';

function fakeStore() {
  return { send: vi.fn(), sessionId: 'sess-1' };
}

describe('sendEditingPresence', () => {
  it('sends a UserPresence heartbeat with editing_entity when editing', () => {
    const store = fakeStore();
    sendEditingPresence(store, { workspaceId: 'ws1', editingEntity: 'spec:specs/a.md' });
    expect(store.send).toHaveBeenCalledTimes(1);
    const msg = store.send.mock.calls[0][0];
    expect(msg.type).toBe('UserPresence');
    expect(msg.session_id).toBe('sess-1');
    expect(msg.workspace_id).toBe('ws1');
    expect(msg.editing_entity).toBe('spec:specs/a.md');
    expect(typeof msg.timestamp).toBe('number');
  });

  it('omits editing_entity when clearing (null) so other banners disappear', () => {
    const store = fakeStore();
    sendEditingPresence(store, { workspaceId: 'ws1', editingEntity: null });
    expect(store.send).toHaveBeenCalledTimes(1);
    const msg = store.send.mock.calls[0][0];
    expect('editing_entity' in msg).toBe(false);
    // Still a valid presence heartbeat for the workspace.
    expect(msg.workspace_id).toBe('ws1');
  });

  it('no-ops when the store is missing send/sessionId or workspace is absent', () => {
    const store = fakeStore();
    sendEditingPresence(null, { workspaceId: 'ws1', editingEntity: 'spec:a.md' });
    sendEditingPresence({ sessionId: 's' }, { workspaceId: 'ws1', editingEntity: 'spec:a.md' });
    sendEditingPresence(store, { workspaceId: '', editingEntity: 'spec:a.md' });
    expect(store.send).not.toHaveBeenCalled();
  });
});

describe('specEditingEntity', () => {
  it('prefixes a spec path with the spec: entity scheme', () => {
    expect(specEditingEntity('specs/system/auth.md')).toBe('spec:specs/system/auth.md');
  });

  it('returns null for an empty path', () => {
    expect(specEditingEntity('')).toBeNull();
    expect(specEditingEntity(null)).toBeNull();
  });
});

describe('createPresenceHeartbeat', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  function heartbeatStore() {
    const statusCbs = [];
    const messageCbs = [];
    return {
      send: vi.fn(),
      sessionId: 'sess-1',
      onStatus: (cb) => {
        statusCbs.push(cb);
        return () => {};
      },
      onMessage: (cb) => {
        messageCbs.push(cb);
        return () => {};
      },
      _setStatus: (s) => statusCbs.forEach((cb) => cb(s)),
      _emitMessage: (m) => messageCbs.forEach((cb) => cb(m)),
    };
  }

  function makeHb(store, opts = {}) {
    return createPresenceHeartbeat(store, {
      getWorkspaceId: () => 'ws1',
      getView: () => opts.view ?? 'specs',
      getEditingEntity: () => opts.editingEntity ?? null,
    });
  }

  // Leg 1: send immediately after the connection is established.
  it('sends presence when the socket connects', () => {
    const store = heartbeatStore();
    const hb = makeHb(store);
    store._setStatus('connected');
    expect(store.send).toHaveBeenCalledTimes(1);
    const msg = store.send.mock.calls[0][0];
    expect(msg.type).toBe('UserPresence');
    expect(msg.view).toBe('specs');
    hb.destroy();
  });

  // Leg 1 variant: reconnect re-sends (outside the debounce window).
  it('re-sends on reconnect after the debounce window has elapsed', () => {
    const store = heartbeatStore();
    const hb = makeHb(store);
    store._setStatus('connected');
    vi.advanceTimersByTime(6_000);
    store._setStatus('disconnected');
    store._setStatus('connected');
    expect(store.send).toHaveBeenCalledTimes(2);
    hb.destroy();
  });

  // Leg 2: 30-second periodic heartbeat keeps the entry alive against the
  // server's 60s idle eviction.
  it('sends a heartbeat every 30 seconds', () => {
    const store = heartbeatStore();
    const hb = makeHb(store);
    vi.advanceTimersByTime(30_000);
    expect(store.send).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(30_000);
    expect(store.send).toHaveBeenCalledTimes(2);
    hb.destroy();
  });

  // Leg 2 invariant: the timer is never starved by the debounce — even a
  // send 1 second ago does not suppress the 30s beat.
  it('timer heartbeat bypasses the 5s debounce', () => {
    const store = heartbeatStore();
    const hb = makeHb(store);
    store._setStatus('connected'); // send #1 at t=0
    vi.advanceTimersByTime(26_000); // last send 26s ago — outside the debounce window
    hb.notifyViewChange(); // sends #2 at t=26s
    vi.advanceTimersByTime(4_000); // timer fires at t=30s, only 4s after the last send
    expect(store.send).toHaveBeenCalledTimes(3); // the beat must NOT be suppressed by the debounce
    hb.destroy();
  });

  // Leg 3: view change re-sends, debounced to at most one per 5 seconds.
  it('re-sends presence on view change', () => {
    const store = heartbeatStore();
    let view = 'specs';
    const hb = createPresenceHeartbeat(store, {
      getWorkspaceId: () => 'ws1',
      getView: () => view,
      getEditingEntity: () => null,
    });
    vi.advanceTimersByTime(10_000); // outside any debounce window
    view = 'inbox';
    hb.notifyViewChange();
    expect(store.send).toHaveBeenCalledTimes(1);
    expect(store.send.mock.calls[0][0].view).toBe('inbox');
    hb.destroy();
  });

  it('debounces view changes to one per 5 seconds', () => {
    const store = heartbeatStore();
    let view = 'specs';
    const hb = createPresenceHeartbeat(store, {
      getWorkspaceId: () => 'ws1',
      getView: () => view,
      getEditingEntity: () => null,
    });
    vi.advanceTimersByTime(10_000);
    view = 'inbox';
    hb.notifyViewChange(); // sent
    view = 'explorer';
    hb.notifyViewChange(); // within 5s → suppressed
    expect(store.send).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(5_000);
    hb.notifyViewChange(); // window elapsed → sent
    expect(store.send).toHaveBeenCalledTimes(2);
    expect(store.send.mock.calls[1][0].view).toBe('explorer');
    hb.destroy();
  });

  // Leg 3 data: the beat carries the CURRENT editing entity, so a heartbeat
  // never clobbers a live concurrent-editing announcement.
  it('heartbeat re-sends the current editing entity', () => {
    const store = heartbeatStore();
    let editing = null;
    const hb = createPresenceHeartbeat(store, {
      getWorkspaceId: () => 'ws1',
      getView: () => 'specs',
      getEditingEntity: () => editing,
    });
    vi.advanceTimersByTime(5_000);
    editing = 'spec:specs/a.md';
    vi.advanceTimersByTime(25_000); // 30s beat
    expect(store.send).toHaveBeenCalledTimes(1);
    expect(store.send.mock.calls[0][0].editing_entity).toBe('spec:specs/a.md');
    hb.destroy();
  });

  it('pauses (no send) when no workspace is in scope', () => {
    const store = heartbeatStore();
    const hb = createPresenceHeartbeat(store, {
      getWorkspaceId: () => null,
      getView: () => 'specs',
      getEditingEntity: () => null,
    });
    store._setStatus('connected');
    vi.advanceTimersByTime(60_000);
    hb.notifyViewChange();
    expect(store.send).not.toHaveBeenCalled();
    hb.destroy();
  });

  // Leg 4: beforeunload sends the graceful disconnect.
  it('sends view="disconnected" on beforeunload', () => {
    const store = heartbeatStore();
    const hb = makeHb(store);
    window.dispatchEvent(new Event('beforeunload'));
    expect(store.send).toHaveBeenCalledTimes(1);
    expect(store.send.mock.calls[0][0].view).toBe('disconnected');
    hb.destroy();
  });

  // Eviction leg (HSI §1): a targeted PresenceEvicted naming THIS tab's
  // session stops heartbeating for that tab — the server dropped the entry
  // (5-session cap / idle sweep) and the client must not re-insert it.
  it('stops heartbeating after PresenceEvicted names its own session', () => {
    const store = heartbeatStore();
    const hb = makeHb(store);
    store._setStatus('connected'); // send #1
    store._emitMessage({ type: 'PresenceEvicted', session_id: 'sess-1' });
    vi.advanceTimersByTime(120_000); // two 30s beats would have fired
    hb.notifyViewChange();
    hb.sendNow();
    window.dispatchEvent(new Event('beforeunload'));
    expect(store.send).toHaveBeenCalledTimes(1); // only the pre-eviction send
    hb.destroy();
  });

  // Eviction must be session-scoped: another tab's PresenceEvicted must not
  // silence this tab's heartbeat (spec: "stops heartbeating only for that tab").
  it('keeps heartbeating when PresenceEvicted names a different session', () => {
    const store = heartbeatStore();
    const hb = makeHb(store);
    store._setStatus('connected'); // send #1
    store._emitMessage({ type: 'PresenceEvicted', session_id: 'sess-other' });
    vi.advanceTimersByTime(30_000);
    expect(store.send).toHaveBeenCalledTimes(2); // beat still fires
    hb.destroy();
  });

  it('destroy stops the timer and removes the unload listener', () => {
    const store = heartbeatStore();
    const hb = makeHb(store);
    hb.destroy();
    vi.advanceTimersByTime(120_000);
    window.dispatchEvent(new Event('beforeunload'));
    expect(store.send).not.toHaveBeenCalled();
  });

  it('returns a no-op triple for a null store', () => {
    const hb = createPresenceHeartbeat(null, {
      getWorkspaceId: () => 'ws1',
      getView: () => 'specs',
    });
    expect(() => {
      hb.notifyViewChange();
      hb.sendNow();
      hb.destroy();
    }).not.toThrow();
  });
});
