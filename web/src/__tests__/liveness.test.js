import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// Module under test uses Svelte 5 runes ($state) — must be imported as a
// .svelte.js module so the compiler transforms the runes.
import {
  activeRuns,
  resetActiveRuns,
  createLivenessTracker,
  specRefFilename,
  nodesForSpecRef,
  activeAgentNodeIds,
  computeRecentChanges,
  recentChangeAlpha,
  computeNodeGateStatus,
  edgeGateStatus,
  RECENT_CHANGE_WINDOW_MS,
} from '../lib/liveness.svelte.js';

// ── Fixtures ──────────────────────────────────────────────────────────────

const NODES = [
  { id: 'n1', name: 'create_user', qualified_name: 'api.create_user', spec_path: 'specs/system/auth.md' },
  { id: 'n2', name: 'get_user', qualified_name: 'api.get_user', spec_paths: ['specs/system/auth.md', 'specs/system/logging.md'] },
  { id: 'n3', name: 'bill', qualified_name: 'api.bill' }, // governed via edge below
  { id: 'spec-auth', name: 'auth.md', qualified_name: 'spec:auth', file_path: 'specs/system/auth.md' },
  { id: 'n4', name: 'unrelated', qualified_name: 'api.unrelated', spec_path: 'specs/system/other.md' },
];

const EDGES = [
  { id: 'e1', source_id: 'n3', target_id: 'spec-auth', edge_type: 'governed_by' },
  { id: 'e2', source_id: 'n1', target_id: 'n4', edge_type: 'calls' },
];

function busMessage(kind, payload) {
  return { id: 'm1', kind, payload, to: { workspace: 'ws1' }, created_at: 1 };
}

/** Mock ws store capturing the message listener. */
function mockWsStore() {
  let listener = null;
  return {
    onMessage(cb) {
      listener = cb;
      return () => { listener = null; };
    },
    dispatch(msg) { if (listener) listener(msg); },
    get listening() { return listener !== null; },
  };
}

beforeEach(() => {
  resetActiveRuns();
});

afterEach(() => {
  resetActiveRuns();
});

// ── createLivenessTracker ─────────────────────────────────────────────────

describe('createLivenessTracker', () => {
  it('records a run on run_started and resolves its spec via the task API', async () => {
    const store = mockWsStore();
    const fetchTask = vi.fn(async (id) => ({ id, spec_path: 'specs/system/auth.md' }));
    createLivenessTracker(store, { fetchTask });

    store.dispatch(busMessage('run_started', { agent_id: 'agent-1', task_id: 'task-9' }));
    expect(activeRuns['agent-1']).toMatchObject({ agentId: 'agent-1', taskId: 'task-9' });
    expect(activeRuns['agent-1'].specRef).toBeNull(); // not yet resolved

    await vi.waitFor(() => expect(activeRuns['agent-1'].specRef).toBe('specs/system/auth.md'));
    expect(fetchTask).toHaveBeenCalledWith('task-9');
  });

  it('removes the run on run_finished', () => {
    const store = mockWsStore();
    createLivenessTracker(store, { fetchTask: async () => null });

    store.dispatch(busMessage('run_started', { agent_id: 'agent-1' }));
    expect(activeRuns['agent-1']).toBeTruthy();
    store.dispatch(busMessage('run_finished', { agent_id: 'agent-1' }));
    expect(activeRuns['agent-1']).toBeUndefined();
  });

  it('does not apply a resolved spec when the run finished while the fetch was in flight', async () => {
    const store = mockWsStore();
    let resolveFetch;
    const fetchTask = () => new Promise((res) => { resolveFetch = res; });
    createLivenessTracker(store, { fetchTask });

    store.dispatch(busMessage('run_started', { agent_id: 'agent-1', task_id: 'task-9' }));
    store.dispatch(busMessage('run_finished', { agent_id: 'agent-1' }));
    resolveFetch({ spec_ref: 'auth.md §2' });
    await new Promise((r) => setTimeout(r, 0));
    expect(activeRuns['agent-1']).toBeUndefined();
  });

  it('ignores messages without an agent_id payload and non-run kinds', () => {
    const store = mockWsStore();
    createLivenessTracker(store, { fetchTask: async () => ({ spec_ref: 'x.md' }) });

    store.dispatch(busMessage('run_started', { task_id: 't' })); // no agent_id
    store.dispatch(busMessage('state_changed', { agent_id: 'a', new_state: 'x' }));
    store.dispatch({ type: 'AuthResult', success: true });
    store.dispatch(null);
    expect(Object.keys(activeRuns)).toEqual([]);
  });

  it('stops listening on destroy', () => {
    const store = mockWsStore();
    const tracker = createLivenessTracker(store, { fetchTask: async () => null });
    tracker.destroy();
    expect(store.listening).toBe(false);
    store.dispatch(busMessage('run_started', { agent_id: 'agent-1' }));
    expect(activeRuns['agent-1']).toBeUndefined();
  });

  it('returns a no-op for a missing ws store', () => {
    const tracker = createLivenessTracker(null);
    expect(typeof tracker.destroy).toBe('function');
    tracker.destroy();
  });
});

// ── specRefFilename / nodesForSpecRef ─────────────────────────────────────

describe('specRefFilename', () => {
  it('extracts the .md file name from a section-qualified spec_ref', () => {
    expect(specRefFilename('human-system-interface.md §3 Design Principle')).toBe('human-system-interface.md');
    expect(specRefFilename('specs/system/auth.md §2')).toBe('auth.md');
    expect(specRefFilename('AUTH.MD §1')).toBe('auth.md'); // case-insensitive
  });
  it('returns null for refs without a spec file', () => {
    expect(specRefFilename('')).toBeNull();
    expect(specRefFilename(null)).toBeNull();
    expect(specRefFilename('no file here')).toBeNull();
  });
});

describe('nodesForSpecRef', () => {
  it('matches nodes by spec_path and spec_paths basename', () => {
    const ids = nodesForSpecRef(NODES, EDGES, 'auth.md §2');
    expect(ids.has('n1')).toBe(true);  // spec_path
    expect(ids.has('n2')).toBe(true);  // spec_paths
    expect(ids.has('n4')).toBe(false); // different spec
  });

  it('matches nodes via governed_by edges to a spec node', () => {
    const ids = nodesForSpecRef(NODES, EDGES, 'specs/system/auth.md');
    expect(ids.has('n3')).toBe(true); // governed by the auth spec node
  });

  it('ignores deleted governed_by edges', () => {
    const edges = [{ ...EDGES[0], deleted_at: 123 }];
    const ids = nodesForSpecRef(NODES, edges, 'auth.md');
    expect(ids.has('n3')).toBe(false);
  });

  it('returns an empty set for unresolvable refs', () => {
    expect(nodesForSpecRef(NODES, EDGES, 'missing.md').size).toBe(0);
    expect(nodesForSpecRef(NODES, EDGES, null).size).toBe(0);
  });
});

describe('activeAgentNodeIds', () => {
  it('unions node sets across active runs and skips unresolved specs', () => {
    activeRuns['a1'] = { agentId: 'a1', taskId: 't1', specRef: 'auth.md §1', startedAt: 1 };
    activeRuns['a2'] = { agentId: 'a2', taskId: 't2', specRef: 'other.md §9', startedAt: 2 };
    activeRuns['a3'] = { agentId: 'a3', taskId: 't3', specRef: null, startedAt: 3 }; // spec not resolved yet
    const ids = activeAgentNodeIds(NODES, EDGES);
    expect(ids.has('n1')).toBe(true);  // auth (a1)
    expect(ids.has('n2')).toBe(true);  // auth (a1)
    expect(ids.has('n3')).toBe(true);  // auth via governed_by (a1)
    expect(ids.has('n4')).toBe(true);  // other.md (a2)
  });
});

// ── computeRecentChanges / recentChangeAlpha ───────────────────────────────

describe('computeRecentChanges', () => {
  const now = Date.now();
  const sec = (msAgo) => Math.floor((now - msAgo) / 1000);
  const node = (qn) => NODES.find((n) => n.qualified_name === qn);

  it('maps nodes_added and nodes_modified within the last hour to node ids', () => {
    const timeline = [
      { timestamp: sec(10 * 60 * 1000), delta_json: JSON.stringify({ nodes_added: [{ qualified_name: 'api.create_user', name: 'create_user', node_type: 'function' }] }) },
      { timestamp: sec(30 * 60 * 1000), delta_json: JSON.stringify({ nodes_modified: [{ qualified_name: 'api.get_user', field_changes: [{ field: 'complexity' }] }] }) },
    ];
    const changes = computeRecentChanges(NODES, timeline, now);
    expect(changes.has('n1')).toBe(true);
    expect(changes.has('n2')).toBe(true);
    expect(changes.has('n4')).toBe(false);
  });

  it('keeps the most recent change time when a node appears in several deltas', () => {
    const timeline = [
      { timestamp: sec(50 * 60 * 1000), delta_json: JSON.stringify({ nodes_added: ['api.create_user'] }) },
      { timestamp: sec(5 * 60 * 1000), delta_json: JSON.stringify({ nodes_modified: [{ qualified_name: 'api.create_user' }] }) },
    ];
    const changes = computeRecentChanges(NODES, timeline, now);
    expect(changes.get('n1').changedAtMs).toBe(Math.floor((now - 5 * 60 * 1000) / 1000) * 1000);
  });

  it('excludes deltas older than one hour and future-dated garbage', () => {
    const timeline = [
      { timestamp: sec(61 * 60 * 1000), delta_json: JSON.stringify({ nodes_added: ['api.create_user'] }) },
      { timestamp: sec(-60 * 1000), delta_json: JSON.stringify({ nodes_added: ['api.get_user'] }) },
      { timestamp: 'not-a-number', delta_json: JSON.stringify({ nodes_added: ['api.bill'] }) },
    ];
    expect(computeRecentChanges(NODES, timeline, now).size).toBe(0);
  });

  it('skips count-only nodes_modified (no agent context) and unparseable delta_json', () => {
    const timeline = [
      { timestamp: sec(5 * 60 * 1000), delta_json: JSON.stringify({ nodes_modified: 3 }) },
      { timestamp: sec(5 * 60 * 1000), delta_json: '{broken json' },
      { timestamp: sec(5 * 60 * 1000) },
    ];
    expect(computeRecentChanges(NODES, timeline, now).size).toBe(0);
  });

  it('ignores removed nodes that no longer exist in the graph', () => {
    const timeline = [
      { timestamp: sec(5 * 60 * 1000), delta_json: JSON.stringify({ nodes_removed: ['api.gone'] }) },
    ];
    expect(computeRecentChanges(NODES, timeline, now).size).toBe(0);
  });
});

describe('recentChangeAlpha', () => {
  it('fades linearly from 1 to 0 over the hour — never an instant off', () => {
    const now = Date.now();
    expect(recentChangeAlpha({ changedAtMs: now }, now)).toBe(1);
    expect(recentChangeAlpha({ changedAtMs: now - 30 * 60 * 1000 }, now)).toBeCloseTo(0.5, 5);
    expect(recentChangeAlpha({ changedAtMs: now - RECENT_CHANGE_WINDOW_MS + 1 }, now)).toBeGreaterThan(0);
    expect(recentChangeAlpha({ changedAtMs: now - RECENT_CHANGE_WINDOW_MS }, now)).toBe(0);
    expect(recentChangeAlpha({ changedAtMs: now - RECENT_CHANGE_WINDOW_MS - 1000 }, now)).toBe(0);
  });
  it('returns 0 for missing entries', () => {
    expect(recentChangeAlpha(null)).toBe(0);
    expect(recentChangeAlpha({})).toBe(0);
  });
});

// ── computeNodeGateStatus / edgeGateStatus ─────────────────────────────────

describe('computeNodeGateStatus', () => {
  it('marks governed nodes passed when all gates passed and failed when any failed', () => {
    const records = [
      { mrId: 'mr1', spec_ref: 'auth.md §2', gates: [{ status: 'passed' }, { status: 'passed' }] },
      { mrId: 'mr2', spec_ref: 'other.md §1', gates: [{ status: 'passed' }, { status: 'failed' }] },
    ];
    const st = computeNodeGateStatus(NODES, EDGES, records);
    expect(st.get('n1')).toBe('passed');
    expect(st.get('n2')).toBe('passed'); // spec_paths includes auth.md
    expect(st.get('n3')).toBe('passed'); // governed_by → auth
    expect(st.get('n4')).toBe('failed'); // other.md
  });

  it('a failed gate on any governing MR wins over passing ones', () => {
    const records = [
      { mrId: 'mr1', spec_ref: 'auth.md §2', gates: [{ status: 'passed' }] },
      { mrId: 'mr2', spec_ref: 'auth.md §5', gates: [{ status: 'failed' }] },
    ];
    const st = computeNodeGateStatus(NODES, EDGES, records);
    expect(st.get('n1')).toBe('failed');
  });

  it('ignores pending/running-only gate sets and records without a spec_ref', () => {
    const records = [
      { mrId: 'mr1', spec_ref: 'auth.md §2', gates: [{ status: 'pending' }, { status: 'running' }] },
      { mrId: 'mr2', spec_ref: null, gates: [{ status: 'passed' }] },
      { mrId: 'mr3', spec_ref: 'auth.md §2', gates: [] },
    ];
    expect(computeNodeGateStatus(NODES, EDGES, records).size).toBe(0);
  });

  it('returns an empty map for empty records', () => {
    expect(computeNodeGateStatus(NODES, EDGES, []).size).toBe(0);
    expect(computeNodeGateStatus(NODES, EDGES, null).size).toBe(0);
  });
});

describe('edgeGateStatus', () => {
  const st = new Map([['a', 'passed'], ['b', 'failed'], ['c', 'passed']]);

  it('green (passed) only when both endpoints passed', () => {
    expect(edgeGateStatus(st, 'a', 'c')).toBe('passed');
  });
  it('red (failed) when at least one endpoint failed', () => {
    expect(edgeGateStatus(st, 'a', 'b')).toBe('failed');
    expect(edgeGateStatus(st, 'b', 'a')).toBe('failed');
    expect(edgeGateStatus(st, 'b', 'unknown')).toBe('failed');
  });
  it('null when neither endpoint has gate data', () => {
    expect(edgeGateStatus(st, 'x', 'y')).toBeNull();
  });
  it('null when only one endpoint has (passing) data', () => {
    expect(edgeGateStatus(st, 'a', 'y')).toBeNull();
  });
});
