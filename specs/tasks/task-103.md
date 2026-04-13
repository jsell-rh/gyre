---
title: "Add agent lifecycle audit event types"
spec_ref: "observability.md §Audit Event Types"
depends_on:
  - task-102
progress: not-started
coverage_sections:
  - "observability.md §Audit Event Types"
commits: []
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

- [ ] All 7 agent lifecycle event types added to `AuditEventType` enum
- [ ] `AgentSpawned` emitted on successful spawn with correct detail fields
- [ ] `AgentCompleted` emitted on agent completion
- [ ] `AgentKilled` emitted on admin kill with reason attribution
- [ ] `AgentDead` emitted by stale agent detector
- [ ] `AgentHeartbeat` emitted on heartbeat
- [ ] `AgentTokenRevoked` emitted on token revocation
- [ ] `AgentReassigned` emitted on task reassignment
- [ ] CEF severity mapping updated for new event types
- [ ] Audit event round-trip serialization tests pass
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/observability.md` §Audit Event Types for the full event type table. The audit domain model is in `gyre-domain/src/audit.rs`. The spawn flow is in `gyre-server/src/api/spawn.rs`. Agent completion is near `POST /api/v1/agents/{id}/complete` — grep for `agent_complete` or `complete` in `gyre-server/src/api/`. The stale agent detector may be in a background job file. Token revocation happens in `auth.rs` or the complete handler. Check `gyre-server/src/siem.rs` for CEF severity mapping. This task depends on task-102 (schema expansion) being completed first.
