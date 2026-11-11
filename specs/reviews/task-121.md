# Review — task-121 (Implement workspace membership model, ownership transfer, and repo access)

Spec: `user-management.md` §Workspace Membership, §Ownership Transfer & Reclamation, §Repo Access.
Base `65f71e65` → candidate `564aed0e` (product commits: `337447e3`/`467e577f` feature checkpoints, `a8c3af6f` test fix, `93c4017c` verification-repair round 1, `8fc4245d` clippy repair, `64fa14fd` duplicate-409 repair; all recorded in the task's `commits:` frontmatter — attribution check passes at the candidate).
Verdict: **PASS** (round 3; round-2 finding verified repaired).

Evidence: `/tmp/stage/review-evidence/` (summary.md indexes every run; logs retained).
Server test-profile codegen is blocked in this sandbox by the build wrapper — the full
suite is deferred to the independent verifier with an explicit list (see summary.md);
everything runnable here was run and is green.

## Acceptance criteria — verified

- **WorkspaceMembership entity / WorkspaceRole enum: PASS.** `gyre-domain/src/workspace_membership.rs`
  carries id, user_id, workspace_id, role, invited_by, accepted_at, created_at (plus the
  `accepted` invite flag; accept mechanics belong to task-110/124 per the coverage matrix).
  Roles are Owner/Admin/Developer/Viewer with a privilege rank backing restrict-only
  override composition.
- **Cannot remove or downgrade last Owner: PASS.** `api/users.rs` `remove_member` /
  `update_member_role` count *active* owners (membership row + user record exists),
  excluding the target, and return 409 with an actionable message; the
  `last_owner_cannot_be_removed_or_downgraded` test asserts both paths leave the
  membership untouched.
- **Ownership transfer via PUT, audit-logged: PASS.** Both the M22.8 route and the
  spec-named alias `PUT /api/v1/workspaces/{id}/members/{user_id}/role` run
  `update_member_role`; transfers involving the Owner role emit a
  `workspace_ownership_transferred` audit event with previous/new role, previous owner,
  reason, and actor, and clear ownerless tracking immediately. The ABAC middleware
  resolves the PUT on both patterns to a `transfer_ownership` action on
  `workspace_member`, allowed by the new `builtin-workspace-member-management` policy
  (subject.workspace_role in [Owner, Admin]) and, for TenantAdmins (platform Admin role),
  by `builtin-admin-all-operations` — the spec's "TenantAdmin reassigns ownership" flow
  works end to end (test: `ownership_transfer_unblocks_old_owner_removal_and_is_audited`).
- **Tenant Admin notified when no active Owner: PASS.** `workspace_ownership.rs` notifies
  every TenantAdmin *of the workspace's tenant* (cross-tenant admins excluded — tested),
  priority-2 `WorkspaceOwnerless`, deduplicated across job re-runs (tested), fired inline
  on the only user-deletion path (SCIM deactivate, `scim.rs` → `on_user_removed`) and by
  the daily `orphaned_workspace_check` job (`jobs.rs`, 86400s). Owner activity requires
  the user record to exist; a departed owner's surviving membership row is the
  departure evidence, so never-owned (system/agent-created) workspaces are neither
  flagged nor notified (tested) — and `create_workspace` seeds the creator's active
  Owner membership so user-created workspaces are never falsely flagged (tested).
- **Orphaned detection, 30-day configurable: PASS.** `GYRE_ORPHANED_WORKSPACE_DAYS`
  (default 30, zero/unparsable → default) via `GET /api/v1/admin/orphaned-workspaces`
  (admin resource → non-Admin denied by `builtin-admin-only-deny`); threshold behavior
  tested with seeded records.
- **Repo access from workspace role per spec table: PASS.** `gyre-domain/src/repo_access.rs`
  `repo_permissions()` matches the table exactly (Owner incl. manage budget; Admin full
  except budget; Developer read/write/create-MR/create-task/spawn-agent; Viewer read-only
  — each pinned by a domain test). Enforced on the git smart-HTTP transport
  (`git_http.rs` `authorize_repo_access`: upload-pack → Read, receive-pack → Write,
  non-members 403 with `RbacDenied` audit) —
  `repo_access_enforced_on_git_transport_for_user_principals` drives real user principals
  with API keys through the transport, including Developer/Viewer/outsider matrix and an
  override restricting a Developer to Viewer (fetch OK, push 403).
- **Per-repo overrides restrict, never expand: PASS.** `RepoAccessOverride::validate`
  rejects a cap more privileged than the workspace role (400 — tested through the
  production ABAC pipeline), and the effective role is computed as
  `most_restrictive(workspace_role, override)` even if an expansion override were stored
  (defense in depth). Port + SQLite/PostgreSQL adapters (mirrored, plain INSERT so the
  UNIQUE(repo_id, user_id) constraint enforces the no-silent-rewrite contract) + portable
  migration `2026-11-12-000060` (next unused sequence number; runs on SQLite — proven by
  the full `cargo test -p gyre-adapters --lib` suite, 438 passed, whose
  `SqliteStorage` constructor executes the embedded migrations) + mem adapter with the
  duplicate guard in code (mem-port-contract check green).
- **cargo test --all: deferred to the verifier** (build wrapper blocks server test
  codegen in this sandbox). Everything runnable here is green: domain 8/8 (`repo_access`),
  common 8/8 (notification taxonomy incl. `WorkspaceOwnerless`), adapters 438/438,
  `cargo check -p gyre-server --lib` and `--tests` exit 0, clippy clean on changed lines.
  Required verifier runs are listed in `/tmp/stage/review-evidence/summary.md`.

## Round-2 finding (duplicate override → 409 only on mem adapter): REPAIRED and independently verified

`64fa14fd` replaces the top-level message match with (a) guard-before-insert — a real
`find_by_repo_and_user` lookup, so the 409 no longer depends on adapter error shape —
and (b) `is_duplicate_override_error`, which walks the whole anyhow chain to recognize
the mem ("already exists"), SQLite ("UNIQUE constraint failed") and PostgreSQL
("duplicate key value violates unique constraint") shapes. I verified the premise with a
standalone probe against real SQLite + diesel 2.3.7 + anyhow 1.0.102 (workspace lock
versions): a duplicate insert wrapped with `.context("insert repo access override")`
(what `sqlite/repo_access.rs` does) keeps the driver marker only in a chain cause; the
old top-level-only check misses it (would 500), the new chain walk matches (409), and a
non-duplicate storage failure does not match (stays 500). Probe source and output
retained under `/tmp/stage/review-evidence/probe/` and `dup-probe-output.txt`.

## ABAC integration — verified live, not just registered

- New routes (`/api/v1/repos/:id/access-overrides[/:user_id]`, `/access/:user_id`,
  `/api/v1/admin/orphaned-workspaces`, the `.../members/:user_id/role` alias) are all in
  the `RouteResourceMapping` registry (check-abac-route-registry green).
- `subject.workspace_role` is resolved from the real membership store for the workspace
  the request addresses — including repo-addressed routes, where the workspace is loaded
  from the repo store; unresolvable scope yields no attribute (fail-closed).
  `workspace_admin_manages_overrides_through_production_abac` runs the full production
  pipeline (api router + abac middleware + seeded builtins) with a workspace-Admin member
  whose *global* role is a plain Developer — the only path that can admit them is the new
  builtin `repo-access-override-management` policy — and additionally asserts the recorded
  Allow decision names that policy. The mirror test proves a Developer member is 403'd by
  the ABAC layer. The repo `admin` action is produced by no other repo route
  (`method_to_action` never yields it; no other repo mapping overrides to it), so the
  new policy cannot broaden repo deletion/update.
- Handler-level containment is real: tenant scoping on every new handler
  (`load_repo_scoped` is fail-closed on a dangling repo→workspace reference — tested),
  Owner/Admin/TenantAdmin gates on override management and membership management, and
  `member_management_requires_owner_admin_or_tenant_admin` proves the 403 comes from the
  handler (ABAC decision recorded as Allow for that exact request).

## Non-findings (checked, judged acceptable — recorded for the controller)

- `GET /api/v1/repos/{id}/access/{user_id}` is tenant-scoped, not workspace-membership-scoped:
  any same-tenant user can read a member's effective role/permissions. Consistent with the
  pre-existing `list_members` tenant scoping, the endpoint is this task's own addition
  (not spec-mandated), and tenants are the hard isolation boundary; role metadata only.
  A follow-up could require caller membership or self.
- Coverage rows 4/8/9 in `specs/coverage/system/user-management.md` remain `task-assigned`;
  per pipeline convention (cf. task-123's `verify` reconcile commit) the matrix
  reconciliation to `implemented` happens in the verify/process stage after review —
  flagging so it is not lost. The task file's Shipped notes are accurate and match what
  I verified (including the round-3 note correctly describing the duplicate-409 repair).
- `authorize_repo_access` admits principals without a user identity (agent tokens) —
  documented design decision (agents are repo-scoped at spawn; membership is a human
  concept), consistent with the rest of the transport's trust model.
- Durable-state move: `workspace_memberships` went from unconditional mem to `store!`
  wiring, so DB-backed deployments persist memberships — a correctness fix this task's
  shipped notes call out; kv-backed ownerless records were already port-backed.

## Verifier must run (recorded in /tmp/stage/review-evidence/summary.md)

`cargo test --all`; focused `cargo test -p gyre-server --lib` for `api::repo_access`
(esp. `override_create_is_idempotent_rejected`, `duplicate_override_error_mapping_covers_all_adapter_chains`,
`workspace_admin_manages_overrides_through_production_abac`, `dangling_workspace_scope_is_fail_closed`),
`workspace_ownership`, `api::users`, `api::workspaces`, `git_http`
(`repo_access_enforced_on_git_transport_for_user_principals`), `abac_middleware`.

— Reviewer, round 3 (2026-11-12). Prior rounds: round 1 (verification findings → repairs
`93c4017c`, `8fc4245d`), round 2 (duplicate-409 finding → repair `64fa14fd`, verified
repaired above).
