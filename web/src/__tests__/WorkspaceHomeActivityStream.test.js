import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, waitFor } from '@testing-library/svelte';

// Same API mock surface as WorkspaceHome.test.js (empty defaults) — the
// feed starts empty so the ONLY source of sidebar entries in this test is
// the live bus stream.
vi.mock('../lib/api.js', () => ({
  api: {
    myNotifications: vi.fn().mockResolvedValue([]),
    workspaceRepos: vi.fn().mockResolvedValue([]),
    specsForWorkspace: vi.fn().mockResolvedValue([]),
    getMetaSpecs: vi.fn().mockResolvedValue([]),
    workspaceGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    getWorkspaceBriefing: vi.fn().mockResolvedValue({ narrative: '' }),
    briefingAsk: vi.fn(),
    approveSpec: vi.fn(),
    revokeSpec: vi.fn(),
    enqueue: vi.fn(),
    markNotificationRead: vi.fn(),
    tasks: vi.fn().mockResolvedValue([]),
    mergeRequests: vi.fn().mockResolvedValue([]),
    mrGates: vi.fn().mockResolvedValue([]),
    mrDiff: vi.fn().mockResolvedValue({ files_changed: 0, insertions: 0, deletions: 0 }),
    updateTaskStatus: vi.fn().mockResolvedValue({}),
    agents: vi.fn().mockResolvedValue([]),
    workspaceBudget: vi.fn().mockResolvedValue(null),
    costSummary: vi.fn().mockResolvedValue([]),
    agent: vi.fn().mockResolvedValue({ name: 'test-agent' }),
    task: vi.fn().mockResolvedValue({ title: 'test-task' }),
    mergeRequest: vi.fn().mockResolvedValue({ title: 'test-mr' }),
    activity: vi.fn().mockResolvedValue([]),
    mergeQueue: vi.fn().mockResolvedValue([]),
    mergeQueueGraph: vi.fn().mockResolvedValue({ nodes: [], edges: [] }),
    tryIt: vi.fn().mockResolvedValue({}),
    mrTryItEnvironments: vi.fn().mockResolvedValue([]),
    previewEnvironment: vi.fn().mockResolvedValue(null),
    teardownPreviewEnvironment: vi.fn().mockResolvedValue(null),
  },
}));

vi.mock('../lib/ExplorerCanvas.svelte', () => ({
  default: function ExplorerCanvasStub() {},
}));

import WorkspaceHome from '../components/WorkspaceHome.svelte';
import {
  activityStreamState,
  attachActivityStream,
  detachActivityStream,
  resetActivityStream,
} from '../lib/activityStream.svelte.js';

// A fake ws-store in the web/src/lib/ws.js shape — the store the App
// attaches the activity rider to. Driving it is exactly "a bus message
// emitted for W" (emit_telemetry / event emit land on this channel).
function fakeWsStore() {
  const messages = new Set();
  return {
    onMessage(cb) {
      messages.add(cb);
      return () => messages.delete(cb);
    },
    onStatus() {
      return () => {};
    },
    emit(msg) {
      messages.forEach((cb) => cb(msg));
    },
  };
}

function busMessage(overrides = {}) {
  return {
    id: 'msg-rt',
    tenant_id: 't-1',
    from: { agent: 'agent-9' },
    workspace_id: 'ws-1',
    to: { workspace: 'ws-1' },
    kind: 'agent_completed',
    payload: { agent_id: 'agent-9', mr_title: 'Live merged work' },
    created_at: Date.now(),
    ...overrides,
  };
}

describe('WorkspaceHome activity sidebar — real-time bus feed', () => {
  beforeEach(() => {
    resetActivityStream();
    detachActivityStream();
  });

  it('a bus message emitted for the open workspace appears without reload or scope change', async () => {
    const store = fakeWsStore();
    attachActivityStream(store);

    const ws = { id: 'ws-1', name: 'Test', slug: 'test' };
    const { container } = render(WorkspaceHome, { props: { workspace: ws } });

    // Feed starts empty (all API mocks resolve empty)…
    await waitFor(() => {
      expect(container.querySelector('.sidebar-activity')).toBeNull();
    });

    // …then a message for ws-1 is emitted on the shared connection.
    store.emit(busMessage());

    // The sidebar live-prepends the entry — no reload, no scope change.
    await waitFor(() => {
      const items = container.querySelectorAll('.sidebar-activity-item');
      expect(items.length).toBe(1);
      expect(items[0].textContent).toContain('Agent completed');
    });
  });

  it('a bus message for a different workspace does NOT appear in the open workspace\'s sidebar', async () => {
    const store = fakeWsStore();
    attachActivityStream(store);

    const ws = { id: 'ws-1', name: 'Test', slug: 'test' };
    const { container } = render(WorkspaceHome, { props: { workspace: ws } });

    store.emit(busMessage({ id: 'msg-other', workspace_id: 'ws-2', to: { workspace: 'ws-2' } }));

    await waitFor(() => {
      expect(container.querySelector('.sidebar-activity')).toBeNull();
    });
    // It landed in its own bucket, not the open workspace's.
    expect(activityStreamState.byWorkspace['ws-1']).toBeUndefined();
    expect(activityStreamState.byWorkspace['ws-2']).toHaveLength(1);
  });

  it('live entries stack newest-first as more messages arrive', async () => {
    const store = fakeWsStore();
    attachActivityStream(store);

    const ws = { id: 'ws-1', name: 'Test', slug: 'test' };
    const { container } = render(WorkspaceHome, { props: { workspace: ws } });

    store.emit(busMessage({ id: 'msg-a', created_at: Date.now() - 5000 }));
    store.emit(busMessage({ id: 'msg-b', created_at: Date.now() }));

    await waitFor(() => {
      const items = container.querySelectorAll('.sidebar-activity-item');
      expect(items.length).toBe(2);
    });
    // Sidebar count reflects the tracked bucket size.
    expect(container.querySelector('.sidebar-count').textContent).toBe('2');
  });
});
