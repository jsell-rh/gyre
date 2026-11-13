# Review — task-124 (Complete user management and workspace membership API endpoints)

Spec: `specs/system/user-management.md` §API > User Management + §API > Workspace Membership.
Candidate under review: `600d3e169c74d982cad5fd1d3bb262e03d7b1375` (merge of verified
implementation `5687f0f6` with main `f7f5c59f`, resolving the DIRTY rebase finding
`cfc761db6aee4984a962a090c116a6eb`).
Verdict: **needs-revision** — one blocking finding (R1: unresolved source conflict in
test code; `cargo test --all` fails to compile on the candidate). The implementation
itself (retained from the previously verified `5687f0f6`) and the merge resolution of
all *non-test* code verified clean.

## Round: rebase-reconciliation review of `600d3e16`

### What was reviewed

- Full base→candidate diff (`f7f5c59f..600d3e16`, 17 files, +1682/−34): new endpoints
  `GET /api/v1/users/{username}`, `GET /api/v1/users`, `PUT /api/v1/users/{id}/deactivate`,
  `POST /api/v1/workspaces/{id}/invite` (alias), `GET .../invitations`,
  `POST .../invite/accept`, `DELETE .../invitations/{invitation_id}`; migration
  `2026-11-13-000063_user_deactivated_at`; deactivation cascade across auth.rs
  (API-key extractor, WS `authenticate_token`, SSO `find_or_create_user`),
  impersonation.rs (session ending + resolver account-state re-check),
  workspace_ownership.rs (deactivated Owner ≠ active Owner); domain/adapters
  `deactivated_at` persistence; ABAC resolver entries; docs/coverage updates.
- Merge soundness: every hunk added by each side (task-124 `7daca25..5687f0f6` vs
  task-125 `7daca25..f7f5c59f`) in all seven doubly-changed files was checked for
  survival in the merged tree (evidence: `review-evidence/merge-hunk-survival.txt`).
  All code content from both sides survives; the only non-surviving lines are
  superseded narrative/header lines in `specs/coverage/system/user-management.md`
  and import-list lines subsumed by the union import in `api/mod.rs` (all 25 handler
  names from both sides verified present).
- Router-construction risk (`/api/v1/users/:username` vs `/api/v1/users/:id/deactivate`,
  different param names at one position): resolved against the actual dependency —
  matchit 0.7.3 `normalize_params` canonicalizes param names at insert, so no
  conflict/panic; consistent with the earlier verified full-suite runs.

### Probes run (evidence under /tmp/stage/review-evidence)

- `cargo check -p gyre-server` → **Finished, 0 errors** (13m15s;
  `cargo-check-gyre-server.log`) — the merged production code type-checks.
- `cargo check -p gyre-server --tests` → **error[E0061]** at
  `crates/gyre-server/src/api/users.rs:4102` (`cargo-check-gyre-server-tests.log`) —
  see R1.
- `cargo test -p gyre-domain user` → 12 passed, incl. new
  `deactivate_stamps_once_and_reports_state` (`test-gyre-domain-user.log`).
- `cargo test -p gyre-adapters --lib user` → 21 passed against real SQLite, incl.
  new `deactivation_persists_and_round_trips` (`test-gyre-adapters-user.log`).
- Invariant scripts: abac-route-registry, abac-exempt-handlers (80 handlers), arch,
  migration-versions, migration-sql-portability, mem-port-contracts, mcp-write-tools,
  dead-message-kinds, scope-literal-defaults, forged-scope-fields, inert-enforcement —
  all OK.
- `scripts/update-coverage-summary.sh` recount: candidate `SUMMARY.md` already matches
  the mechanical recount (user-management 8/36: 11 n/a, 17 task-assigned, 8 implemented,
  0 not-started; `grep -c '| not-started |'` = 0).
- `check-task-commit-attribution.sh` FAILs only against the executor's staged
  working-tree reset of `specs/tasks/task-124.md` (progress/commits reverted to the
  assignment state). The candidate commit itself records all four task-labeled
  commits (`33e30ac9`, `6033324f`, `60c86252`, `38b3134f`) in its frontmatter —
  stage artifact, not a candidate defect.

### Verified clean (no findings)

- **Endpoints and authorization are real.** `get_user_by_username` does a real
  lookup and enforces tenant containment (cross-tenant → 404, no existence leak;
  system principal exempt). `list_users` is TenantAdmin-only via stored
  `global_role` (not just the JWT claim), tenant-scoped, paginated with total.
  `deactivate_user` is TenantAdmin-only, same-tenant, self-deactivation → 409,
  idempotent (first stamp kept), audit-logged with reason, and cascades for real:
  auth extractor + WS + SSO reject the deactivated account; impersonation sessions
  participated in (either side) are ended AND the resolver independently re-checks
  both accounts' deactivation state; ownerless flow fires for departed Owners with
  inline TenantAdmin notification. Invitation endpoints are backed by real membership
  rows (pending = unaccepted), `accept` is invitee-only by (caller, workspace)
  lookup, revoke is pending-only and workspace-scoped (foreign invitation id → 404).
- **ABAC model consistent with repo invariants**: new routes registered in
  `api/mod.rs` with resolver entries; `invite/accept` is resolver-exempt with
  per-handler auth via `load_workspace_scoped` (tenant comparison) — passes
  `check-abac-exempt-handlers.sh`.
- **Migration is portable** plain `ALTER TABLE ... ADD COLUMN` (000063, next unused
  sequence), round-trips through SQLite and PostgreSQL row mappings and the mem
  adapter via the shared domain struct.
- **Tests are behavior-killing, not self-confirming**: the deactivation test
  pre-conditions that the target's API key authenticates (so the later 403 is
  meaningful), asserts the ownerless record + TenantAdmin notification + audit
  event with reason; the impersonation tests include a resolver-only variant that
  deactivates the account record directly (bypassing the cascade) so only the
  resolver check can reject; the tenant-scoping tests assert cross-tenant 404 and
  Developer 403 negatives.
- **Honest scoping**: the three session endpoints (`GET /users/me/sessions`,
  `DELETE .../sessions/{id}`, `POST .../sessions/revoke-all`) are NOT implemented
  and are truthfully recorded as owned by not-started task-111; coverage row 26
  stays `task-assigned` over them. This matches the task body (sessions conditional
  on task-111) and the no-fake-completion rule; not a finding.

### Findings

- [**R1 — blocking, code/merge-resolution**] `crates/gyre-server/src/api/users.rs:4102`
  (test `deactivate_user_cascades_to_auth_and_ownership`, "Cascade 3" assertion)
  calls `state.notifications.list_for_user(&Id::new(&admin_id), None, None, None, None, 50, 0)`
  with **7 arguments**, but the port signature in `crates/gyre-ports/src/notification.rs`
  takes **8** — main's task-125 widened `NotificationRepository::list_for_user` with a
  `repo_id: Option<&Id>` parameter, and this task-124-authored call site was not
  updated when `f7f5c59f` was merged in. Reproduce: `cargo check -p gyre-server --tests`
  (or any `cargo test -p gyre-server ...`) → `error[E0061]: this method takes 8
  arguments but 7 arguments were supplied`. Consequence: the acceptance criterion
  "`cargo test --all` passes" fails at compile time on the candidate — the gyre-server
  lib test target does not build. The shipped-notes evidence for this round
  ("cargo check -p gyre-server passes") only covered the non-test target, so the
  merged test code was never compiled. Repair is mechanical: add the `repo_id`
  argument (`None`) at that call site (it is the only stale `list_for_user` call in
  the tree — integration tests and production call sites already use the 8-arg form),
  commit with a task-124 label, record it in the task's `commits:` frontmatter, and
  re-run `cargo check -p gyre-server --tests` plus the server test suites below.

### Generated-output observation (non-blocking, per review policy)

`cargo check` triggered `build.rs`, which ran `npm ci` + `npm run build` for the web
assets; the resulting `web/dist/` matched the committed tree (no working-tree change).
`web/dist/` ownership stays with the deterministic verifier; no source-side frontend
changes are part of this candidate.

### Verifier must run (post-repair)

1. `cargo test --all` — the assignment's definition-of-done gate.
2. `cargo test -p gyre-server api::users::tests` — includes
   `get_user_by_username_is_tenant_scoped`,
   `list_users_is_tenant_admin_only_scoped_and_paginated`,
   `deactivate_user_cascades_to_auth_and_ownership`,
   `workspace_invitation_lifecycle`, plus the pre-existing member-management and
   task-125 notification handler tests now sharing `users.rs`.
3. `cargo test -p gyre-server api::impersonation::tests` — includes
   `deactivating_target_revokes_impersonation_token`,
   `deactivating_impersonator_revokes_impersonation_token`,
   `resolver_rejects_deactivated_accounts_even_with_unended_session`.
