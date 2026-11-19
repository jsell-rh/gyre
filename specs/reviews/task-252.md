# Review — task-252 (Workspace-scoped telemetry access: Subscribe membership authorization and the REST activity endpoint)

Spec: `specs/system/message-bus.md` §Telemetry Store (Subscription authorization) and §API > Telemetry (GET /api/v1/workspaces/:workspace_id/activity).
Candidate under review: `1c16b007eae5c53f8ab8fd81f4dafdb0a19c60b9` (branch `pipeline/task-252/076781eca3f1420b971fb579e386e96d-1`), base `bafc03531aec9c9ddd05c52497aa10d0d8856fe5`.
Verdict: **approved** (no blocking findings). This round is a rebase repair onto task-249's base plus process/docs commits; the two capabilities were re-verified end-to-end on the merged tree by code inspection, focused probes, and every mechanical invariant.

## Diff scope (base → candidate)

9 files: `gyre-common/src/message.rs` (new `MessageKind::ag_event_type` + unit test; `broadcast_only()` from main kept side-by-side), `gyre-server/src/ws.rs` (Subscribe membership gate + leak-probe tests), `gyre-server/src/api/activity.rs` (new `workspace_activity_handler` + 4 tests), `api/mod.rs` (route), `abac_middleware.rs` (RouteResourceMapping entry), `scripts/test_dev_cargo_clean.py` (CI repair for Cargo 1.100 build-unit layout), `specs/coverage/system/message-bus.md` (rows 9/11/13), `specs/tasks/task-252.md`, `web/src/__tests__/WorkspaceSettings.test.js` (conflict resolution: frozen clock + main's slack). No exemption-file changes; no `web/dist` changes (verifier-owned).

## Subscribe membership authorization (gap 2b439cf656cf14c344afdb07) — verified

`crates/gyre-server/src/ws.rs` Subscribe handler authorizes each requested workspace in order: preserved Admin bypass → preserved tenant containment → **real membership verification**: user connections via `state.workspace_memberships.find_by_user_and_workspace(user_id, ws_id)` (fail-closed on `Err`), agent connections via `state.agents.find_by_id` scoped to the agent record's own workspace (fail-closed on `Err`/missing record). Rejected scopes land in `rejected_workspaces`, produce the `SubscribeError` frame, and are absent from `subscribed_workspaces` and the `ws_connection_workspaces` broadcast index — so no bus messages, telemetry, or replay are delivered for them. `find_by_user_and_workspace` is implemented in all three adapters (mem, sqlite, postgres), so the port contract holds.

Leak-probe quality (`ws_subscribe_rejects_same_tenant_non_member`): the outsider (tenant "default", no membership) `.expect`s a `SubscribeError` frame within 5s. Against the base tenant-match-only handler the subscription would be *accepted* and no such frame would arrive (the seeded workspace has no Event-tier messages to replay), so the expect times out — the test genuinely fails on pre-fix code. The member control in the same tenant must receive BOTH the telemetry and the workspace-scoped bus message, pinning that the rejection is membership, not tenant. The agent test (`ws_subscribe_agent_scoped_to_own_workspace_record`) likewise expects a `SubscribeError` for a same-tenant foreign workspace that the base handler would have authorized.

## REST telemetry endpoint (gap 64d1be477f27ebf27ec73076) — verified

`GET /api/v1/workspaces/:workspace_id/activity` (`workspace_activity_handler`, api/activity.rs) is registered in `api/mod.rs` and in the ABAC `RouteResourceMapping` (resource "activity", workspace path param) — `check-abac-route-registry.sh` passes with no new exemptions. Handler: per-handler membership enforcement (403) mirroring `list_workspace_messages` — users via `find_by_user_and_workspace`, agent tokens via the agent record's workspace (this covers the case the builtin `workspace-membership-required` deny does not, since that policy keys on `subject.type == "user"`); Admin skips the handler check, and the builtin deny (priority 940, no admin exemption) still applies at the middleware — same layered pattern as the sibling workspace-scoped routes. Backing store is `state.telemetry_buffer.list_since` (strictly-greater `created_at > since_ms`, so `created_at <= since` excluded and 1-ms-newer included — matching the task AC and the boundary test), `limit` default 100. Field mapping verified against the spec table and the `AgEventType` serde (manual `Serialize` via `as_str`): event_id ← `message.id`; agent_id ← `payload["agent_id"]` with `Agent(id)` origin fallback (empty for Server origin); event_type ← `MessageKind::ag_event_type` (7 telemetry kinds → AG-UI strings, AgentError → ERROR, others → Custom(kind wire name)); description ← `payload["tool_name"]` or kind name; timestamp ← `created_at` (epoch ms). The unscoped `/api/v1/activity` handler and the WS `ActivityQuery` path are byte-unchanged. The gyre-common unit test `kind_ag_event_type_mapping_matches_spec` pins the mapping table.

Test quality: `workspace_activity_agent_token_scoped_to_own_workspace` asserts 403 for a foreign workspace and 200 for the agent's own — if the handler's agent-record check were removed, the foreign request would return 200 and the test fails (the 200-for-own assertion also proves the middleware is not blanket-denying agent tokens). The non-member 403 is additionally enforced by the builtin ABAC deny at the middleware, so the REST enforcement has two independent layers.

## Probes run (this sandbox, merged tree at `1c16b007`)

Evidence under `/tmp/stage/review-evidence/`:

- `cargo check -p gyre-server --lib` → exit 0 (cold, 7m36s).
- `cargo check -p gyre-server --tests` → exit 0 — all new tests compile (cargo-check-gyre-server-tests.txt).
- `cargo test -p gyre-common --lib` → **130 passed, 0 failed**, incl. `kind_ag_event_type_mapping_matches_spec` and the rebased `broadcast_only_returns_true_for_dataseeded_queueupdated_only`.
- Mechanical invariants all exit 0: `check-abac-route-registry`, `check-arch`, `check-migration-versions`, `check-byte-slice-truncation`, `check-mem-port-contracts`, `check-abac-exempt-handlers`, `check-dead-message-kinds`, `check-inert-enforcement`, `check-scope-literal-defaults`, `check-forged-scope-fields`, `check-fabricated-scope-defaults`, `check-lossy-secret-conversion`, `check-relative-path-defaults`, `check-mcp-write-tools`, `check-migration-sql-portability`, `check-unbounded-external-http`, `check-forwarded-header-trust`, `check-fail-open-ref-resolution`, `check-task-commit-attribution`.
- `web/`: `npm ci` (locked versions) then `npx vitest run src/__tests__/WorkspaceSettings.test.js` → **85/85 passed** (vitest 4.1.0 from the lockfile — an initial ad-hoc `npx vitest@5` run from the repo root failed to parse the .svelte import; rerun from `web/` with the locked toolchain is the valid probe).

## Infrastructure restrictions (not code defects)

- The executor's cargo wrapper blocks gyre-server test codegen ("gyre-server code generation plus the model exceeds 4 GiB") — server test execution is deferred to the independent verifier, per the review contract.
- The wrapper also blocks ANY bare `cargo test` (package-less), verified with a trivial 3-line temp project (wrapper-bare-cargo-test-probe.txt). Consequently `python3 -m unittest scripts.test_dev_cargo_clean` fails in this sandbox with the wrapper's own error text as the cargo output — an environment restriction, not the candidate's code (the test's Cargo-1.100 JSON-message-based sentinel logic is sound by inspection).

## Required verifier runs (recorded for remote verification)

- `cargo test -p gyre-server --lib ws_subscribe_rejects_same_tenant_non_member`
- `cargo test -p gyre-server --lib ws_subscribe_agent_scoped_to_own_workspace_record`
- `cargo test -p gyre-server --lib workspace_activity` (4 tests: maps_fields_per_spec, since_boundary_and_limit, rejects_same_tenant_non_member, agent_token_scoped_to_own_workspace)
- `cargo test -p gyre-common --lib` (already green in-sandbox; re-run at the integrated head)
- `python3 -m unittest scripts.test_dev_cargo_clean` (blocked in-sandbox by the cargo wrapper, not by code)
- Full `cd web && npm test`, workspace clippy/fmt, and CI at the merged head.
