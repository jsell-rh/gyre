# Review — task-124 (Complete user management and workspace membership API endpoints)

Spec: `user-management.md` §API > User Management (row 26), §API > Workspace Membership (row 27).
Candidate: `8930cd81` (base `cdd1dd02`). Product-surface commits: `33e30ac9`, `6033324f`, `60c86252`, `38b3134f`, `33508170` — all recorded in the task's `commits:` frontmatter at HEAD (attribution check passes in a clean worktree; the review-stage checkout resets the task file, which makes the script fail only against the dirty working tree).
Verdict: **approved**.

## What was verified

**§API > User Management — 5 of 8 endpoints implemented for real (row 26 stays task-assigned, honestly):**

- `GET /api/v1/users/{username}` (`get_user_by_username`): real lookup via `find_by_username` + memberships via `list_by_user`; tenant containment returns 404 (not 403) for cross-tenant handles, so foreign-tenant existence does not leak; system principal exempt.
- `GET /api/v1/users` (`list_users`): per-handler TenantAdmin gate (`is_tenant_admin`: system principal, Admin role, or stored `GlobalRole::TenantAdmin` — the stored record is authoritative over JWT claims, same rationale as `can_manage_membership`); RBAC denial recorded; tenant-scoped retain; `limit`/`offset` pagination capped at 200 with a post-filter `total`.
- `PUT /api/v1/users/{id}/deactivate` (`deactivate_user`): TenantAdmin-only, same-tenant targets only, self-deactivation → 409, idempotent stamp. The cascade is real, not cosmetic: migration `2026-11-13-000063_user_deactivated_at` adds `users.deactivated_at` (plain portable `ALTER TABLE`, next unused sequence number 000063); the auth extractor's API-key path, `authenticate_token` (WebSocket), and `find_or_create_user` (SSO/JWT — reached via `validate_jwt`) all reject deactivated accounts, so existing keys and IdP sessions die immediately; `end_active_sessions_for_user` ends impersonation sessions the user participates in (either side) AND `resolve_impersonation_auth` independently re-checks both accounts' deactivation on every call (belt-and-suspenders, proven by a dedicated test that bypasses the endpoint cascade); `workspace_ownership::on_user_removed` + `has_active_owner` (now counting only non-deactivated owners) record the ownerless transition and notify the tenant's TenantAdmins inline; audit-logged as `user_deactivated` with reason.
- `GET/PUT /users/me` pre-existing (task-120) and return/accept the spec'd profile fields including preferences.

**§API > Workspace Membership — all 7 endpoints live (row 27 implemented):**

- `POST /workspaces/{id}/invite` — spec-named alias of the member invite (same handler/authorization): creates a PENDING membership + `InvitationReceived` notification, rejects cross-tenant invitees and duplicates (409).
- `GET .../invitations` — pending-only (unaccepted memberships), Owner/Admin/TenantAdmin via `can_manage_membership`, tenant-scoped via `load_workspace_scoped`.
- `POST .../invite/accept` — ABAC-exempt with real per-handler auth: workspace tenant scope enforced first, then the membership is looked up by (caller, workspace), so nobody else's invitation can be accepted; stamps `accepted_at`; 404 when not invited, 409 when already accepted; audit-logged.
- `DELETE .../invitations/{invitation_id}` — pending-only revocation (accepted → 409 pointing at member removal), workspace-membership-of-the-invitation enforced (foreign workspace id → 404), deletes the membership row, audit-logged.
- `GET .../members`, `PUT/DELETE .../members/{user_id}`(+`/role`) pre-existing, now with last-Owner guards that count only *active* (non-deactivated) Owners, audited ownership transfer (`workspace_ownership_transferred` with previous owner + reason), and ownerless-tracking cleared on Owner reassignment.

**Route/ABAC registration:** all new routes registered in `api/mod.rs`; 5 `RouteResourceMapping::api` entries + 1 `exempt` (accept, per-handler auth as required) in `abac_middleware.rs`. `check-abac-route-registry.sh` and `check-abac-exempt-handlers.sh` pass.

## Probes (full detail + raw output in /tmp/stage/review-evidence)

- All 19 repo invariant scripts pass at the candidate (attribution verified in a clean worktree at HEAD; see note above).
- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` → rc=0.
- `cargo test -p gyre-domain` → 511 passed (incl. `deactivate_stamps_once_and_reports_state`).
- `cargo test -p gyre-adapters sqlite::user` → 17 passed (incl. `deactivation_persists_and_round_trips` against real SQLite; every run applies all migrations, proving 000063 applies cleanly).
- Standalone matchit 0.7.3 probe with all 53 `/users*` + `/workspaces*` route shapes from `api/mod.rs`: 0 insert failures, `/users/me` keeps static priority over `/users/:username`, no static route shadowed — the router cannot panic at construction (a runtime risk `cargo check` cannot cover).
- Test quality: the new tests drive the real `api_router()` with real API-key auth extraction via `oneshot`, assert pre-conditions (the key/token authenticates *before* the mutation) so the later 401/403 assertions are meaningful, and cover the negative paths (Developer → 403, cross-tenant → 404/403, self-deactivation → 409, foreign invitation id → 404, re-accept → 409). Not self-confirming.

## Deferred to the verifier (gyre-server test compilation excluded from this stage by the build wrapper)

- `cargo test -p gyre-server --lib api::users`
- `cargo test -p gyre-server --lib api::impersonation`
- `cargo test --all` (task acceptance criterion)

## Non-blocking observations

- The three session endpoints (`GET /users/me/sessions`, `DELETE .../sessions/{id}`, `POST .../sessions/revoke-all`) are not implemented; they are owned by task-111 (not-started), the shipped notes and coverage row 26 state this explicitly, and the row stays task-assigned. Correct honest handling per the no-fake-completion rule; task-124's own acceptance criteria do not include them.
- `PUT /users/{id}/deactivate` requires a JSON body ( `{}` suffices) — a bodyless PUT gets a 4xx from the extractor. Documented in api-reference.md.
- `list_users` loads all users then retains the caller's tenant in memory — fine at current scale, not a correctness issue.
