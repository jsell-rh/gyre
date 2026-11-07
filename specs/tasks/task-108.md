---
title: "Implement admin user impersonation"
spec_ref: "identity-security.md §Impersonation"
depends_on:
  - task-102
progress: ready-for-review
coverage_sections:
  - "identity-security.md §Impersonation"
commits: ["14af7abd7e27ffcc7aa28e8703cdda7e97abe218", "a1194a81a0b22e891d7b215b6b019b4861d27c6c", "2d511c2169c0c038533d9d2ba240318389555c8c", "cad88393d30797174d3ae3bd3ab90f0d8353f2c6"]
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

## Shipped

Admin user impersonation is implemented end-to-end on commit `cad88393d` (identity-security.md §Impersonation), with one deliberate deviation from the task plan's response shape, documented below. Follow-up `2d511c216` fixed a merge casualty in the test module. This round (`14af7abd7`) repairs two review findings on the previous repair round. Acceptance-criteria status is recorded here rather than by editing the protected checkbox text.

### This round (review repair: exemption remap, approval critical-section serialization)

- **`check-fabricated-scope-defaults.sh` passes again** (finding: the final base merge inserted lines above the pre-existing exempted site — the JWT `claims.tenant_id` fallback in `auth.rs`, already validated by `validate_tenant_id` before use — moving it from line 945 to 983 without re-updating the exemption remap, so the guard failed on the candidate and broke pre-commit/CI). The exemption entry is re-mapped to `auth.rs:983` (edit, not addition; the line count stays frozen at 5). Guard verified green on the fixed tree.
- **The approval session-creation critical section is serialized** (finding: the Pending-status check, the max-1-active-session check, `save_session` and the request-status save were an unlocked read-modify-write, so two concurrent `POST .../approve` calls from the target both loaded the request as Pending and both minted live impersonation tokens for the same admin — two sessions for one consent, two `ImpersonationStarted` events, violating the max-1-active-per-admin rate limit). `SESSION_WRITE_LOCK` (the lock already held by the action counter and `end_session`) is now held across the whole section and released before token minting, audit emission and notification delivery. The sibling read-modify-writes are covered the same way:
  - `create_request`'s rate-limit read + request save now run inside the lock (moved from the handler, which had checked it unlocked), closing the check-to-save gap against a concurrently approving request.
  - `deny_request`'s terminal transition (a concurrent approve/deny pair can no longer both pass the Pending check and overwrite each other's status).
  - The expiry sweep's stale-request marking (cannot overwrite a request a concurrent approve just marked Approved). The sweep's session-ending loop acquires/releases the lock per session, so taking it for the marking loop cannot deadlock.
- New test `concurrent_approvals_create_exactly_one_session` (yielding kv adapter, same construction as the existing atomicity test): two concurrent approvals → exactly one succeeds, the loser gets 409, exactly one session and one `ImpersonationStarted`. **Mutation-verified**: with the approve lock removed, the test fails with `2` successful approvals (evidence in `/tmp/stage/review-evidence/task-108-mutation-probe.txt`); with the fix it passes.

### Previous round (review repair: attribution coverage, approve ordering, atomic counter)

- **Impersonation attribution now covers every authenticated surface** (finding: the `impersonation_action` recording ran only after policy evaluation, so ABAC-exempt routes, unresolved patterns, git HTTP, MCP and WebSocket left impersonated calls unattributed and uncounted):
  - Repair probe through the full production router (`build_router`: `require_auth_middleware` -> `last_seen` -> `abac` -> handler) exposed that `require_auth_middleware` had **no impersonation branch** — the token was 401-rejected before any handler ran, so impersonated `/api/v1/*` calls were impossible in a default production deployment (the test suites layered only `abac_middleware`, which is why they stayed green). Fixed by mirroring `FromRequestParts` step 1.5 in the middleware (EdDSA signature + active session; inactive session -> 401).
  - `abac_middleware` attributes exempt and unresolved routes **before** the passthrough (`record_impersonated_passthrough`: bearer-token peek -> impersonation-JWT crypto pre-filter -> session resolution -> fire-and-forget record with `surface` = `abac-exempt`/`abac-unresolved`); evaluated routes still record after policy evaluation with the decision outcome (`surface` = `abac`).
  - The auth-resolution layer (`AuthenticatedAgent::from_request_parts`) records impersonated extractions with `surface` = `per-handler`, covering the surfaces that authenticate via the extractor without the ABAC middleware: git smart HTTP, MCP, conversations, explorer and ws-ticket issuance; ws-ticket consumption records with `surface` = `ws-ticket`.
  - WebSocket (`/ws`) and agent TTY connections opened with an impersonation token record one action per connection (`surface` = `websocket`, method `WS`).
  - Exactly-once per HTTP request via a request-extensions marker (`ImpersonationActionRecorded`): `abac_middleware` marks evaluated routes before its extraction (it owns the post-evaluation record with the outcome), the exempt/unresolved passthrough marks before the handler extractor runs, and `last_seen_middleware` defers to abac the same way — no double counting across the require_auth -> last_seen -> abac -> handler-extraction chain (asserted: two impersonated calls -> `actions_taken == 2`).
  - Honest limitation, replacing the previous blanket claim: on per-handler surfaces the event's outcome reflects successful *authentication* (the handler's own response is not visible at the auth-resolution layer); evaluated `/api/v1` routes keep the exact ABAC decision outcome. The audit `detail` carries a `surface` field distinguishing all of the above.
- **`approve_request` checks the caller identity first** (finding: status/expiry were revealed before the target check, letting any authenticated user probe request state — a non-target probing an approved request got 409 instead of 403). The target-user check now runs before any status or expiry is revealed, matching `deny_request`'s ordering. Test: `non_target_cannot_probe_request_status`.
- **Atomic `actions_taken`** (finding: the unlocked load -> increment -> save in the fire-and-forget recorder lost counts under concurrency). The session-record read-modify-write (increment in `record_impersonated_action`, end in `end_session`) is now serialized by a session-write lock; this also prevents an in-flight increment from overwriting an `end_session` write and resurrecting an ended session. Test: `concurrent_impersonated_actions_are_counted_atomically` — 8 concurrent recordings through a yielding kv adapter (models a real async backend; the synchronous mem adapter cannot reproduce the interleaving); verified to fail with `actions_taken == 1` when the lock is removed, passes with `8` and strictly sequential per-event counts.

### Verification (focused, this round)

- `./scripts/check-fabricated-scope-defaults.sh` — OK (was FAIL on the candidate before the remap).
- `cargo test -p gyre-server --lib api::impersonation` — 14/14 green (new: `concurrent_approvals_create_exactly_one_session`); `impersonation`-filtered across the crate — 15/15; `jobs::` 6/6; `auth::` 37/37.
- `cargo clippy -p gyre-server --lib` — warning count for `impersonation.rs` identical before/after the change (6 pre-existing, 0 new).
- Mutation probe: removing the approve critical-section lock makes the new test fail (`exactly one approval must succeed: left 2, right 1`); restoring the fix makes it pass.
- Evidence: `/tmp/stage/review-evidence/task-108-fabricated-scope-guard.txt`, `task-108-impersonation-tests.txt`, `task-108-mutation-probe.txt`. Full workspace suites, all-target Clippy and GitHub CI are owned by the verification stage.

### Flow (intent -> signed mandate -> receipt)

- `POST /api/v1/admin/impersonate` (Admin only -- ABAC `admin` resource + in-handler role/user-identity check): creates an approval request. The server generates a one-time 8-digit CSPRNG approval code, stores **only its SHA-256 hash**, and delivers the raw code **only to the target user** -- an `ImpersonationRequest` Inbox notification (priority 2, body carries `request_id`/`approval_code`/`expires_at`) plus a WebSocket `NotificationPush` when the user is online. Request and code are valid 10 minutes. **Deviation from the plan's response shape:** the plan specified `{request_id, approval_code}` in the admin-facing response; that is not shipped because returning the code to the requesting admin would let the admin self-approve, defeating the spec requirement that the *target* provides the code or explicit approval. The response is `{request_id, expires_at}`.
- `POST /api/v1/admin/impersonate/:request_id/approve` (called by the **target user**, any role): per-handler authorization (caller's verified user id must equal the request's target -- checked before any status is revealed), constant-time hash comparison of the code, max-1-active-session rate limit. On success mints a short-lived EdDSA JWT via `AgentSigningKey::mint_impersonation` and persists a session through the `KvJsonStore` port (namespaces `impersonation_requests` / `impersonation_sessions` -- restart-survivable, no in-memory store).
- `POST /api/v1/admin/impersonate/:request_id/deny`: target explicitly rejects; a denied request can no longer be approved.
- `DELETE /api/v1/admin/impersonate/:request_id` (only the impersonating admin): ends the session -> the token is rejected immediately (session-record revocation before `exp`) and `ImpersonationEnded` is emitted.
- Approve/deny are ABAC-exempt routes with per-handler authorization because the caller is the target user who may hold any role (a ReadOnly target must still be able to consent); request/end are ABAC `admin`-resource routes with in-handler defense in depth. All four routes are registered in the `ResourceResolver`.

### Impersonation token

EdDSA JWT with `sub` = target user (permissions match), `impersonator_id`/`target_user_id`/`session_id`/`impersonating: true` claims. TTL from `GYRE_IMPERSONATION_TTL` (default 900 s), validated with **zero leeway** on `exp` (self-minted, same clock). The auth chain (`require_auth_middleware`, `FromRequestParts` and the WS `authenticate_token` path) resolves it to the target's identity with an `ImpersonationContext` attached; a missing/ended/expired session record rejects the token.

### Audit trail

- `AuditEventType::ImpersonationStarted` / `ImpersonationEnded` variants (gyre-domain) with the spec `detail` fields: `impersonator_id`, `target_user_id`, `approval_token` (the consumed code's hash -- the raw single-use code is never persisted) and `duration_secs`, `actions_taken` (+ `reason`). Both events are attributed (`user_id`) to the impersonator.
- Every authenticated call made under an impersonation token is recorded as an `impersonation_action` audit event attributed to the impersonator and counted in the session's `actions_taken` — evaluated `/api/v1` routes via `abac_middleware` after policy evaluation (with the decision outcome), exempt/unresolved `/api/v1` routes via `abac_middleware` before the passthrough, per-handler surfaces (git HTTP, MCP, conversations, explorer, ws-ticket) at the auth-resolution layer, and WS/TTY connections at connection time; see "This round" for the exactly-once marker design. Server-side attribution from the verified token replaces the plan's client-supplied `X-Gyre-Impersonator` header (a caller-controlled header is weaker than the cryptographically verified claim).
- `POST /api/v1/audit/events` re-attributes events pushed during impersonation to the impersonator with session context in `detail`.

### Notifications

- `NotificationType::ImpersonationRequest` (priority 2) and `ImpersonationAlert` (priority 1, "Admin {name} is impersonating your account") variants.
- `WsMessage::NotificationPush` variant; `AppState.ws_connection_users` maps connection id to server-verified user id so pushes are matched by verified identity, never client-supplied ids.
- Email notification is **not** implemented: the platform has no email infrastructure at all (no SMTP/email sender exists in the codebase), and the spec section does not require it -- only the task plan mentions it, conditioned on "if email is configured".

### Authorization constraints

- Only `Admin` role with a real user identity (the shared system token is rejected) may request/end; non-admins get 403.
- Impersonating another Admin is prohibited -- `UserRole` has no SuperAdmin variant, so the plan's "requires SuperAdmin **or is prohibited**" resolves to prohibited.
- Max one active session per admin (409); no requesting/approving/denying while impersonated (prevents privilege chaining); cannot impersonate yourself.
- `impersonation_expiry` background job (30 s interval, registered in the job registry and spawned from main) ends expired sessions and marks stale pending requests; JWT expiry is enforced at validation time regardless of the sweep.
