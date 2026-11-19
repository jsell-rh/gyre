---
title: "Workspace-scoped telemetry access: Subscribe membership authorization and the REST activity endpoint"
spec_ref: "Spec gap spec-c62aac6701125cb09e0c736c / telemetry-workspace-access / 60b177f85a11"
depends_on: []
progress: ready-for-review
commits: ["9fa691150452e3df58ea95f3731cf150c96d84b4", "d2fe75e4c1098a26adb8717c02feed86e2afbc4a", "81910e025d8602ee0f20612620f1fdcb14a4349d"]
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

Both capabilities are implemented in production code and previously reviewed
and approved; this round (rebase repair, merge commit `5e4a506e`) resolved
the rebase against current main head `ee03c299` (task-274) and re-verified
the merged tree. The task implementation is unchanged this round — the merge
touched no task-252 file.

### Subscribe membership authorization (gap 2b439cf656cf14c344afdb07)

The ws.rs Subscribe handler authorizes each requested workspace in order:
the preserved Admin bypass, the preserved tenant-containment check, then real
workspace-membership verification. User connections resolve a membership
record through `state.workspace_memberships.find_by_user_and_workspace`;
agent connections (user_id None) are scoped to their own agent record's
workspace. A rejected scope produces the SubscribeError frame, creates no
subscription, and delivers neither workspace-scoped bus messages, telemetry,
nor replay.

### REST telemetry endpoint (gap 64d1be477f27ebf27ec73076)

`GET /api/v1/workspaces/:workspace_id/activity` (`workspace_activity_handler`,
crates/gyre-server/src/api/activity.rs) is backed by
`state.telemetry_buffer`: `since` in epoch milliseconds (entries with
`created_at <= since` excluded, 1-ms-newer included), `limit` honored
(default 100), per-handler membership enforcement (403 for non-members), and
the spec's exact ActivityEventData field mapping via
`MessageKind::ag_event_type`. The route is registered in the ABAC
RouteResourceMapping (abac_middleware.rs, "activity" resource, workspace
path param) — check-abac-route-registry.sh passes with no new exemptions
(re-run on the merged tree this round, exit 0). The unscoped
`/api/v1/activity` and WS ActivityQuery paths are unchanged.

### Rebase repair (this round, merge commit 5e4a506e)

Single conflict, in `scripts/test_dev_cargo_clean.py`: both sides carried
independent fixes for the same cargo 1.100 build-unit-layout CI failure —
ours `20fb8239` (task-252) and main's `b58fa463` (task-259, verified on
cargo 1.99.0 in both layouts and landed on main). Resolved to main's
version; our parallel fix is superseded. All other files auto-merged; the
only server-code change of the merge is `api/repos.rs`, which came
verbatim from the approved base (our branch never modified it relative to
the merge base), so ws.rs, api/activity.rs, api/mod.rs, and
abac_middleware.rs are byte-identical to the previously approved state.

### Test evidence

- This sandbox, merged tree (5e4a506e): `scripts/check-abac-route-registry.sh`
  exit 0 (no new exemptions); workspace `cargo fmt --check` clean;
  `python3 -m unittest test_dev_cargo_clean` green (2 tests, cargo 1.99.0
  legacy layout). Evidence: /tmp/stage/review-evidence/task-252-rebase-repair-4.md.
- This sandbox has no outbound network (index.crates.io unreachable, curl
  exit 7) and no cargo registry cache, so server build/test compilation
  cannot run here — deferred to the independent verifier per the assignment
  budget rule. Focused verifier list: ws_subscribe_rejects_same_tenant_non_member
  (ws.rs), ws_subscribe_agent_scoped_to_own_workspace_record (ws.rs), the
  four workspace_activity_* tests (api/activity.rs), and gyre-common's
  kind_ag_event_type_mapping_matches_spec, plus `cargo check -p gyre-server
  --all-targets` on the merged tree.
- Deferred for remote verification: full `npm test`, all-target clippy,
  full workspace suite, and exact-head GitHub checks.
