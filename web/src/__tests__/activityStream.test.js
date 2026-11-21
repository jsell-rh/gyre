import { describe, it, expect, vi, beforeEach } from 'vitest';

// The rider imports api.js for the reconnect re-sync — mock the module so
// the test observes (and drives) the re-fetch.
const activityMock = vi.fn().mockResolvedValue([]);
vi.mock('../lib/api.js', () => ({
  api: {
    activity: (...args) => activityMock(...args),
  },
}));

import {
  activityStreamState,
  attachActivityStream,
  detachActivityStream,
  resetActivityStream,
  busMessageToActivityEntry,
  mergeFetchedActivity,
} from '../lib/activityStream.svelte.js';

// A fake ws-store (web/src/lib/ws.js shape): registries of message/status
// listeners the test drives, exactly like the real socket callbacks.
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

/** A bus Message in the ws.rs wire shape (gyre_common::message::Message). */
function busMessage(overrides = {}) {
  return {
    id: 'msg-1',
    tenant_id: 't-1',
    from: { agent: 'agent-9' },
    workspace_id: 'ws-1',
    to: { workspace: 'ws-1' },
    kind: 'agent_completed',
    payload: { agent_id: 'agent-9', mr_id: 'mr-7', mr_title: 'Add payments' },
    created_at: 1_700_000_123_456, // epoch milliseconds
    ...overrides,
  };
}

describe('activityStream.svelte.js (real-time activity feed rider)', () => {
  let store;

  beforeEach(() => {
    activityMock.mockReset();
    activityMock.mockResolvedValue([]);
    resetActivityStream();
    detachActivityStream();
    store = fakeWsStore();
  });

  it('live-prepends a bus message emitted for the subscribed workspace', () => {
    attachActivityStream(store);
    store.emit(busMessage());
    const bucket = activityStreamState.byWorkspace['ws-1'];
    expect(bucket).toHaveLength(1);
    const entry = bucket[0];
    // Same entry shape the /activity handler returns.
    expect(entry.event_type).toBe('agent_completed');
    expect(entry.entity_type).toBe('mr');
    expect(entry.entity_id).toBe('mr-7');
    expect(entry.mr_id).toBe('mr-7');
    expect(entry.agent_id).toBe('agent-9');
    expect(entry.entity_name).toBe('Add payments');
    expect(entry.repo_id).toBeNull();
    expect(entry.live).toBe(true);
    // Bus timestamps are milliseconds; activity entries are seconds.
    expect(entry.timestamp).toBe(1_700_000_123);
  });

  it('a message for a DIFFERENT workspace does not land in W\'s bucket', () => {
    attachActivityStream(store);
    store.emit(busMessage({ id: 'msg-other', workspace_id: 'ws-2', to: { workspace: 'ws-2' } }));
    expect(activityStreamState.byWorkspace['ws-1']).toBeUndefined();
    expect(activityStreamState.byWorkspace['ws-2']).toHaveLength(1);
  });

  it('dedupes by bus message id (reconnect replay must not duplicate)', () => {
    attachActivityStream(store);
    store.emit(busMessage());
    store.emit(busMessage()); // same id again — e.g. server replay after reconnect
    expect(activityStreamState.byWorkspace['ws-1']).toHaveLength(1);
  });

  it('ignores non-bus frames (NotificationPush, presence, pongs)', () => {
    attachActivityStream(store);
    store.emit({ type: 'NotificationPush', notification: { read: false } });
    store.emit({ type: 'UserPresence', user_id: 'u1', view: 'home' });
    store.emit({ type: 'Pong', timestamp: 1 });
    expect(Object.keys(activityStreamState.byWorkspace)).toHaveLength(0);
  });

  it('maps artifact refs from the payload (gate refs, commit, branch)', () => {
    const entry = busMessageToActivityEntry(
      busMessage({
        kind: 'gate_failure',
        payload: {
          mr_id: 'mr-7',
          gate_name: 'spec-coverage',
          commit_sha: '0123456789abcdef0123456789abcdef01234567',
          branch: 'feature/payments',
          repo_id: 'repo-3',
        },
      })
    );
    expect(entry.gate_name).toBe('spec-coverage');
    expect(entry.commit_sha).toBe('0123456789abcdef0123456789abcdef01234567');
    expect(entry.branch).toBe('feature/payments');
    expect(entry.repo_id).toBe('repo-3');
  });

  it('returns null for messages without a workspace scope', () => {
    const entry = busMessageToActivityEntry(busMessage({ workspace_id: null }));
    expect(entry).toBeNull();
    expect(busMessageToActivityEntry(null)).toBeNull();
    expect(busMessageToActivityEntry({ type: 'Pong' })).toBeNull();
  });

  it('mergeFetchedActivity merges fetched pages newest-first, deduped against live entries', () => {
    attachActivityStream(store);
    store.emit(busMessage({ id: 'live-1', created_at: 1_700_000_200_000 }));

    mergeFetchedActivity('ws-1', [
      // Older fetched entry — sorted after the live one.
      { event_type: 'mr_merged', entity_id: 'mr-7', timestamp: 1_700_000_100, workspace_id: 'ws-1' },
      // Same identity as a later fetch — deduped by the tuple key.
      { event_type: 'mr_merged', entity_id: 'mr-7', timestamp: 1_700_000_100, workspace_id: 'ws-1' },
    ]);

    const bucket = activityStreamState.byWorkspace['ws-1'];
    expect(bucket).toHaveLength(2);
    expect(bucket[0].id).toBe('live-1'); // newest first
    expect(bucket[0].live).toBe(true);
    expect(bucket[1].event_type).toBe('mr_merged');
    expect(bucket[1].live).toBe(false);
  });

  it('re-syncs via GET /api/v1/activity?since=<newest timestamp> on reconnect', async () => {
    attachActivityStream(store);
    // Initial connection: no re-sync (the components' first load covers it).
    store.setStatus('connected');
    expect(activityMock).not.toHaveBeenCalled();

    store.emit(busMessage({ id: 'live-1', created_at: 1_700_000_200_000 }));

    // Outage and reconnect: the missed window is re-fetched with since =
    // the newest tracked entry timestamp (epoch seconds).
    activityMock.mockResolvedValue([
      { event_type: 'spec_approved', entity_id: 's.md', timestamp: 1_700_000_300, workspace_id: 'ws-1' },
    ]);
    store.setStatus('disconnected');
    store.setStatus('connected');

    await vi.waitFor(() => {
      expect(activityMock).toHaveBeenCalledWith(100, 1_700_000_200);
    });
    // The re-synced entry landed in the workspace bucket.
    await vi.waitFor(() => {
      const bucket = activityStreamState.byWorkspace['ws-1'];
      expect(bucket.some((e) => e.event_type === 'spec_approved')).toBe(true);
    });
  });

  it('failed re-sync leaves tracked entries intact', async () => {
    attachActivityStream(store);
    store.setStatus('connected');
    store.emit(busMessage());
    activityMock.mockRejectedValue(new Error('offline'));
    store.setStatus('disconnected');
    store.setStatus('connected');
    await vi.waitFor(() => {
      expect(activityMock).toHaveBeenCalled();
    });
    expect(activityStreamState.byWorkspace['ws-1']).toHaveLength(1);
  });

  it('detach stops tracking and is idempotent to attach', () => {
    const detach = attachActivityStream(store);
    detach();
    store.emit(busMessage());
    expect(activityStreamState.byWorkspace['ws-1']).toBeUndefined();
    // Re-attach works after detach.
    attachActivityStream(store);
    store.emit(busMessage({ id: 'msg-2' }));
    expect(activityStreamState.byWorkspace['ws-1']).toHaveLength(1);
  });
});
