# Review — task-121: Workspace membership, ownership transfer, repo access

**Candidate:** `aa3fdc0f60dec57eb804f568886b93a7e06276e3` (base `65f71e65`)
**Verdict:** NEEDS REVISION (1 finding) — evidence in `/tmp/stage/review-evidence/`

## What was reviewed

Full diff base..candidate (~3.7k lines): domain model (`repo_access.rs`,
`workspace_membership.rs`), new port + SQLite/PG/mem adapters + migration
000060, server surface (`api/repo_access.rs`, `api/users.rs` membership
endpoints, `api/workspaces.rs` creator-owner seeding, `api/admin.rs`
orphaned-workspaces, `api/scim.rs` inline ownerless trigger, `jobs.rs`
`orphaned_workspace_check`, `workspace_ownership.rs`, `git_http.rs`
transport enforcement, `abac_middleware.rs` subject.workspace_role
resolution + method-scoped action overrides + two builtin policies),
docs, and the task record.

## Verified with independent evidence

- **Spec-table permissions + restrict-only overrides** — `cargo test -p
  gyre-domain repo_access` 8/8; effective role is
  `most_restrictive(workspace_role, override)`, so an override can never
  expand (asserted even against a stored expansion override).
- **Last-Owner protection, transfer audit, TenantAdmin gating** — handler
  logic reads correctly (`count_active_owners_excluding` treats a deleted
  user's lingering Owner membership as inactive); the transfer test
  asserts the real audit event (`workspace_ownership_transferred` with
  previous/new role and reason) via `state.audit.query`. The
  Developer-forbidden test runs the production ABAC pipeline AND asserts
  an Allow decision existed, proving the 403 is the handler's.
- **Ownerless notification/orphan flag** — `workspace_ownership.rs`
  logic reviewed line-by-line: tenant-isolated TenantAdmin notification
  with dedup, departure-vs-never-owned discrimination, 30-day
  configurable threshold (`GYRE_ORPHANED_WORKSPACE_DAYS`), reassignment
  clears tracking, inline trigger on SCIM user removal + daily job
  backstop. kv-backed (`store!`), restart-safe.
- **Git transport enforcement** — `authorize_repo_access` gates
  info_refs/upload-pack (Read) and receive-pack (Write) with real
  membership + override lookups and an `RbacDenied` audit event on
  denial; the test drives real API-key user principals (Developer
  read+write, Viewer read-only, outsider none, overridden Developer
  loses push).
- **Mechanical invariants** — check-abac-route-registry,
  -migration-versions, -migration-sql-portability, -mem-port-contracts,
  -arch, -dead-message-kinds, -in-memory-state-stores,
  -abac-context-parity, -api-auth: all pass. New routes all registered;
  migration 000060 is unique and portable SQL; mem adapter guards the
  duplicate-create port contract in code.
- **Clippy-diff gate (prior verification failure)** — independently
  reproduced the gate's scope: `cargo clippy -p gyre-server --lib --tests
  --all-features -W clippy::all` (and the four smaller changed crates)
  intersected with the candidate's added lines = **zero warnings**. The
  repair commit 8fc4245da is legitimate cleanup, not gate-gaming.
- `cargo check -p gyre-server --lib --tests` clean; build.rs web build
  succeeded.

## Finding (blocks approval)

**F1 — duplicate override → 500 (not the documented 409) on SQLite/PG.**
`set_repo_access_override` maps to Conflict only when
`e.to_string().contains("already exists")` — the mem adapter's message.
The SQLite/PG adapters wrap the UNIQUE violation with
`.context("insert repo access override")`, so `to_string()` never
matches and the default SQLite deployment returns 500, contradicting the
api-reference.md row this candidate adds ("a duplicate override → 409").
The mem-backed test `override_create_is_idempotent_rejected` masks it.
Fix: find-before-create (as the candidate itself did in
`invite_member`) or match the diesel unique-violation error chain (cf.
explorer_ws.rs:1526, compute_targets.rs:114).

## Notes (non-blocking)

- The spec's literal ABAC condition (`subject.global_role == TenantAdmin`
  AND `action == transfer_ownership`) is not expressible in the
  middleware's pre-existing attribute model (`subject.global_role` is the
  UserRole string). The behavior is delivered: TenantAdmins necessarily
  carry `UserRole::Admin` (users.rs creation logic) and pass via
  `builtin-admin-all-operations`; workspace Owner/Admin pass via the new
  priority-810 policy on `subject.workspace_role`. Enforcement is as
  strict or stricter than the spec's.
- `GET /repos/:id/access/:user_id` and `GET /workspaces/:id/members`
  require tenant scope but not workspace membership of the caller —
  consistent with every sibling tenant-scoped read endpoint (and
  strictly tighter than the base, where `list_members` had no auth
  extractor at all).
- `GET /admin/orphaned-workspaces` lists across tenants — consistent
  with the existing global admin surface (admin_health/admin_audit).

## Deferred to the independent verifier (wrapper forbids gyre-server test builds here)

- `cargo test --all` (acceptance criterion)
- Focused: `cargo test -p gyre-server --lib workspace_ownership`,
  `repo_access`, `last_owner`, `ownership_transfer`,
  `invite_member_rejects_duplicate`,
  `member_management_requires_owner_admin`,
  `create_workspace_seeds_owner_membership`,
  `repo_access_enforced_on_git_transport`
- The original failing gate: `cargo clippy --all-targets
  --all-features` + `python3 scripts/check-clippy-diff.py 65f71e65`
  (my lib+tests probe found 0 changed-line warnings; `--all-targets`
  could not run in this sandbox)
