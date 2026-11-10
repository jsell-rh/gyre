# Review: task-111 — Implement session management

**Candidate:** `0232c41848673d9315120881b5a9cef782107491` (branch head; merge `763389787` onto base `cd752300c8926171e0ab87a38c0fc65e5ecb891b`)
**Spec:** `user-management.md` §Session Management
**Verdict:** **Approved** — independent evidence supports the task contract; no findings.

## What was reviewed

The full diff from base `cd752300c` was inspected with focus on the session-management
surface (domain entity, port, adapters, migration, auth integration, API handlers,
background cleanup) and the merge-conflict resolutions/fallout the candidate shipped.

## Acceptance criteria — evidence

| Criterion | Evidence |
|---|---|
| `UserSession` domain entity | `gyre-domain/src/user_profile.rs` — all nine spec fields; `is_active(now)` = not revoked ∧ not expired |
| `SessionRepository` port + adapters | `gyre-ports/src/user_profile.rs`; SQLite (`gyre-adapters/src/sqlite/user_profile.rs`), PostgreSQL, and mem (`gyre-server/src/mem.rs`) implementations; duplicate-id `create` fails in both SQLite (PK) and mem (explicit guard, `check-mem-port-contracts.sh` PASS) |
| DB migration | `2026-10-08-000059_user_sessions` (unique sequence 000059; `check-migration-versions.sh` PASS; portable SQL — `check-migration-sql-portability.sh` PASS) |
| Sessions created on auth, throttled `last_active_at` | `auth.rs` `track_session` — find-or-mint per (credential, IP, User-Agent); touch gated on `SESSION_TOUCH_THROTTLE_SECS = 60`; sliding `expires_at`; expired-but-unrevoked rows re-minted. Killing tests `session_last_active_touch_is_throttled`, `expired_session_is_reminted_not_zombified`, `session_touch_slides_expiry` |
| GET lists active sessions | `list_my_sessions` filters `is_active(now)`; never exposes `token_hash`; test `sessions_endpoint_lists_session_created_by_auth` asserts both |
| DELETE revokes individual session | `revoke_my_session` — owner-scoped, foreign id → 404 (`revoke_cannot_target_another_users_session`) |
| POST revoke-all | `revoke_all_my_sessions` → `revoke_all_for_user`; credential rejected afterwards on **never-seen devices** too (`credential_revoked`; tests `revoke_all_sessions_signs_out_everywhere`, `revoke_all_blocks_credential_from_never_seen_device`) |
| Revoked sessions rejected in auth middleware | Enforced on **both** the HTTP extractor and the WS/TTY `authenticate_token` path; per-device revoke ≠ sign-out-everywhere (`revoking_one_device_session_does_not_sign_out_other_devices`) |
| Routes + ABAC | All six routes registered in `api/mod.rs` as `RouteResourceMapping::exempt(...)` with **per-handler** authorization: `/me` variants scope to caller; `:user_id` variants require Admin role **and** tenant containment (`ensure_same_tenant_session_target`, fail-closed on missing tenant binding; unknown target user → 404). `check-abac-route-registry.sh` and `check-abac-exempt-handlers.sh` PASS. Cross-tenant killing test: `cross_tenant_admin_cannot_view_or_revoke_another_tenants_user_sessions` (real two-tenant setup) |
| Tests pass | `gyre-server --lib` **1619/0**, `gyre-adapters --lib` **417/0**, `gyre-domain` **443/0** (this reviewer's runs, `SKIP_WEB_BUILD=1`, logs in `/tmp/stage/review-evidence/task-111-*.log`) |

Background cleanup (`spawn_session_cleanup`, daily) deletes only unrevoked rows expired
>30 days — revoked rows are the durable sign-out record (`session_retention_never_deletes_revoked_rows`,
`signout_everywhere_survives_retention_cleanup`).

## Mutation probe (killing-test authenticity)

Both enforcement paths in `auth.rs` were temporarily disabled (revoked-device branch
returns `true`; `credential_revoked` returns `false`). Result: **all 6 targeted killing
tests FAILED** (`revoke_session_then_credential_is_rejected`, `revoke_all_sessions_signs_out_everywhere`,
`revoke_all_blocks_credential_from_never_seen_device`, `ws_auth_rejects_revoked_ws_device_session`,
`ws_auth_rejects_signed_out_everywhere_credential`, `signout_everywhere_survives_retention_cleanup`).
Source was restored and verified byte-identical (sha256 match, clean `git status`).
Log: `/tmp/stage/review-evidence/task-111-mutation-probe.log`.

The endpoint tests drive the real `api_router()` + real auth extractor with real
provisioned API keys — not mirrored logic.

## Mechanical checks (all PASS, this reviewer's runs)

`check-arch`, `check-abac-route-registry`, `check-abac-exempt-handlers`,
`check-migration-versions`, `check-mem-port-contracts`, `check-scope-literal-defaults`,
`check-fabricated-scope-defaults`, `check-inert-enforcement`,
`check-forwarded-header-trust`, `check-migration-sql-portability`.

Note: the candidate *lowered* `scope-literal-defaults` exemptions 12→9 by actually
fixing the three `merge_request.rs` scope-fabrication sites (allowed direction), and
fixed a vacuous no-arg invocation bug in that check script itself.

## Design notes (accepted, not defects)

- **Session token = API key.** The platform has no separate login-minted session
  token; auth is API-key based. `token_hash` stores `hash_api_key(key)` (SHA-256,
  plaintext never persisted), and a session is one (credential, IP, User-Agent)
  row. This realizes the spec's user-facing contract (per-device view/revoke,
  sign-out-everywhere, TenantAdmin oversight) and was accepted in prior review
  rounds of this branch.
- **Fail-soft on session-store errors** in `track_session`/`credential_revoked`
  (log + allow): documented availability trade-off — the credential itself was
  already validated against `api_keys`; revocation state is unavailable, not "no".
- **IP from `ConnectInfo` socket peer only**, no forwarded-header trust (CWE-348).
- WS path tracks sessions with empty device attributes (no request headers exist
  there); same revocation rules apply.

## Merge-resolution check

The four content conflicts (`api/mod.rs`, `api/spawn.rs`, `auth.rs` ×2 paths,
coverage header) and the auto-merge fallout repairs (`MergeRequest::new` 7-arg
call sites, `rbac.rs` repo seeding) were spot-checked; full crate suites pass at
head, and no base-side fix was dropped (session enforcement and task-104 audit
dedupe coexist on the success path; rejection path audits `AuthFailure`).
