# Review — task-252 (Workspace-scoped telemetry access: Subscribe membership authorization and the REST activity endpoint)

Spec: `specs/system/message-bus.md` §Telemetry Store (Subscription authorization) and §API > Telemetry (GET /api/v1/workspaces/:workspace_id/activity), per gap records 2b439cf656cf14c344afdb07 and 64d1be477f27ebf27ec73076.
Candidate under review: `c965147c` (implementation commit `81910e02` + docs commit `c965147c`), base `fb1424f`.
Verdict: **needs revision** (one test defect; production code sound).

## What was verified (no findings)

- **Subscribe membership authorization (production code)** — `crates/gyre-server/src/ws.rs` Subscribe handler now authorizes in order: preserved Admin bypass (global token → `UserRole::Admin`), preserved tenant containment, then real membership: user connections via `state.workspace_memberships.find_by_user_and_workspace` (the same store the REST handlers use), agent connections (`user_id: None`) via `state.agents.find_by_id` scoped to the agent record's workspace. Lookup errors fail closed (rejected). Rejected scopes produce the existing SubscribeError frame, are excluded from `subscribed_workspaces`, and delivery filtering (`Destination::Workspace` requires `subscribed_workspaces.contains`) means no bus messages and no telemetry reach the connection. API-key auth supplies `user_id: Some` + user roles (verified in `auth.rs` `authenticate_token` path 3; test users default to `UserRole::ReadOnly`, so no accidental Admin bypass), agent tokens supply `user_id: None` with tenant `system_principal_tenant()` == "default".
- **REST telemetry endpoint (production code)** — `GET /api/v1/workspaces/:workspace_id/activity` (`activity::workspace_activity_handler`, `crates/gyre-server/src/api/activity.rs`) registered in `api/mod.rs` and in the ABAC `RouteResourceMapping` ("activity" resource, `:workspace_id` param — the resolver derives `resource.workspace_id` from that param name, abac_middleware.rs:982). Handler mirrors the sibling `list_workspace_messages` per-handler authorization exactly (Admin bypass; membership record for users; agent-record workspace for agent tokens; 403 otherwise). `since` is epoch ms via `TelemetryBuffer::list_since` (`created_at > since` — `<= since` excluded, 1-ms-newer included), `limit` defaults to 100. `telemetry_to_activity_event` implements the spec field table exactly: event_id ← id, agent_id ← payload["agent_id"] with `MessageOrigin::Agent(id)` fallback and empty-when-absent, event_type ← new `MessageKind::ag_event_type()` (seven telemetry kinds → AG-UI strings, AgentError → ERROR, others → `Custom(kind.as_str())`), description ← payload["tool_name"] or kind name, timestamp ← created_at. The unscoped `/api/v1/activity` handler and the WS ActivityQuery path are unchanged (diff shows additions only).
- **Checks executed in this sandbox** (evidence in `/tmp/stage/review-evidence/`):
  - `/tmp/stage/bin/cargo check -p gyre-server --lib --tests` → clean (10m09s, cold).
  - `/tmp/stage/bin/cargo test -p gyre-common --lib` → **129 passed, 0 failed** (incl. `kind_ag_event_type_mapping_matches_spec`).
  - `bash scripts/check-abac-route-registry.sh` → OK, no new exemption entries (grep of exemption files finds none for the new route).
  - `bash scripts/check-abac-exempt-handlers.sh` → OK (85 handlers).
  - `bash scripts/check-arch.sh` → OK. `cargo fmt --check -p gyre-server -p gyre-common` → clean.
  - Coverage rows updated only for the relevant rows 9/11/13 in `specs/coverage/system/message-bus.md` with concrete source references.

## Finding F1 (blocking) — new WS tests fail against the correct implementation (missing Subscribe/emit synchronization)

`ws_subscribe_rejects_same_tenant_non_member` (the member-delivery assertions) and `ws_subscribe_agent_scoped_to_own_workspace_record` (the "agent must receive its own workspace's telemetry" expect) subscribe and then immediately emit without any synchronization:

- `subscribe_ws` only awaits the **client-side** `ws.send` — the server has not necessarily processed the Subscribe.
- `state.emit_telemetry` / `state.emit_event` never yield on the current-thread `#[tokio::test]` runtime: every inner await (`workspaces.find_by_id`, `messages.store` — uncontended mem locks) completes inline and `broadcast::Sender::send` is synchronous.
- The connection task's `tokio::select!` is therefore first polled after the test task yields (at the first `next_json_frame` read) with the broadcast branch ready and the socket-read branch — holding the unprocessed Subscribe — not yet marked ready by the reactor: the telemetry broadcast is consumed and dropped while `subscribed_workspaces` is still empty. Telemetry is never replayed (only Event-tier messages are, from `MessageRepository`).

Empirical reproduction (faithful TCP `select!` analog — server task with `socket read | broadcast recv | targeted recv`, client sends "sub" then the test emits, same sequencing):

- Shipped sequencing: **40/40 trials lost the telemetry broadcast** (`/tmp/stage/review-evidence/ws-subscribe-race-probe-output.txt`).
- With a 50 ms sleep between subscribe and emit (the pre-existing `ws_activity_event_emits_to_telemetry` pattern): 40/40 delivered.

The implementer's sandbox could not build the gyre-server test binary (per the 4 GiB constraint), so these tests have never been executed; the verifier's `cargo test -p gyre-server --lib ws_subscribe` will fail on them (the agent test panics at the 5 s `.expect`; the member test fails `assert!(saw_telemetry && saw_event)` — the bus event may still arrive via Subscribe replay since `TaskCreated` is persisted, but telemetry never does).

Repair: await the subscription's effect on the server between `subscribe_ws` and the emits — e.g. a short `tokio::time::sleep` (as the pre-existing test does) or, deterministically, poll `state.ws_connection_workspaces` until the connection's authorized list contains the workspace. The REST tests (`workspace_activity_*`) are request/response via `tower::ServiceExt::oneshot` and are not affected.

## Server tests the verifier must execute

- `cargo test -p gyre-server --lib ws_subscribe` (currently expected to FAIL per F1 until the synchronization fix lands)
- `cargo test -p gyre-server --lib workspace_activity`
- `cargo test -p gyre-server --lib ws_` (presence regressions — the membership-seeded helper)
- `cargo test -p gyre-server --lib message::tests::kind_ag_event_type` (already executed here via gyre-common: 129 passed)

## Scope check

Diff base `fb1424f` → candidate `c965147c` touches only `gyre-common/src/message.rs` (ag_event_type + unit test), `gyre-server/src/ws.rs` (Subscribe authorization + tests + presence-test membership seeding), `gyre-server/src/api/activity.rs` (new handler + tests), `gyre-server/src/api/mod.rs` (route), `gyre-server/src/abac_middleware.rs` (resolver entry), `specs/coverage/system/message-bus.md` (rows 9/11/13), `specs/tasks/task-252.md`. No scripts, gates, or unrelated tasks modified; working tree clean at review time.
