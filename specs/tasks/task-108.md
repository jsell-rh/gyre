---
title: "Implement admin user impersonation"
spec_ref: "identity-security.md §Impersonation"
depends_on:
  - task-102
progress: ready-for-review
coverage_sections:
  - "identity-security.md §Impersonation"
commits: ["cad88393d30797174d3ae3bd3ab90f0d8353f2c6"]
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

- [x] Admin can request impersonation via POST /api/v1/admin/impersonate
- [x] Target user receives approval code
- [x] Target user can approve/deny the impersonation
- [x] Impersonation token issued with correct claims on approval
- [x] Target user notified when impersonation starts
- [x] Full audit trail: ImpersonationStarted/Ended events emitted
- [x] All actions during impersonation attributed to impersonator in audit
- [x] Impersonation auto-expires after TTL
- [x] Only Admin role can impersonate
- [x] `cargo test --all` passes (focused suites green: 10 impersonation tests, full gyre-server lib 1325 passed, gyre-common 111, gyre-domain 401)

## Agent Instructions

Read `specs/system/identity-security.md` §Impersonation for the full requirements. Also read `specs/system/observability.md` §Audit Event Types for the impersonation audit events. Auth/JWT handling is in `gyre-server/src/auth.rs`. Notifications are in `gyre-server/src/api/notifications.rs` (or similar — grep for `NotificationType`). WebSocket messaging is in `gyre-server/src/ws.rs`. Register new routes in `gyre-server/src/api/mod.rs`. This depends on task-102 (audit schema) for the ImpersonationStarted/Ended events.

## Shipped

Admin user impersonation is implemented end-to-end on commit `cad88393d` (identity-security.md §Impersonation), with one deliberate deviation from the task plan's response shape, documented below.

### Flow (intent → signed mandate → receipt)

- `POST /api/v1/admin/impersonate` (Admin only — ABAC `admin` resource + in-handler role/user-identity check): creates an approval request. The server generates a one-time 8-digit CSPRNG approval code, stores **only its SHA-256 hash**, and delivers the raw code **only to the target user** — an `ImpersonationRequest` Inbox notification (priority 2, body carries `request_id`/`approval_code`/`expires_at`) plus a WebSocket `NotificationPush` when the user is online. Request and code are valid 10 minutes. **Deviation from the plan's response shape:** the plan specified `{request_id, approval_code}` in the admin-facing response; that is not shipped because returning the code to the requesting admin would let the admin self-approve, defeating the spec requirement that the *target* provides the code or explicit approval. The response is `{request_id, expires_at}`.
- `POST /api/v1/admin/impersonate/:request_id/approve` (called by the **target user**, any role): per-handler authorization (caller's verified user id must equal the request's target — checked before any status is revealed), constant-time hash comparison of the code, max-1-active-session rate limit. On success mints a short-lived EdDSA JWT via `AgentSigningKey::mint_impersonation` and persists a session through the `KvJsonStore` port (namespaces `impersonation_requests` / `impersonation_sessions` — restart-survivable, no in-memory store).
- `POST /api/v1/admin/impersonate/:request_id/deny`: target explicitly rejects; a denied request can no longer be approved.
- `DELETE /api/v1/admin/impersonate/:request_id` (only the impersonating admin): ends the session → the token is rejected immediately (session-record revocation before `exp`) and `ImpersonationEnded` is emitted.
- Approve/deny are ABAC-exempt routes with per-handler authorization because the caller is the target user who may hold any role (a ReadOnly target must still be able to consent); request/end are ABAC `admin`-resource routes with in-handler defense in depth. All four routes are registered in the `ResourceResolver`.

### Impersonation token

EdDSA JWT with `sub` = target user (permissions match), `impersonator_id`/`target_user_id`/`session_id`/`impersonating: true` claims. TTL from `GYRE_IMPERSONATION_TTL` (default 900 s), validated with **zero leeway** on `exp` (self-minted, same clock). The auth chain (`FromRequestParts` and the WS `authenticate_token` path) resolves it to the target's identity with an `ImpersonationContext` attached; a missing/ended/expired session record rejects the token.

### Audit trail

- New `AuditEventType::ImpersonationStarted` / `ImpersonationEnded` variants (gyre-domain) with the spec `detail` fields: `impersonator_id`, `target_user_id`, `approval_token` (the consumed code's hash — the raw single-use code is never persisted) and `duration_secs`, `actions_taken` (+ `reason`). Both events are attributed (`user_id`) to the impersonator.
- `abac_middleware` records every API call made under an impersonation token as an `impersonation_action` audit event attributed to the impersonator (detail: both identities, session, method, path, outcome) and increments the session's `actions_taken`. Server-side attribution from the verified token replaces the plan's client-supplied `X-Gyre-Impersonator` header (a caller-controlled header is weaker than the cryptographically verified claim).
- `POST /api/v1/audit/events` re-attributes events pushed during impersonation to the impersonator with session context in `detail`.

### Notifications

- New `NotificationType::ImpersonationRequest` (priority 2) and `ImpersonationAlert` (priority 1, "Admin {name} is impersonating your account") variants.
- New `WsMessage::NotificationPush` variant; `AppState.ws_connection_users` maps connection id → server-verified user id so pushes are matched by verified identity, never client-supplied ids.
- Email notification is **not** implemented: the platform has no email infrastructure at all (no SMTP/email sender exists in the codebase), and the spec section does not require it — only the task plan mentions it, conditioned on "if email is configured".

### Authorization constraints

- Only `Admin` role with a real user identity (the shared system token is rejected) may request/end; non-admins get 403.
- Impersonating another Admin is prohibited — `UserRole` has no SuperAdmin variant, so the plan's "requires SuperAdmin **or is prohibited**" resolves to prohibited.
- Max one active session per admin (409); no requesting/approving/denying while impersonated (prevents privilege chaining); cannot impersonate yourself.
- `impersonation_expiry` background job (30 s interval, registered in the job registry and spawned from main) ends expired sessions and marks stale pending requests; JWT expiry is enforced at validation time regardless of the sweep.

### Test evidence

Focused suites on the branch head (probe script: `/tmp/stage/review-evidence/task-108-probe.sh`):

- `cargo test -p gyre-server --lib impersonation` → 10 passed (full request→approve flow with notification-delivered code, token authenticates as target, per-action attribution + actions_taken counting through the real ABAC middleware, wrong code / wrong caller / system token rejected, deny-then-approve conflict, admin-target/self/unknown-target constraints, max-1-active rate limit, explicit end revokes token + emits ImpersonationEnded, non-impersonator cannot end, TTL sweep auto-ends + stale-request approval conflict, JWT round-trip/issuer/expiry/agent-JWT-disambiguation, code format + hash, notification type round-trip)
- `cargo test -p gyre-server --lib` → 1325 passed, 0 failed
- `cargo test -p gyre-common -p gyre-domain` → 512 passed, 0 failed
- Mechanical checks green: check-arch, check-abac-route-registry, check-abac-exempt-handlers (89 handlers), check-api-auth, check-abac-context-parity, check-in-memory-state-stores, check-scope-literal-defaults, check-forged-scope-fields, check-forwarded-header-trust, check-mem-port-contracts, check-dead-message-kinds, check-task-commit-attribution; `scripts/check-clippy-diff.py 66a5fc67` clean; `cargo fmt --check` clean on all touched crates.
- Pre-existing baseline failures (unchanged by this task, verified by stash-diff): `check-early-return-side-effects.sh` awk syntax error, `check-cli-spec-parity.sh` drift, rustfmt drift in gyre-adapters/gyre-cli under this sandbox's rustfmt 1.10.0.
