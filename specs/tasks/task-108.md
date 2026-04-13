---
title: "Implement admin user impersonation"
spec_ref: "identity-security.md §Impersonation"
depends_on:
  - task-102
progress: not-started
coverage_sections:
  - "identity-security.md §Impersonation"
commits: []
---

## Spec Excerpt

From `identity-security.md` §Impersonation:

> Support **user impersonation** for enterprise support scenarios.
> Requirements:
> - Target user must **provide a code or explicit approval** before impersonation begins.
> - Impersonated user is **notified** when someone impersonates them.
> - **Full audit trail** of all actions taken during impersonation — clearly attributed to the impersonator acting as the target.
> - Authorization pattern inspired by AP2: intent → signed mandate → receipt with full audit trail.

From `observability.md` §Audit Event Types:

| Event Type | Trigger | `detail` fields |
|---|---|---|
| `ImpersonationStarted` | Admin begins impersonating user | `impersonator_id`, `target_user_id`, `approval_token` |
| `ImpersonationEnded` | Impersonation session ends | `impersonator_id`, `target_user_id`, `duration_secs`, `actions_taken` |

## Implementation Plan

1. **Impersonation request/approval flow:**
   - `POST /api/v1/admin/impersonate` — Admin requests to impersonate a user
     - Request: `{ target_user_id: String }`
     - Response: `{ request_id: String, approval_code: String }` — a short-lived code sent to the target user
   - `POST /api/v1/admin/impersonate/:request_id/approve` — Target user approves with the code
     - Request: `{ approval_code: String }`
     - Response: `{ impersonation_token: String, expires_at: u64 }` — short-lived JWT with impersonation claims

2. **Impersonation token:**
   - JWT with additional claims: `impersonator_id`, `impersonating: true`, `target_user_id`
   - Short TTL (15 minutes, configurable via `GYRE_IMPERSONATION_TTL`)
   - All API calls with this token are attributed to the impersonator in audit trail
   - The `sub` claim is the target user (so permissions match), but `impersonator_id` is always present

3. **Notification to target user:**
   - When impersonation starts, create a notification for the target user:
     - Type: `ImpersonationAlert` (add to `NotificationType` enum if needed)
     - Message: "Admin {impersonator_name} is impersonating your account"
   - Send via WebSocket if the target user is online
   - Also send email notification if email is configured

4. **Audit trail:**
   - Emit `ImpersonationStarted` audit event when impersonation begins (with impersonator_id, target_user_id, approval_token)
   - Emit `ImpersonationEnded` audit event when impersonation token expires or is explicitly ended
   - Track `actions_taken` count during the impersonation session
   - All API calls during impersonation carry `X-Gyre-Impersonator` header in audit records

5. **End impersonation:**
   - `DELETE /api/v1/admin/impersonate/:request_id` — Admin explicitly ends impersonation
   - Automatic end on token expiry
   - `ImpersonationEnded` event includes duration and action count

6. **Authorization:**
   - Only `Admin` role can request impersonation
   - Impersonation of other Admin users requires `SuperAdmin` or is prohibited
   - Rate limit: max 1 active impersonation per admin at a time

## Acceptance Criteria

- [ ] Admin can request impersonation via POST /api/v1/admin/impersonate
- [ ] Target user receives approval code
- [ ] Target user can approve/deny the impersonation
- [ ] Impersonation token issued with correct claims on approval
- [ ] Target user notified when impersonation starts
- [ ] Full audit trail: ImpersonationStarted/Ended events emitted
- [ ] All actions during impersonation attributed to impersonator in audit
- [ ] Impersonation auto-expires after TTL
- [ ] Only Admin role can impersonate
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/identity-security.md` §Impersonation for the full requirements. Also read `specs/system/observability.md` §Audit Event Types for the impersonation audit events. Auth/JWT handling is in `gyre-server/src/auth.rs`. Notifications are in `gyre-server/src/api/notifications.rs` (or similar — grep for `NotificationType`). WebSocket messaging is in `gyre-server/src/ws.rs`. Register new routes in `gyre-server/src/api/mod.rs`. This depends on task-102 (audit schema) for the ImpersonationStarted/Ended events.
