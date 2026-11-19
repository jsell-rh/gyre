# Review — task-252 (Workspace-scoped telemetry access: Subscribe membership authorization and the REST activity endpoint)

Spec: `specs/system/message-bus.md` §Storage > Telemetry Store (Subscription authorization) and §API > Telemetry (GET /api/v1/workspaces/:workspace_id/activity). Gaps `2b439cf656cf14c344afdb07`, `64d1be477f27ebf27ec73076`.
Commits under review: `81910e02` (product), `d2fe75e4` (round-1 test-race repair), `181eec81`/`ce2dd495` (task-file docs). Base `bd611083`, candidate `ce2dd495`.
Verdict: **approved** — round-2; the round-1 race finding is repaired, and both capabilities are verified in production code against the spec.

## Probes run (at candidate head `ce2dd495`)

- `cargo check -p gyre-server --lib --tests` → clean (9m03s) — the new WS and REST test code type-checks; evidence `/tmp/stage/review-evidence/cargo-check-gyre-server.txt`.
- `cargo test -p gyre-common --lib` → **129 passed, 0 failed**, including the new `kind_ag_event_type_mapping_matches_spec` and all pre-existing `telemetry_buffer_*` tests.
- `bash scripts/check-abac-route-registry.sh` → OK (new route resolves in the ABAC registry; `git diff base..candidate -- scripts/` is empty — no exemption-file growth).
- `bash scripts/check-arch.sh`, `check-abac-exempt-handlers.sh`, `check-mem-port-contracts.sh`, and the full invariant-script sweep (16 scripts incl. `check-task-commit-attribution`, `check-inert-enforcement`, `check-scope-literal-defaults`) → all PASS; evidence under `/tmp/stage/review-evidence/`.

Deferred to the independent verifier (gyre-server test builds exceed this sandbox's 4 GiB codegen budget — same deferral the implementer recorded):

    cargo test -p gyre-server --lib ws_subscribe
    cargo test -p gyre-server --lib workspace_activity
    cargo test -p gyre-server --lib
    cargo test -p gyre-server --test api_integration   (unscoped /api/v1/activity regression)

## Verified (round 1 finding + both gaps)

- **Round-1 race repair (`d2fe75e4`) is sound.** `await_ws_subscription` polls `state.ws_connection_workspaces` — the same index the Subscribe handler populates *after* authorization and *before* returning to its `select!` loop — so the test's emits happen only after the subscription is registered server-side. This is a happens-before edge through shared state, not a timing sleep. Rejected workspaces never enter the index, so the non-member/foreign-workspace leak assertions stay synchronized by their SubscribeError frames. The member/agent delivery assertions that round 1 proved would lose the telemetry broadcast now have a deterministic sync. Source-verified (analysis in `/tmp/stage/review-evidence/source-analysis.md`); the implementer's standalone-TCP probe artifacts are not present in this checkpoint, so the repair was re-verified from source plus an independent `cargo check --tests`.
- **Subscribe membership authorization (gap 2b43...): real and order-preserving.** The ws.rs handler now checks, in order: the preserved Admin bypass, the preserved tenant-containment check, then workspace membership — users via `WorkspaceMembershipRepository::find_by_user_and_workspace` (real Diesel lookups in both the SQLite and Postgres adapters), agents via their own agent record's `workspace_id`. Lookup errors and missing records fail closed (reject). A rejected scope produces the SubscribeError frame and never enters `subscribed_workspaces`; since both `Destination::Workspace` bus delivery *and* telemetry delivery (`emit_telemetry` broadcasts `to: Destination::Workspace(ws)`) filter on `subscribed_workspaces.contains(ws_id)`, a same-tenant non-member receives neither. The leak probe (`ws_subscribe_rejects_same_tenant_non_member`) fails against the tenant-match-only handler and passes only through the membership gate; the agent probe covers own-workspace delivery plus foreign-workspace rejection.
- **REST telemetry endpoint (gap 64d1...): spec-exact.** `GET /api/v1/workspaces/:workspace_id/activity` is registered beside the sibling messages route, with an ABAC `RouteResourceMapping` entry (`:workspace_id` path param → `resource.workspace_id`, which drives the builtin `workspace-membership-required` Deny at priority 940 for user principals). The handler mirrors `list_workspace_messages`' per-handler authorization exactly (Admin bypass, user membership record, agent-record workspace match, 403 otherwise) — the required pattern. `since` is epoch ms via `TelemetryBuffer::list_since` (`created_at > since`, so `<= since` excluded and 1-ms-newer included), `limit` defaults to 100. The `ActivityEventData` mapping matches the spec table field-for-field: `event_id` ← `message.id`, `agent_id` ← `payload["agent_id"]` with the `Agent(id)` fallback (empty otherwise — no fabricated identity), `event_type` ← new `MessageKind::ag_event_type()` (seven telemetry kinds → AG-UI variants, `AgentError` → `ERROR` per the §MCP Integration table, others → `Custom(kind wire name)`; unit-tested in gyre-common, passing), `description` ← `payload["tool_name"]` or the kind name, `timestamp` ← `created_at`. Response is a bare JSON array.
- **Unchanged paths verified untouched:** the unscoped `/api/v1/activity` handler and the WS `ActivityQuery` handler are not modified by either product commit (diff-verified); `activity_log_query` remains the regression. Presence tests were updated only to seed the membership the production invitation-accept flow writes — necessary and scoped. `message_persistence.rs` subscribes with the global Admin token (bypass preserved), so the restart/replay suite is unaffected.
- **Coverage rows scoped correctly:** only rows 9, 11, 13 of `specs/coverage/system/message-bus.md` updated, with concrete source evidence; no unrelated rows touched.

## Test-inflation check

None found. Both WS tests and all four REST tests drive real surfaces (live TCP WebSocket against `build_router`, `oneshot` through `api_router` with real bearer tokens and the real mem stores) and each fails against the pre-fix/pre-implementation code path it targets (leak assertions receive the delivered frames under the tenant-match-only handler; the 403 assertions get 200/404 when the gate is missing).

## Notes (non-blocking)

- The REST lib tests use `api_router()` without the ABAC middleware layers (matching the sibling `messages.rs` test pattern); the middleware's membership deny is separately exercised platform-wide, and the handler-side check is what the tests target.
- The implementer's claimed probe outputs (`/tmp/stage/review-evidence/probe/…`) from their sandbox are absent in this checkpoint; this review's approval rests on its own evidence (source analysis, cargo check --tests, gyre-common tests, invariant scripts), not on those artifacts.
