# Review — task-110: Implement tenant & workspace invitation flow

**Candidate:** `da33e7f42b0cde8748a89be93b44621e68dbfcc0` (base `77473763887f249f71d6402d4fc712d51bd247c5`)
**Round:** 9 (review of the round-8 rebase candidate)
**Verdict:** ❌ Not approved — 3 blocking findings (reproduced with probe tests), 3 non-blocking
**Evidence:** `/tmp/stage/review-evidence/task-110-round9-findings.md`, `task-110-round9-probe.rs`, `task-110-round9-probe-run.log`, guard `.out` files, `test-invitations.log`

## What was verified sound

- **Rebase repair (round 8's work):** `cargo test -p gyre-server --lib auth` 69/0 (the auth.rs conflict resolution is sound; the three F2 email-link tests pass), `cargo test -p gyre-adapters --lib migration` 8/0 including `no_duplicate_migration_version_prefixes` and `migrations_pending_zero_after_startup` — the renumbered `2026-11-08-000060_user_tenant_scope` migration runs and does not collide with upstream's `2026-10-09-000057_agent_persona_id`.
- **Mechanical guards** at the candidate head, all exit 0: check-migration-versions, check-arch, check-abac-route-registry, check-fabricated-scope-defaults, check-scope-literal-defaults, check-migration-sql-portability, check-mem-port-contracts, check-forged-scope-fields, check-inert-enforcement, check-lossy-secret-conversion, check-task-commit-attribution.
- **Existing suite:** `cargo test -p gyre-server --lib api::invitations` 20/0.
- **Token handling:** ring `SystemRandom`, 32 bytes, hex; only SHA-256 hash stored; unique index on `token_hash`; list endpoints do not return tokens.
- **Race safety:** conditional `transition_status` inside the write in all three adapters; accept claims Pending→Accepted before side effects; decline/revoke/expiry share the transition; failure-after-claim compensates by rolling back to Pending.
- **Authorization:** tenant-admin guard on tenant endpoints, tenant containment + Owner/Admin guard on workspace endpoints, invitee-identity guard on magic-link routes with a Bearer; non-admin/non-member 403s negative-tested through the real extractor; no caller-supplied tenant_id/workspace_id is trusted.
- **Adapters (sub-agent review + spot checks):** port contract parity across SQLite/Postgres/mem; migrations portable across both dialects; `claim_external_id` cannot silently overwrite another user's external_id (unique-index backstop).

## Blocking findings (reproduced — probe tests in evidence)

1. **Accept does not link an existing SSO account.** `accept_tenant_invitation_claimed` looks up only the `tenant:{t}:email:{e}` namespace. An invitee who already has an SSO-provisioned account (the normal state in SSO-mode tenants — users auto-provision on first login) accepts — even authenticated with matching verified email — and a **parallel local-mode account** is created that receives the pre-assigned workspace memberships and invited GlobalRole; the real user's account gets nothing (the auth.rs email-link claim never fires because the SSO subject resolves by external_id first), and the response misreports `mode: "created"`. Spec §Tenant-Level User Onboarding flow step 3 ("SSO mode: redirected to Keycloak login, then account linked") and the task plan ("create user (local mode) or link account (SSO)") require the link. Probe: 200 OK, new user got the ws-1 membership, SSO user got none.

2. **Username collision permanently breaks acceptance (500).** Username is the raw email local-part (or `req.username` verbatim) with no `sanitize_username`/`resolve_unique_username`/`validate_username`, unlike the auth.rs provisioning path; username is globally unique (`idx_users_username`). Same local-part across domains, or the same person invited in two tenants (which the spec's cross-tenant model explicitly requires — "users must have accounts in both tenants"), makes the second accept return **500 Internal Server Error** with the invitation rolled back to Pending — permanently un-acceptable unless the invitee guesses the undocumented `username` body field.

3. **Handle contract bypass.** `AcceptInviteRequest.username` is stored verbatim: `{"username": "Carol Smith!!"}` is accepted with 200 and persisted. The URL-safe handle contract (`validate_username`, 1–64 chars `[a-z0-9_-]`, user-management.md §Username vs Display Name) is enforced on the SSO and SCIM creation paths but not here.

## Non-blocking findings

- `GYRE_INVITE_EXPIRY_INTERVAL_SECS=0` panics the server at startup (`tokio::time::interval` with zero duration).
- `load_policy` fails open on a corrupt stored policy row (permissive defaults restored silently).
- `max_pending_invitations` is read-then-write with no DB backstop (concurrent invites can overshoot, bounded by concurrency).
- Test gaps: zero cross-tenant negative tests (deleting any "different tenant" 403 guard would not fail the suite); `max_pending_invitations` enforcement and workspace-invitation expiry rejection untested.
- mem adapters' invitation `create` overwrites same-id rows; `json_to_ids` swallows corrupt JSON to an empty `workspace_ids`.

## Scope notes (not task-110 defects)

- Local-mode accounts have no credential/session mechanism yet — Session Management is task-111's assigned section (specs/coverage/system/user-management.md row 12).
- Workspace Admin granting Owner role via workspace invite is unrestricted, but §Workspace Invitation Flow does not restrict grantable roles and §Ownership Transfer semantics are task-121's scope.

## Required for approval

Fix findings 1–3 in `accept_tenant_invitation_claimed` (link the existing SSO account when the invitee is reachable by verified email or authenticated identity; derive/sanitize the username with uniqueness resolution like auth.rs; validate `req.username` against the handle contract, returning a client-actionable 409/400 on collision), with tests covering: existing-SSO-user accept links and receives the memberships; cross-tenant same-email accept succeeds; invalid username rejected.
