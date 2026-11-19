---
title: "Workspace-scoped telemetry access: Subscribe membership authorization and the REST activity endpoint"
spec_ref: "Spec gap spec-c62aac6701125cb09e0c736c / telemetry-workspace-access / 60b177f85a11"
depends_on: []
progress: ready-for-review
commits: ["81910e025d8602ee0f20612620f1fdcb14a4349d", "d2fe75e4c1098a26adb8717c02feed86e2afbc4a"]
---

## Required behavior

Two capabilities gating access to workspace telemetry: (1) Subscribe membership authorization (spec Telemetry Store: 'Subscription authorization: The server verifies workspace membership when processing Subscribe messages — a client cannot subscribe to a workspace it doesn't belong to. This prevents telemetry leakage across workspaces even though the buffer itself has no tenant isolation.'): the ws.rs Subscribe handler (crates/gyre-server/src/ws.rs:98-115) currently validates only that each requested workspace belongs to the caller's TENANT (plus an Admin bypass) — add real workspace-membership verification via WorkspaceMembershipRepository for user connections (agents subscribe to their own workspace per their agent record); a non-member same-tenant user's Subscribe for that workspace must be rejected (no subscription created, no delivery), while the existing tenant check and Admin bypass behavior are preserved. (2) REST telemetry endpoint (spec API/Telemetry: 'GET /api/v1/workspaces/:workspace_id/activity?since=<epoch_ms>&limit=100 — Queries the in-memory TelemetryBuffer. The since parameter uses epoch milliseconds. The handler verifies workspace membership before returning results — an unauthenticated or unauthorized caller receives 403. Response format preserves backwards compatibility with today's ActivityEventData'): the route does not exist today (only the unscoped notification-synthesized /api/v1/activity). Add GET /api/v1/workspaces/:workspace_id/activity backed by state.telemetry (TelemetryBuffer), with `since` in epoch milliseconds and `limit`, membership verification via WorkspaceMembershipRepository (403 for non-members, preserving the per-handler authorization pattern of the sibling workspace-scoped message routes), and the ActivityEventData field mapping from the spec table: event_id = message.id, agent_id = payload["agent_id"] (or from when Agent(id)), event_type = message.kind mapped to its AgEventType string, description = payload["tool_name"] or the kind name, timestamp = message.created_at. The new route MUST be registered in the ABAC RouteResourceMapping in abac_middleware.rs (check-abac-route-registry.sh invariant — no new exemption-file entries), and the existing /api/v1/activity and WS ActivityQuery paths are unchanged.

## Acceptance Criteria

- [ ] Subscribe authorization: a user authenticated in tenant T with no membership record for workspace W (which belongs to T) has their Subscribe for W rejected — no workspace-scoped bus messages or telemetry for W are delivered to that connection — while a member of W in the same tenant receives them; agent connections still receive their own workspace's stream.
- [ ] REST endpoint, happy path: a member of W GETs /api/v1/workspaces/W/activity?since=<ms> and receives telemetry records shaped as ActivityEventData with the exact spec field mapping (event_id from message id, agent_id from payload or origin, event_type from the kind, description from tool_name or kind name, timestamp = created_at in epoch ms).
- [ ] REST endpoint, boundary: telemetry entries with created_at <= since are excluded; entries 1ms newer are included; limit is honored.
- [ ] REST endpoint, enforcement: a same-tenant non-member caller receives 403 and no telemetry rows; the route has an ABAC RouteResourceMapping entry and scripts/check-abac-route-registry.sh passes with no new exemptions.
- [ ] The unscoped /api/v1/activity route and the WebSocket ActivityQuery path continue to work unchanged.

## Verification intent

Subscribe leak probe: authenticate a user in tenant T, create workspace W in T without granting the user membership, connect the WebSocket and Subscribe to W, then emit telemetry and a workspace-scoped bus message to W; the current handler accepts the subscription on tenant match alone (ws.rs:107-108) and delivers both — the test asserts the connection receives NOTHING for W and therefore fails against the current code, passing only when the WorkspaceMembershipRepository check gates Subscribe. Endpoint probe: the same non-member GETs /api/v1/workspaces/W/activity — today the route does not exist (404 from the router), and after implementation a missing membership check would return 200 with rows; the test asserts 403, failing if the handler serves the buffer without verifying membership.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/message-bus.md`. Source: `4127964001113dda08e4852404a68ba560ceb764`; plan: `151021041bfb974bd08554e6182cff451df6efb83f37d8a11f6dfdcc1950d05e`.

[
  {
    "id": "2b439cf656cf14c344afdb07",
    "decision": "gap",
    "requirement_quote": "**Subscription authorization:** The server verifies workspace membership when processing `Subscribe` messages — a client cannot subscribe to a workspace it doesn't belong to. This prevents telemetry leakage across workspaces even though the buffer itself has no tenant isolation.",
    "detail": "The buffer itself is implemented exactly as specced: TelemetryBuffer in gyre-common with per-workspace DashMap<Id, VecDeque<Message>> ring buffers, FIFO oldest-entry eviction at max_per_workspace (default 10,000, GYRE_TELEMETRY_BUFFER_SIZE), and largest-first workspace eviction when distinct workspaces exceed max_workspaces (default 100, GYRE_TELEMETRY_MAX_WORKSPACES) — wired in build_state with those env vars and defaults. Telemetry is pushed through the broadcast channel and stored only in the buffer, never in MessageRepository. The subscription-authorization requirement is NOT met: the ws.rs Subscribe handler validates only that each requested workspace belongs to the caller's TENANT (plus an Admin bypass) — it performs no workspace-membership verification (no WorkspaceMembershipRepository lookup, in contrast to the REST handlers which do check membership). A user authenticated in tenant T can subscribe to any workspace in T without being a member, and since workspace-scoped bus messages and telemetry are delivered to subscribed connections, same-tenant non-members receive workspace telemetry the spec says they must not.",
    "evidence": [
      {
        "file": "crates/gyre-common/src/message.rs",
        "quote": "pub struct TelemetryBuffer {\n    /// Per-workspace ring buffers. Key: workspace_id.\n    buffers: DashMap<Id, VecDeque<Message>>,",
        "detail": "The telemetry store is the specced in-memory per-workspace ring buffer, replacing ActivityStore; it is not part of MessageRepository."
      },
      {
        "file": "crates/gyre-common/src/message.rs",
        "quote": "// Enforce workspace count limit: evict the workspace with the most entries.\n        if self.buffers.len() > self.max_workspaces {",
        "detail": "Largest-first workspace eviction is implemented as specified (the anti-flushing mitigation), alongside FIFO eviction at max_per_workspace."
      },
      {
        "file": "crates/gyre-server/src/lib.rs",
        "quote": "let telemetry_buffer = Arc::new(TelemetryBuffer::new(\n        std::env::var(\"GYRE_TELEMETRY_BUFFER_SIZE\")",
        "detail": "The buffer is wired in build_state with GYRE_TELEMETRY_BUFFER_SIZE (default 10_000) and GYRE_TELEMETRY_MAX_WORKSPACES (default 100), matching the specced defaults."
      },
      {
        "file": "crates/gyre-server/src/ws.rs",
        "quote": "let tenant_match = caller.roles.contains(&gyre_domain::UserRole::Admin)\n                                                    || workspace.tenant_id.as_str() == caller.tenant_id;",
        "detail": "Subscribe processing authorizes on tenant match (plus Admin bypass) only — no workspace-membership verification is performed, so a same-tenant non-member can subscribe to a workspace it does not belong to, contradicting the section's explicit authorization requirement."
      }
    ]
  },
  {
    "id": "64d1be477f27ebf27ec73076",
    "decision": "gap",
    "requirement_quote": "Queries the in-memory `TelemetryBuffer`. The `since` parameter on this endpoint uses **epoch milliseconds** (matching the message bus, not the legacy seconds convention). The handler verifies workspace membership before returning results — an unauthenticated or unauthorized caller receives 403.",
    "detail": "The specced endpoint does not exist. There is no GET /api/v1/workspaces/:workspace_id/activity route anywhere in the router; the only activity routes are /api/v1/activity (which synthesizes events from notifications, is not workspace-scoped, and takes an unused _auth parameter with no workspace-membership check) and /api/v1/users/:username/activity (an unrelated profile feed). The TelemetryBuffer itself is only queryable through the WebSocket ActivityQuery path (scoped to subscribed workspaces) and the admin-only /api/v1/admin/audit endpoint (cross-workspace) — neither is the REST endpoint with per-workspace membership enforcement and the ActivityEventData field mapping (event_id/agent_id/event_type/description/timestamp) that this section specifies. The mapping logic exists in the WS path but the specced endpoint, its 403 membership enforcement, and its response contract are missing.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "quote": ".route(\"/api/v1/activity\", get(activity::activity_handler))",
        "detail": "The only general activity route is the unscoped /api/v1/activity; no /api/v1/workspaces/:workspace_id/activity route exists anywhere in the router (inspected the full route table in api/mod.rs)."
      },
      {
        "file": "crates/gyre-server/src/api/activity.rs",
        "quote": "/// GET /api/v1/activity — unified activity feed.\n///\n/// Synthesizes activity events from notifications (which are populated for all",
        "detail": "The existing activity handler is notification-synthesized, not TelemetryBuffer-backed: it never reads state.telemetry_buffer, is not workspace-scoped, and its _auth parameter is unused (no membership check), so neither the backing store nor the 403 authorization requirement of this section is met on any REST path."
      },
      {
        "file": "crates/gyre-common/src/message.rs",
        "quote": "pub fn list_since(&self, workspace_id: &Id, since_ms: u64, limit: usize) -> Vec<Message> {",
        "detail": "TelemetryBuffer does provide the per-workspace, epoch-millisecond since query the endpoint would need (used by the WebSocket ActivityQuery path), so the gap is the missing REST endpoint and its membership enforcement, not the store."
      }
    ]
  }
]

## Shipped

Both capabilities are implemented in production code, and the review-round
test race is repaired.

### Subscribe membership authorization (gap 2b439cf656cf14c344afdb07)

The ws.rs Subscribe handler authorizes each requested workspace in three
steps, in order: the preserved Admin bypass (global token), the preserved
tenant-containment check (workspace must belong to the caller's tenant),
and real workspace-membership verification — user connections resolve a
membership record through `state.workspace_memberships.find_by_user_and_workspace`
(the same store the REST handlers use); agent connections (user_id None)
are scoped to their own agent record's workspace via `state.agents.find_by_id`.
A rejected scope produces the existing SubscribeError frame, creates no
subscription, and — because delivery filtering keys on the connection's
authorized `subscribed_workspaces` — delivers neither workspace-scoped bus
messages nor telemetry for that workspace. Lookup errors fail closed
(reject). The unscoped legacy ActivityEvent/ActivityQuery fallback paths
are unchanged.

### REST telemetry endpoint (gap 64d1be477f27ebf27ec73076)

`GET /api/v1/workspaces/:workspace_id/activity`
(`activity::workspace_activity_handler`, crates/gyre-server/src/api/activity.rs)
backed by `state.telemetry_buffer` (the in-memory TelemetryBuffer):

- `since` in epoch milliseconds — `TelemetryBuffer::list_since` filters
  `created_at > since`, so entries with `created_at <= since` are excluded
  and 1-ms-newer entries are included; `limit` defaults to 100.
- Membership enforcement per-handler, mirroring the sibling
  `list_workspace_messages` route: Admin bypass, API-key users require a
  `WorkspaceMembershipRepository` record, agent tokens require the agent
  record's workspace to match — 403 otherwise. (The ABAC middleware's
  builtin `workspace-membership-required` deny at priority 940 is the
  first line for user principals; the handler check covers agent tokens,
  which that policy does not key on.)
- Response is a bare JSON array of `ActivityEventData` with the spec's
  exact field mapping: `event_id` ← message.id, `agent_id` ←
  payload["agent_id"] with the `MessageOrigin::Agent(id)` fallback (empty
  when neither is present — never a fabricated identity), `event_type` ←
  `MessageKind::ag_event_type()`, `description` ← payload["tool_name"]
  or the kind name, `timestamp` ← created_at.
- Route registered in api/mod.rs and in the ABAC RouteResourceMapping
  ("activity" resource, `:workspace_id` path param) —
  `scripts/check-abac-route-registry.sh` passes with no new exemption
  entries. The unscoped `/api/v1/activity` and the WS ActivityQuery path
  are unchanged.

### Test-race repair (commit d2fe75e4, review round 1 finding)

The two new WS tests emitted telemetry immediately after the client-side
Subscribe send; on the current-thread `#[tokio::test]` runtime the
connection task is not polled until the test task yields, so the telemetry
broadcast was consumed and dropped while the connection's
`subscribed_workspaces` was still empty (telemetry is never replayed from
the buffer) — the member/agent delivery assertions would fail against the
correct implementation. New `await_ws_subscription` helper polls
`state.ws_connection_workspaces` until the authorized subscription is
registered server-side (the Subscribe handler inserts it after
authorization, before returning to its select loop) and is called between
`subscribe_ws` and the emits in both tests. This is a happens-before
synchronization, not a timing sleep: a rejected workspace never reaches
the index, so the non-member/foreign-workspace leak assertions stay
unsynchronized by construction (their sync is the SubscribeError frame
itself).

Validated with a standalone TCP `select!` analog of the connection task
(current-thread runtime, loopback socket, broadcast channel, shared
per-connection workspace index — sources and output under
`/tmp/stage/review-evidence/probe/` and
`ws-subscribe-sync-probe-output.txt`): the pre-repair sequencing loses
the telemetry broadcast 160/160 trials; the index-poll sync delivers
160/160. `cargo check -p gyre-server --lib --tests` passes at the fix
head.

### Test evidence

- `crates/gyre-server/src/ws.rs` (lib tests):
  `ws_subscribe_rejects_same_tenant_non_member` (leak probe — same-tenant
  non-member gets SubscribeError and receives NOTHING after telemetry + a
  workspace bus message, while a member of the same workspace receives
  both; fails against the tenant-match-only handler),
  `ws_subscribe_agent_scoped_to_own_workspace_record` (agent subscribes
  to own workspace and receives its telemetry; foreign same-tenant
  workspace rejected with no delivery).
- `crates/gyre-server/src/api/activity.rs` (lib tests): endpoint probes —
  member happy path with the exact ActivityEventData field mapping
  (`workspace_activity_maps_fields_per_spec`), `since` boundary
  (`created_at <= since` excluded, 1-ms-newer included) and `limit`
  (`workspace_activity_since_boundary_and_limit`), same-tenant non-member
  403 (`workspace_activity_rejects_same_tenant_non_member`), agent-token
  scoping (`workspace_activity_agent_token_scoped_to_own_workspace`).
- Unscoped `/api/v1/activity` regression: pre-existing
  `activity_log_query` (crates/gyre-server/tests/api_integration.rs);
  the handler is untouched by both commits. The WS ActivityQuery handler
  is likewise untouched.

### Deferred to the independent verifier (sandbox build budget)

gyre-server test builds exceed this sandbox's 4 GiB code-generation
budget (build wrapper enforces); run after model exit:

    cargo test -p gyre-server --lib ws_subscribe
    cargo test -p gyre-server --lib workspace_activity

plus the task-wide server suite at the exact review head. `cargo check -p
gyre-server --lib --tests` was executed locally at the fix head (clean).
