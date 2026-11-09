---
title: "Platform Model Secrets Rotation Background Job"
spec_ref: "platform-model.md §7 Secrets Delivery"
depends_on:
  - task-097
  - task-098
progress: complete
coverage_sections: []
commits: ["abd7dab3b2755cfef567d8669dcad7e77b24f22e", "afd5ab18bb1b17954e04de5aafdcb6a6f4f690fe", "c7f6d13d5049ee53a0bcbc4d24f613cb418722a4", "a42d006b8a24dbfb3fb92ea514f9ef6e5ede0347"]
---

## Spec Excerpt

### Rotated Secret Type

| Type | Example | Lifecycle |
|---|---|---|
| Rotated | OAuth tokens, Claude Max refresh tokens | Background job refreshes before expiry |

Secrets with `expires_at` set require proactive rotation. A background job checks for expiring secrets and triggers rotation or creates admin notifications.

## Implementation Plan

1. **Rotation check background job:**
   - `spawn_secret_rotation_check` — runs every 5 minutes (similar to `spawn_budget_daily_reset`)
   - Queries secrets where `expires_at` is within the next hour
   - For each expiring secret:
     a. If secret has a rotation handler (e.g., OAuth token refresh): attempt automatic rotation
     b. If no handler: create notification for admin users (priority 7 — Budget warning level)

2. **Agent teardown secret revocation:**
   - When an agent completes or is killed, revoke any task-scoped ephemeral secrets
   - Delete task-scoped secrets from the secrets table
   - This runs in the agent completion handler alongside policy cleanup

3. **Secret access audit logging:**
   - Log `secret.accessed` event when resolve_for_agent returns secrets (at spawn time)
   - Log `secret.rotated` event when rotation occurs
   - Log `secret.revoked` event on agent teardown
   - All events include secret name (not value), scope, and agent_id

## Acceptance Criteria

- [ ] Background job checks for expiring secrets every 5 minutes
- [ ] Admin notification created for secrets expiring within 1 hour
- [ ] Task-scoped secrets revoked on agent completion/kill
- [ ] Audit events logged for access, rotation, revocation
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/platform-model.md` §7 "Secrets Delivery" for the rotation and lifecycle requirements. For background job patterns, see how `spawn_budget_daily_reset` and `spawn_stale_agent_detector` are implemented in `gyre-server/src/lib.rs` and `gyre-server/src/jobs.rs`. For agent teardown, see the agent completion handler in the MCP `gyre_agent_complete` tool and the admin kill handler. Audit events follow the pattern in `gyre-server/src/constraint_check.rs` (tracing macros with `event` field).

## Shipped

### Behavior

All five acceptance criteria are implemented in production code on top of the task-098 `secret_rotation_check` job (already spawned every 5 minutes via `jobs::spawn_job` in `main.rs`):

- **Automatic rotation via handlers.** `secret_rotation.rs` defines a `SecretRotationHandler` trait plus a shipped `OAuthRefreshHandler` for secrets whose name ends in `_refresh_token` (e.g. `oauth_refresh_token`, `claude_max_refresh_token`). The secret's plaintext value is a JSON credential document (`token_endpoint`, `client_id`, optional `client_secret`, `refresh_token`) set by an admin through the task-098 Admin API. The handler performs a real RFC 6749 §6 `refresh_token` grant over the latency-bounded AppState HTTP client (30 s timeout), stores the rotated credential document (new access token + rotated refresh token when the provider issues one), sets `last_rotated_at`, and extends `expires_at` by the provider's `expires_in` — so a successfully rotated secret leaves the 1-hour warning window. Rotation failure (HTTP error, malformed value, missing credential fields) or a missing handler falls back to the existing priority-7 `SecretExpiring` admin notification.
- **Teardown revocation.** `revoke_task_secrets()` deletes every Task-scoped secret for the agent's task. A shared wrapper `revoke_agent_task_secrets(state, agent)` resolves the agent's current task (no-op for taskless agents such as orchestrators) and is called from **every path that ends an agent run**:
  - MCP `gyre_agent_complete` (after the agent status update succeeds) and REST `POST /api/v1/agents/{id}/complete` (`complete_agent`, alongside token revocation and interrogation-policy cleanup — review round-1 F1);
  - REST `POST /api/v1/agents/{id}/stop` (`stop_agent`, alongside token revocation — review round-2 F1) and `POST /api/v1/agents/{id}/fail` (`fail_agent`, after the terminal Failed transition — review round-2 F2);
  - `POST /api/v1/admin/agents/{id}/kill` (alongside policy cleanup);
  - the spec-reject cascaded shutdown (`reject_spec` in `api/specs.rs`, which cancels the spec's in-flight tasks and stops their Active agents — review round-2 F3, mirroring the repo-archive stop);
  - the stale-agent detector's **both** arms — heartbeat abort (the kill that marks the agent Dead) and the `DisconnectedBehavior::Pause` stop (review round-3 F1; Pause is the default behavior and Stopped is terminal, so the run ends there exactly as it does under Abort);
  - `PUT /api/v1/agents/{id}/status` when the parsed new status is terminal (Dead/Failed/Stopped — review round-3 F2; the generic status endpoint ends the run just like the dedicated stop/fail/kill endpoints);
  - compose teardown (`POST /api/v1/compose/teardown`), repo-archive agent stop (the agents' tasks are cancelled, so their one-time secrets must not outlive the task), and the process/container/SSH exit monitors in `spawn.rs` (the run's process is gone).

  The tenant is resolved from the agent's workspace record; an unresolvable workspace skips revocation with a warn — no fabricated scope.
- **Audit events** (tracing macros with `event` field, the `constraint_check.rs` pattern; secret name, never value):
  - `secret.accessed` — logged per secret at spawn time when `resolve_for_agent` returns, with agent_id, secret_name, and the resolution scope (tenant/workspace/repo/task ids).
  - `secret.rotated` — logged on background handler rotation (`rotated_by = "secret_rotation_check"`) and on admin force-rotate (`POST /api/v1/secrets/:id/rotate`, `rotated_by` = admin id), with name, scope, and new expiry.
  - `secret.revoked` — logged per secret deleted at agent teardown, with agent_id, secret_name, scope `task`, and task_id.

### Test evidence

- `cargo test -p gyre-server --lib` — **1497 passed, 0 failed** (after the round-3 repair), including:
  - `secret_rotation::tests::rotation_handler_refreshes_expiring_oauth_secret` — real refresh grant against a local token endpoint; asserts the grant posted the stored refresh token/client_id, the stored value is the rotated document, expiry extended past the window, `last_rotated_at` set, and no admin notification.
  - `secret_rotation::tests::rotation_handler_failure_falls_back_to_admin_notification` — 500 from the endpoint leaves the value untouched and notifies the admin.
  - `secret_rotation::tests::malformed_handler_value_falls_back_to_admin_notification`.
  - `secret_rotation::tests::revoke_task_secrets_deletes_only_that_task_scope` / `..._skips_unresolvable_workspace`.
  - `mcp::tests::agent_complete_revokes_task_scoped_secrets`, `api::admin::tests::admin_kill_revokes_task_scoped_secrets`, and (round-1) `api::spawn::tests::complete_agent_revokes_task_scoped_secrets` — the REST completion path.
  - (round-2) `api::spawn::tests::stop_agent_revokes_task_scoped_secrets` and `api::spawn::tests::fail_agent_revokes_task_scoped_secrets` — REST stop/fail; `api::specs::tests::reject_spec_revokes_task_scoped_secrets` — the spec-reject cascaded shutdown. Each asserts the task-scoped secret is deleted while workspace-scoped and other-task secrets survive. All three were verified to FAIL when the fix call sites are removed (negative control run during the round-2 repair; evidence: `/tmp/stage/review-evidence/task-100-round2-repair.md`) — bug-killing, not self-confirming.
  - (round-3) `stale_agents::tests::stale_detector_pause_revokes_task_scoped_secrets` — Pause-behavior stale stop revokes the task-scoped secret while workspace and other-task secrets survive; `api::agents::tests::update_agent_status_terminal_revokes_task_scoped_secrets` — PUT status `dead` revokes the task-scoped secret while a non-terminal `idle` transition (control agent, own task) and workspace/other-task secrets survive. Both were verified to FAIL when the new call sites are removed (negative control run during this round-3 repair, alongside the six sibling revocation tests which stayed green; evidence: `/tmp/stage/review-evidence/task-100-round3-repair.md`).
  - Existing task-098 tests (expiring/expired notify, dedup, healthy never notify) still pass unchanged.
- `cargo test -p gyre-adapters secret` — 17 passed (adapter contract untouched).
- `cargo clippy -p gyre-server --all-targets` — no warnings in any region touched by this task (the repo-wide pre-existing warning set is unchanged by the round-3 repair).
- `cargo fmt -p gyre-server -- --check` — clean (the round-3 repair also normalized rustfmt drift the round-2 repair had left in `spawn.rs`/`mcp.rs`).
- All mechanical check scripts pass (`check-arch`, `check-unbounded-external-http`, `check-fabricated-scope-defaults`, `check-lossy-secret-conversion`, `check-scope-literal-defaults`, `check-inert-enforcement`, `check-task-commit-attribution`, and the rest).

### Notes

- No port or storage changes: rotation reuses `SecretRepository::rotate` + the delete/create metadata rewrite pattern already used by the Admin API's force-rotate-with-expiry; teardown reuses `list_by_scope(Task, ..)` + `delete`.
- `coverage_sections` stays empty (as decomposed); the §7 coverage rows in `specs/coverage/system/platform-model.md` referencing task-100 residuals were updated to reflect the landed lifecycle.
