// Real-time activity feed state (activity-dashboard.md §"Real-time activity
// feed" — the sidebars must be live, not one-shot snapshots).
//
// Rides the app's ONE shared /ws connection (the same store
// notifications.svelte.js and liveness.svelte.js ride): the server
// broadcasts Telemetry/Event-tier bus Messages for every subscribed
// workspace, and ws.rs delivers them only to subscribed same-tenant members
// (per-workspace membership enforcement is server-side, existing behavior).
// This module maps each delivered bus Message into the SAME entry shape
// GET /api/v1/activity returns, live-prepends it under its workspace, and
// dedupes by message id. On reconnect it re-syncs via
// GET /api/v1/activity?since=<newest entry timestamp> so entries emitted
// while the socket was down are not lost.

import { api } from './api.js';

/** Cap per workspace — the sidebar shows the newest slice anyway. */
const MAX_ENTRIES_PER_WORKSPACE = 100;

/**
 * Reactive state. `byWorkspace` maps workspaceId → entries (newest first).
 * `streamStatus` mirrors the shared /ws connection status.
 */
export const activityStreamState = $state({
  byWorkspace: {},
  streamStatus: 'closed',
});

/** Test/teardown helper — clears all tracked entries. */
export function resetActivityStream() {
  for (const k of Object.keys(activityStreamState.byWorkspace)) {
    delete activityStreamState.byWorkspace[k];
  }
  activityStreamState.streamStatus = 'closed';
}

/** Stable dedupe key for an entry: bus message id when present, else the
 * (event_type, entity, timestamp) tuple — the same fallback key synthesized
 * for fetched entries, so a live-delivered message and its durable
 * notification merge instead of duplicating. */
function entryKey(entry) {
  if (entry.id) return `id:${entry.id}`;
  const ts = Number(entry.timestamp ?? entry.created_at ?? 0);
  return `t:${entry.event_type ?? ''}:${entry.entity_id ?? ''}:${ts}`;
}

/** Insert entries into a workspace bucket, newest-first, deduped. */
function mergeIntoWorkspace(workspaceId, entries) {
  const bucket = activityStreamState.byWorkspace[workspaceId] ?? [];
  const byKey = new Map(bucket.map((e) => [entryKey(e), e]));
  let changed = false;
  for (const entry of entries) {
    if (!entry) continue;
    const key = entryKey(entry);
    if (!byKey.has(key)) {
      byKey.set(key, entry);
      changed = true;
    }
  }
  if (!changed) return activityStreamState.byWorkspace[workspaceId] ?? [];
  const merged = Array.from(byKey.values()).sort((a, b) => {
    const ta = Number(a.timestamp ?? a.created_at ?? 0);
    const tb = Number(b.timestamp ?? b.created_at ?? 0);
    return tb - ta;
  });
  const trimmed = merged.slice(0, MAX_ENTRIES_PER_WORKSPACE);
  activityStreamState.byWorkspace[workspaceId] = trimmed;
  return trimmed;
}

/**
 * Map a bus Message (the ws.rs wire shape of
 * gyre_common::message::Message) to the /api/v1/activity entry shape.
 * Returns null for anything that is not a workspace-scoped bus message
 * (NotificationPush, presence, pongs all lack `kind`).
 */
export function busMessageToActivityEntry(msg) {
  if (!msg || typeof msg !== 'object') return null;
  // Bus Messages carry `kind`; control frames carry `type`.
  if (typeof msg.kind !== 'string' || !msg.kind) return null;
  const workspaceId = msg.workspace_id ?? null;
  // A message with no workspace scope is not sidebar activity for any
  // workspace bucket.
  if (!workspaceId) return null;
  const payload = msg.payload && typeof msg.payload === 'object' ? msg.payload : {};
  const agentId =
    payload.agent_id ??
    (msg.from && typeof msg.from === 'object' && typeof msg.from.agent === 'string'
      ? msg.from.agent
      : null);
  // Bus timestamps are epoch MILLISECONDS; /activity entries are seconds.
  const createdMs = Number(msg.created_at);
  const timestamp = Number.isFinite(createdMs) && createdMs > 0 ? Math.floor(createdMs / 1000) : Math.floor(Date.now() / 1000);

  const entityType = payload.mr_id
    ? 'mr'
    : agentId
      ? 'agent'
      : payload.task_id
        ? 'task'
        : payload.spec_path
          ? 'spec'
          : null;
  const entityId =
    payload.mr_id ?? agentId ?? payload.task_id ?? payload.spec_path ?? null;

  return {
    // Bus message id — the live dedupe key.
    id: msg.id ?? null,
    event_type: msg.kind,
    title: payload.title ?? payload.description ?? payload.tool_name ?? msg.kind,
    description: payload.description ?? payload.tool_name ?? null,
    entity_type: entityType,
    entity_id: entityId,
    entity_name: payload.mr_title ?? payload.agent_name ?? null,
    timestamp,
    agent_id: agentId,
    mr_id: payload.mr_id ?? null,
    task_id: payload.task_id ?? null,
    spec_path: payload.spec_path ?? null,
    repo_id: payload.repo_id ?? null,
    gate_name: payload.gate_name ?? payload.gate ?? null,
    commit_sha: payload.commit_sha ?? payload.merge_commit_sha ?? payload.sha ?? null,
    branch: payload.branch ?? payload.source_branch ?? null,
    workspace_id: workspaceId,
    // Live marker: the entry arrived over the push channel, not a fetch.
    live: true,
  };
}

/**
 * Merge a fetched /api/v1/activity page into the stream (the initial load
 * and post-scope-change reload both route through here so fetched and live
 * entries share one deduped, newest-first bucket). Returns the merged
 * bucket for the workspace.
 */
export function mergeFetchedActivity(workspaceId, entries) {
  if (!workspaceId) return [];
  const list = Array.isArray(entries) ? entries : [];
  return mergeIntoWorkspace(workspaceId, list.map((e) => ({ ...e, live: false })));
}

/** Newest entry timestamp across the tracked buckets (epoch seconds). */
function newestTimestamp() {
  let newest = 0;
  for (const bucket of Object.values(activityStreamState.byWorkspace)) {
    for (const e of bucket) {
      const ts = Number(e.timestamp ?? e.created_at ?? 0);
      if (ts > newest) newest = ts;
    }
  }
  return newest;
}

/** Re-sync after a reconnect: fetch everything newer than the newest entry
 * we hold and merge it. Best-effort — a failed fetch leaves the tracked
 * entries untouched (the next reconnect retries). */
async function resyncAfterReconnect() {
  try {
    const since = newestTimestamp();
    const entries = await api.activity(100, since > 0 ? since : null);
    const list = Array.isArray(entries) ? entries : [];
    // The unified feed spans workspaces; bucket each entry by its own
    // workspace_id so every open sidebar re-syncs from one request.
    const byWs = new Map();
    for (const e of list) {
      const wsId = e?.workspace_id;
      if (!wsId) continue;
      if (!byWs.has(wsId)) byWs.set(wsId, []);
      byWs.get(wsId).push({ ...e, live: false });
    }
    for (const [wsId, wsEntries] of byWs) mergeIntoWorkspace(wsId, wsEntries);
  } catch {
    // Offline/failed re-sync — entries stay as tracked.
  }
}

/**
 * Wire the activity stream to the shared WS store. Idempotent; returns a
 * detach function. Bus Messages for ANY subscribed workspace are tracked
 * (the server only delivers workspaces the connection subscribed to and is
 * a member of); components read the buckets they render.
 */
export function attachActivityStream(store) {
  if (detachStream) return detachStream;

  const offMessage = store.onMessage((msg) => {
    const entry = busMessageToActivityEntry(msg);
    if (!entry) return;
    mergeIntoWorkspace(entry.workspace_id, [entry]);
  });

  let wasConnected = false;
  const offStatus = store.onStatus((s) => {
    activityStreamState.streamStatus = s === 'connected' ? 'open' : 'closed';
    // Reconnect (connected after a prior connected session): re-sync the
    // missed window. The initial connection's data comes from the
    // components' own first load.
    if (s === 'connected') {
      if (wasConnected) void resyncAfterReconnect();
      wasConnected = true;
    }
  });

  detachStream = () => {
    offMessage();
    offStatus();
    detachStream = null;
    activityStreamState.streamStatus = 'closed';
  };
  return detachStream;
}

let detachStream = null;

/** Detach the real-time subscription (App teardown). */
export function detachActivityStream() {
  if (detachStream) detachStream();
}
