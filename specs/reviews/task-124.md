# Review — task-124 (User management + workspace membership API endpoints)

Spec: `specs/system/user-management.md` §API > User Management, §API > Workspace Membership.
Candidate under review: `3df7f325` (merge of the task-124 branch onto base `349a32ac`; product commits `33e30ac9` endpoints, `6033324f` impersonation-deactivation repair; rebase repair `c2af71ee`).
Verdict: **approved** — no findings. Server test execution deferred to the independent verifier (commands recorded below and in `/tmp/stage/review-evidence/README.md`); everything verifiable in the review sandbox was verified independently.

## Probes run (evidence in /tmp/stage/review-evidence/)

- `cargo check -p gyre-server --tests` → exit 0 (6m01s). Both the task-124 and the rebased task-122 test sets in `api/users.rs` type-check together.
- `cargo test -p gyre-domain --lib user` → 12 passed (incl. `deactivate_stamps_once_and_reports_state`: idempotent stamp keeps the first deactivation time).
- `cargo test -p gyre-adapters --lib deactivation` → `sqlite::user::tests::deactivation_persists_and_round_trips` passes against real SQLite: `find_by_id`, `find_by_external_id`, `find_by_username`, and `list` all see `deactivated_at` after `update()`.
- `cargo test -p gyre-adapters --lib migration` → 9 passed, incl. `migrations_pending_zero_after_startup` (migration `2026-11-13-000063_user_deactivated_at` actually runs at startup) and `no_duplicate_migration_version_prefixes`.
- Mechanical gates, all exit 0: `check-migration-versions.sh`, `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh` (76 handlers), `check-arch.sh`, `check-mem-port-contracts.sh`, `check-task-commit-attribution.sh`, `check-migration-sql-portability.sh`, `check-forged-scope-fields.sh`, `check-inert-enforcement.sh`, `check-scope-literal-defaults.sh`, `check-in-memory-state-stores.sh`, `check-api-auth.sh`.
- `scripts/update-coverage-summary.sh` regenerates `specs/coverage/SUMMARY.md` **byte-identical** to the committed file (counts are faithful, not hand-written).
- Router-conflict risk eliminated by source inspection of matchit 0.7.3 (axum 0.7.9): `normalize_params` + `param_remapping` mean `/api/v1/users/:username`, `/api/v1/users/:id/deactivate`, and the static `/api/v1/users/me` coexist without insert conflicts; static segments win. No `api_router()` panic risk.

## What was verified

**Acceptance criteria (all five implementable ones):**

- **`GET /api/v1/users/{username}`** — real handler over the user + membership stores; tenant containment returns 404 (not 403) for cross-tenant handles, so existence doesn't leak; system principal exempt; memberships included per the spec's tenant-public profile surface. ABAC-mapped to the `user` resource (read), which the builtin Developer/ReadOnly policies allow — the endpoint is genuinely tenant-public, not just unauthenticated-open.
- **`GET /api/v1/users`** — TenantAdmin-only via `is_tenant_admin` (stored global_role authoritative; fail-closed on lookup error), scoped to the caller's tenant, `limit/offset` pagination capped at 200, `total` counts the filtered set, `deactivated_at` exposed. RBAC denial recorded before the 403.
- **`PUT /api/v1/users/{id}/deactivate`** — a real cascade, not a flag flip: new `users.deactivated_at` column (portable migration 000063, round-trip adapter-tested); auth extractor API-key path, WS `authenticate_token`, and SSO `find_or_create_user` all reject deactivated accounts — and since `validate_jwt` (Keycloak path) resolves the user through `find_or_create_user`, that path is covered too; active impersonation sessions ended inline AND `resolve_impersonation_auth` independently re-checks both accounts' deactivation on every call (the belt-and-suspenders is real: the third regression test deactivates the record directly, bypassing the cascade, and the resolver alone rejects the unexpired token); `has_active_owner`/`count_active_owners_excluding` treat a deactivated Owner as departed, and `on_user_removed` records the ownerless transition with inline TenantAdmin notification; audit-logged (`user_deactivated`, reason); idempotent; self-deactivation → 409; cross-tenant target → 403.
- **Invitation endpoints** — `POST /invite` (alias of the audited M22.8 invite, creates a PENDING membership + `InvitationReceived`), `GET /invitations` (pending-only, Owner/Admin/TenantAdmin), `POST /invite/accept` (ABAC-exempt with genuine per-handler auth: membership looked up by (caller, workspace) so nobody else's invitation can be accepted; `load_workspace_scoped` enforces tenant containment; 404/409 semantics correct), `DELETE /invitations/{id}` (pending-only, workspace-id containment 404, accepted → 409). The lifecycle rides real membership storage (`accepted`/`accepted_at`), not a parallel table.
- **Role change / ownership transfer** — pre-existing handler audited: last-active-Owner protection (now correctly ignoring deactivated Owners), audited `workspace_ownership_transferred`, ownerless record cleared on Owner assignment.

**Route/ABAC registration** — all new routes in `api/mod.rs` with `RouteResourceMapping` entries; `check-abac-route-registry.sh` and `check-abac-exempt-handlers.sh` pass; no exemption-file growth.

**Coverage honesty** — row 27 (§API > Workspace Membership) marked implemented with all seven endpoints live; row 26 stays task-assigned with the three session endpoints explicitly open and correctly attributed to task-111 (the earlier false "covered by task-111" claim was already corrected in round 2 and stays corrected). SUMMARY.md regeneration is byte-faithful.

**Test quality** — no test inflation found. The deactivate test proves its pre-condition (the target's key authenticates 200) before asserting the post-condition (403), so it fails if the extractor check is removed. The impersonation resolver test isolates the resolver check from the cascade. The invitation lifecycle test drives the real router end-to-end including the negative paths (foreign acceptor 404, re-accept 409, revoke-after-accept 409, revoke-gone 404). Fixture assumptions hand-verified against `mem::test_state` (empty stores, per-test seeding — the `total == 3` assertion is sound).

**Merge/rebase repair** — migration renumber 000062→000063 was necessary and correct (base's team migration owns Diesel version 20261113000062); no code referenced the old version; both migration suites pass. No conflict markers survive (full test-profile type-check).

## Deferred to the independent verifier

gyre-server test codegen exceeds the review sandbox's 4 GiB budget (wrapper blocks it by design). The verifier MUST run:

    cargo test -p gyre-server --lib api::users
    cargo test -p gyre-server --lib api::impersonation
    cargo test --all

No predicted failures: the suites type-check and every fixture assumption was hand-verified.

## Minor observations (non-blocking, recorded for completeness)

- `specs/coverage/system/user-management.md` row 26's note still cites "migration 000062 `users.deactivated_at`" — stale after the rebase renumber to 000063 (000062 is the team migration). Doc-only drift inside one note; task file, api-reference, and the migration dir itself are correct.
- The task's Shipped section once describes `GET /invitations` as "member-visible" while the handler (and the coverage row) gate it on Owner/Admin/TenantAdmin — the stricter gate matches the spec ("visible in workspace settings and admin panel"); only the bullet's phrasing is loose.
- `web/dist/` untouched (no web source changes) — generated output owned by the verifier; nothing to observe.
