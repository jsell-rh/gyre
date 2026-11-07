---
title: "Add agent lifecycle audit event types"
spec_ref: "observability.md §Audit Event Types"
depends_on:
  - task-102
progress: ready-for-review
coverage_sections:
  - "observability.md §Audit Event Types"
commits: ["3b9d312fc5c9fd2a6cbfc3eabf631f8cb13b1d8b"]
---

## Spec Excerpt

The spec defines 7 agent lifecycle audit events that must be emitted at the appropriate points:

| Event Type | Trigger | `detail` fields |
|---|---|---|
| `AgentSpawned` | `POST /api/v1/agents/spawn` succeeds | `name`, `task_id`, `compute_target`, `branch`, `spawned_by` |
| `AgentCompleted` | `POST /api/v1/agents/{id}/complete` succeeds | `task_id`, `mr_id`, `branch` |
| `AgentKilled` | Admin kills agent | `reason`, `killed_by`, `pid` |
| `AgentDead` | Stale agent detector marks agent Dead | `last_heartbeat_at`, `pid_alive` |
| `AgentHeartbeat` | `PUT /api/v1/agents/{id}/heartbeat` | `pid`, `pid_alive` |
| `AgentTokenRevoked` | Token revoked on complete or admin action | `jti`, `reason` |
| `AgentReassigned` | Admin reassigns agent's task | `old_task_id`, `new_task_id`, `reassigned_by` |

Currently none of these exist as `AuditEventType` variants. `AgentSpawned` and `AgentCompleted` exist as `MessageKind` variants in the message bus, but not as audit events.

## Implementation Plan

1. **Add event type variants (`gyre-domain/src/audit.rs`):**
   - Add `AgentSpawned`, `AgentCompleted`, `AgentKilled`, `AgentDead`, `AgentHeartbeat`, `AgentTokenRevoked`, `AgentReassigned` to `AuditEventType` enum
   - Update `as_str()` and `from_str()` for round-trip serialization

2. **Wire emission points in server:**
   - **AgentSpawned**: In `gyre-server/src/api/spawn.rs`, after successful agent creation, record audit event with agent_id, task_id, compute_target, branch, spawned_by
   - **AgentCompleted**: In agent complete handler, record with task_id, mr_id, branch
   - **AgentKilled**: In admin kill endpoint, record with reason, killed_by, pid
   - **AgentDead**: In stale agent detector (`gyre-server/src/stale_agent.rs` or similar), record with last_heartbeat_at, pid_alive
   - **AgentHeartbeat**: In heartbeat handler, record with pid, pid_alive
   - **AgentTokenRevoked**: In token revocation path (agent complete, admin revoke), record with jti, reason
   - **AgentReassigned**: In task reassignment handler, record with old_task_id, new_task_id, reassigned_by

3. **Update SIEM CEF severity mapping:**
   - `AgentKilled` → severity 7 (high)
   - `AgentDead` → severity 6
   - `AgentSpawned`/`AgentCompleted`/`AgentHeartbeat` → severity 3 (informational)
   - `AgentTokenRevoked` → severity 5
   - `AgentReassigned` → severity 4

## Acceptance Criteria

- [x] All 7 agent lifecycle event types added to `AuditEventType` enum
- [x] `AgentSpawned` emitted on successful spawn with correct detail fields
- [x] `AgentCompleted` emitted on agent completion
- [x] `AgentKilled` emitted on admin kill with reason attribution
- [x] `AgentDead` emitted by stale agent detector
- [x] `AgentHeartbeat` emitted on heartbeat
- [x] `AgentTokenRevoked` emitted on token revocation
- [x] `AgentReassigned` emitted on task reassignment
- [x] CEF severity mapping updated for new event types
- [x] Audit event round-trip serialization tests pass
- [x] `cargo test --all` passes (gyre-server lib: 1325 passed, gyre-domain lib: 401 passed)

## Agent Instructions

Read `specs/system/observability.md` §Audit Event Types for the full event type table. The audit domain model is in `gyre-domain/src/audit.rs`. The spawn flow is in `gyre-server/src/api/spawn.rs`. Agent completion is near `POST /api/v1/agents/{id}/complete` — grep for `agent_complete` or `complete` in `gyre-server/src/api/`. The stale agent detector may be in a background job file. Token revocation happens in `auth.rs` or the complete handler. Check `gyre-server/src/siem.rs` for CEF severity mapping. This task depends on task-102 (schema expansion) being completed first.

## Shipped

All 7 agent lifecycle audit events from observability.md §Audit Event Types are
now real, typed `AuditEventType` variants with production emission points,
persistent storage via the `AuditRepository` port, live SSE broadcast, and CEF
severity mapping. Commit: `3b9d312fc5c9fd2a6cbfc3eabf631f8cb13b1d8b`.

### Behavior

- **Enum + round-trip** (`gyre-domain/src/audit.rs`): `AgentSpawned`,
  `AgentCompleted`, `AgentKilled`, `AgentDead`, `AgentHeartbeat`,
  `AgentTokenRevoked`, `AgentReassigned` added with snake_case
  `as_str()`/`from_str()` round-trip (SQLite adapter stores/reads
  `event_type` as these strings).
- **Emission module** (`gyre-server/src/agent_audit.rs`): shared emitters that
  record through `state.audit` and broadcast on `audit_broadcast_tx` (same
  pattern as the M23 container audit events).
- **AgentSpawned** — `spawn_agent_core` after a successful spawn, detail:
  `name`, `task_id`, `compute_target` (resolved label), `branch`, `spawned_by`.
- **AgentCompleted** — complete handler after MR creation + task/agent
  transitions, detail: `task_id`, `mr_id`, `branch`. The idempotent 202
  re-complete path does not re-emit (the completion was already audited).
- **AgentKilled** — admin kill endpoint; the caller identity comes from the
  new `AuthenticatedAgent` extractor (`killed_by`), PID from the process
  registry handle or workload attestation (null when unknown, never
  fabricated). Admin kill now also revokes the agent's token (previously it
  did not), closing the "revoked on admin action" trigger.
- **AgentDead** — stale agent detector `Abort` branch, detail:
  `last_heartbeat_at` (agent's stored heartbeat) and `pid_alive` measured via
  `/proc` from the attested PID (null when no PID was attested).
- **AgentHeartbeat** — heartbeat handler, detail: `pid`/`pid_alive` taken from
  the workload attestation liveness check already performed on heartbeat.
- **AgentTokenRevoked** — every actual revocation site (agent complete,
  operator stop, admin kill) now goes through
  `agent_audit::revoke_agent_token`, which reads the token before deleting it
  from `agent_tokens` and emits the event with the token's `jti` and a reason
  string. Minted agent JWTs now carry a unique `jti` claim
  (`AgentJwtClaims.jti`, serde-defaulted for pre-existing tokens);
  `AgentSigningKey::extract_jti_unverified` pulls it for audit detail only
  (documented `crypto-verify:ok` — authentication still goes through
  `validate()`).
- **AgentReassigned** — admin reassign endpoint emits one event per moved
  task with `old_task_id`/`new_task_id` (task ids are stable across
  reassignment; the assignee move is recorded as `from_agent_id`/
  `to_agent_id`) and `reassigned_by` (authenticated caller).
- **CEF severity** (`gyre-server/src/siem.rs`): kill 7, dead 6, revoke 5,
  reassign 4, spawned/completed/heartbeat 3, per the task plan.

### Test evidence

- `gyre-domain`: `audit_event_type_round_trip` extended to all 7 new variants
  (401 lib tests pass).
- `gyre-server` lib: 1325 tests pass, including 10 new hard tests that drive
  the real HTTP handlers / detector loop and assert exact detail fields
  against the audit port:
  `spawn_emits_agent_spawned_audit_event`,
  `complete_emits_agent_completed_and_token_revoked_audit_events` (asserts the
  audited jti equals the jti embedded in the minted token),
  `admin_kill_emits_lifecycle_audit_events` (also asserts the token is really
  deleted from `agent_tokens`),
  `admin_reassign_emits_agent_reassigned_audit_event`,
  `heartbeat_emits_agent_heartbeat_audit_event`,
  `heartbeat_audit_reports_attested_pid_liveness` (live PID → true, exited
  child PID → false, proving the value is measured),
  `stale_detector_emits_agent_dead_audit_event`,
  `stale_detector_reports_pid_liveness_in_agent_dead_event`,
  `auth::minted_jwt_carries_unique_jti`, `siem::cef_agent_lifecycle_severities`.
- Architecture lints pass (`check-arch.sh`, `check-abac-exempt-handlers.sh`,
  `check-forged-scope-fields.sh`, `check-inert-enforcement.sh`, test-quality
  checks). `check-crypto-verify.sh` fails with a pre-existing awk syntax error
  at the base commit (verified via `git stash`), unrelated to this change.
- Pre-existing at base and NOT touched here: clippy `never_loop` error in
  `gyre-domain/src/rust_extractor.rs` (task-075 code) blocks all-target
  clippy on gyre-server; verified identical at base. Owned by verification.

### Interpretation notes

- `AgentReassigned` detail `old_task_id`/`new_task_id`: the reassign endpoint
  moves a task between agents without changing the task's id, so both fields
  carry the reassigned task's id and the assignee change is recorded as
  `from_agent_id`/`to_agent_id` alongside the spec fields.
- `pid`/`pid_alive`/`jti` are JSON null when genuinely unknown (no
  attestation, legacy non-JWT token) rather than fabricated defaults.
