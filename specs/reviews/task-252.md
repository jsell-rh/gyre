# Review — task-252 (Workspace-scoped telemetry access: Subscribe membership authorization and the REST activity endpoint)

Spec: `specs/system/message-bus.md` §Telemetry Store (Subscription authorization) + §API > Telemetry (workspace activity endpoint).
Candidate under review: `51495fad56412b0e511e8bac98f4b2a8bcdfb03e` (merge of the task-252 branch with main `321615ec`, task-250's replay work).
Product commits: `81910e02` (implementation), `d2fe75e4` (test synchronization fix) — both recorded in frontmatter.
Verdict: **approved** — no blocking findings; server test execution deferred to the independent verifier per the 4 GiB budget (commands recorded below).

## Probes run (this review, at candidate `51495fad`)

- `bash scripts/check-abac-route-registry.sh` → **OK** (the new `/api/v1/workspaces/:workspace_id/activity` route resolves in the ABAC registry; no new exemption-file entries).
- `bash scripts/check-abac-exempt-handlers.sh` → **OK (85 handlers)**.
- `bash scripts/check-arch.sh` → **passed**.
- `bash scripts/check-task-commit-attribution.sh` → **OK** (completes in this sandbox, contrary to the implementer's note; both product commits are in the frontmatter).
- `bash scripts/check-byte-slice-truncation.sh`, `check-fabricated-scope-defaults.sh`, `check-scope-literal-defaults.sh`, `check-inert-enforcement.sh` → all **OK**.
- `/tmp/stage/bin/cargo check -p gyre-server` → **clean** (7m30s) on the merged head.
- `/tmp/stage/bin/cargo check -p gyre-server --tests` → **clean** (3m41s) — the new lib tests and the merged integration tests type-check.
- `/tmp/stage/bin/cargo test -p gyre-common --lib` → **129 passed**, including the new `kind_ag_event_type_mapping_matches_spec` and the full `telemetry_buffer_*` suite (list_since strict-`>` boundary, per-workspace eviction, isolation).
- `/tmp/stage/bin/cargo fmt -p gyre-server -- --check` → clean.
- Full diff inspection `321615ec..51495fad` (7 files: ws.rs, api/activity.rs, api/mod.rs, abac_middleware.rs, gyre-common/message.rs, coverage matrix, task file) plus line-level trace of every enforcement path.

Evidence saved under `/tmp/stage/review-evidence/`.

## Verified — gap 2b439cf656cf14c344afdb07 (Subscribe membership authorization)

- The ws.rs Subscribe handler now authorizes each requested workspace in three ordered steps: preserved Admin bypass (`caller.roles.contains(&UserRole::Admin)` → all workspaces), preserved tenant containment (`workspace.tenant_id != caller.tenant_id` → reject), then real membership: user connections (`caller.user_id: Some`) resolve `state.workspace_memberships.find_by_user_and_workspace` — the same store and method the REST handlers use (messages.rs:461, repo_access.rs, git_http.rs); agent connections (`user_id: None`) resolve `state.agents.find_by_id(&caller.agent_id)` and require `agent.workspace_id == *ws_id`. Lookup errors fail closed (reject + warn). A rejected scope is named in the `SubscribeError` frame and excluded from `subscribed_workspaces`.
- Delivery isolation is structural, not incidental: both the bus-delivery arm (`Destination::Workspace(ws_id)` → `subscribed_workspaces.contains(ws_id)`) and the per-connection index (`ws_connection_workspaces`, used by presence rebroadcast) key on the membership-gated `authorized_workspaces` value, and task-250's replay window iterates the same list — so a rejected non-member receives neither telemetry, nor workspace bus messages, nor replay.
- Cross-checked the auth paths the check depends on: API-key users get `user_id: Some` + `roles: user.roles` (`User::new` defaults to `vec![UserRole::ReadOnly]` — not Admin, so the membership check actually applies); plain agent tokens get `user_id: None` with `tenant_id = system_principal_tenant()` ("default"), so tenant containment passes for the agent's own tenant and the agent-record check is what scopes it; the global token gets `roles: [Admin]` → preserved bypass.
- The leak-probe test (`ws_subscribe_rejects_same_tenant_non_member`) matches the task's verification intent and kills the bug: with the pre-fix tenant-match-only handler the outsider's Subscribe succeeds, no `SubscribeError` frame arrives, and the first assertion fails. The member control (same tenant, membership row) receives both the telemetry and the workspace bus message, proving the 403/rejection is the membership check, not the tenant. The agent test covers own-workspace acceptance and same-tenant foreign-workspace rejection with a no-delivery assertion. `d2fe75e4`'s `await_ws_subscription` index-poll fixes a real race (client-side send ≠ server-side processing on the current-thread test runtime) deterministically.
- Test-seeding correctness verified against the adapters: mem `WorkspaceRepository::create` is idempotent for the same id (the enforcement test re-seeds the same workspace id for member and outsider — no duplicate bail), and mem + SQLite both implement `find_by_user_and_workspace` genuinely. The `seed_api_key_user` membership seeding keeps the pre-existing presence tests valid under the new gate.

## Verified — gap 64d1be477f27ebf27ec73076 (REST telemetry endpoint)

- `GET /api/v1/workspaces/:workspace_id/activity` (`workspace_activity_handler`) is registered in api/mod.rs and backed by `state.telemetry_buffer.list_since` — `since` in epoch ms with the strict `created_at > since` filter (entries `<= since` excluded, 1-ms-newer included), `limit` defaulting to 100, oldest-first within the window.
- Per-handler authorization mirrors the sibling `list_workspace_messages` exactly (diffed side by side): Admin bypass, API-key users require a `WorkspaceMembershipRepository` record, agent tokens require the agent record's workspace to match — 403 otherwise. The tests drive `api_router()` WITHOUT the ABAC middleware layer, so the 403s proven are the handler's own check, not middleware side effects; in production the middleware's builtin `workspace-membership-required` deny (priority 940, keyed on `subject.member_of_resource_workspace`, derived from `resource.workspace_id` which the `:workspace_id` path param feeds per the documented convention) is an additional first line for user principals, and agent tokens are covered by the handler.
- Field mapping matches the spec table exactly: `event_id ← message.id`, `agent_id ← payload["agent_id"]` with `MessageOrigin::Agent(id)` fallback and empty-string default (never a fabricated identity), `event_type ← MessageKind::ag_event_type()` (the seven telemetry kinds → their AG-UI strings, AgentError → ERROR per the §MCP Integration table, other kinds → Custom(kind wire name) — verified against `AgEventType::as_str`/Serialize in protocol.rs), `description ← payload["tool_name"]` or the kind name, `timestamp ← created_at`. The mapping test asserts all five fields on all three origin/payload combinations.
- The unscoped `/api/v1/activity` handler is byte-identical to main (notification-synthesized, untouched), and the WS ActivityQuery path is unchanged.
- Route registered in the ABAC `RouteResourceMapping` ("activity" resource, `:workspace_id` param); registry script passes with no exemption-file growth.

## Merge coherence (base `321615ec` = task-250 replay work)

The only conflicted file was the coverage matrix (resolved by combining both tasks' row-9/11/13 notes — inspected, both tasks' evidence is preserved). In ws.rs the membership gate sets `subscribed_workspaces` before task-250's replay window reads it, so a rejected non-member gets no replay either; the presence-test seeding change is scoped to the test helper. No cross-task interference found.

## Test-inflation check

None found. All new assertions are concrete and would fail on regression: the SubscribeError frame + rejected-workspace name, the nothing-delivered leak assertion (after the member control proves both emissions were broadcast), the exact 403-vs-200 member/outsider pair, the `<= since` / 1-ms-newer boundary vectors, and the exact five-field mapping. No conditional guards, no mirrored logic, no stub-backed assertions.

## Minor (non-blocking, recorded for completeness)

- `docs/api-reference.md` does not yet list `GET /api/v1/workspaces/{workspace_id}/activity` (sibling workspace-scoped routes are listed, e.g. the messages route at line 40). The task contract and spec section are satisfied by the code; a one-line docs row would fit repo convention and can ride along with any next docs touch — not a spec violation, not blocking.
- The REST handler does not cap `limit` (the sibling messages route caps at 500). Harmless here: `list_since` bounds the response by the per-workspace ring buffer (default 10,000), so a huge `limit` cannot amplify beyond buffered entries.

## Tests the independent verifier must execute

gyre-server test compilation was deferred (4 GiB budget); type-checking of lib + all test targets passed in review:

- `cargo test -p gyre-server --lib ws_subscribe_rejects_same_tenant_non_member`
- `cargo test -p gyre-server --lib ws_subscribe_agent_scoped_to_own_workspace_record`
- `cargo test -p gyre-server --lib workspace_activity_maps_fields_per_spec`
- `cargo test -p gyre-server --lib workspace_activity_since_boundary_and_limit`
- `cargo test -p gyre-server --lib workspace_activity_rejects_same_tenant_non_member`
- `cargo test -p gyre-server --lib workspace_activity_agent_token_scoped_to_own_workspace`
- Regression (task-250 same-handler work + merge): `cargo test -p gyre-server --lib ws_replay`, `cargo test -p gyre-server --test message_persistence`
- Unscoped-path regression: `cargo test -p gyre-server --lib ws_activity_event`, `cargo test -p gyre-server --lib ws_` (full WS module sweep recommended)
