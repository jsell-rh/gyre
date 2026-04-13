---
title: "Platform Model Secrets Rotation Background Job"
spec_ref: "platform-model.md §7 Secrets Delivery"
depends_on:
  - task-097
  - task-098
progress: not-started
coverage_sections: []
commits: []
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
