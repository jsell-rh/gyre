# Review — task-121 (workspace membership model, ownership transfer, and repo access)

Spec: `specs/system/user-management.md` §Workspace Membership, §Ownership
Transfer & Reclamation, §Repo Access.
Candidate: `f9ddbdd08388d000606b19969b5b2210b935d6b0` (base `ceefc14af`).
This round is the re-review after the rebase repair (merge of main into the
published candidate; durable finding `7fdb82436d374dac91b4fa7f6393ed7d`,
merge_state DIRTY, single conflicting file `crates/gyre-server/src/lib.rs`).

Verdict: **approved** — no blocking findings. Independent evidence supports
every acceptance criterion; the merge repair preserves both sides; server-level
tests are recorded for the independent verifier (this sandbox's build wrapper
rejects gyre-server test compilation, exit 64 — transport restriction, not a
code defect).

## Merge-repair verification (this round's trigger)

- Merge commit `48295d7f5` (parents `6c44d5a32` task-121 head, `ceefc14af`
  base) resolved `lib.rs` keeping BOTH sides: task-121's `repo_access_overrides`
  AppState field + `store!` wiring (durable memberships and overrides on
  SQLite/PostgreSQL) and main's (task-158) `tenant_meta_spec_sets` + effective
  meta-spec SHA changes. Verified by diffing the merge against each parent.
- No conflict markers in `crates/` (`grep -rln '^<<<<<<< '` → none).
- `web/dist` and `web/src` are byte-identical to base — the merge took main's
  frontend wholesale; no mixed bundle. (Generated output: the deterministic
  verifier rebuilds dist after integration; not reviewed here per policy.)
- `cargo check -p gyre-server` on the merged tree: exit 0; `-p gyre-cli`: exit 0.

## Acceptance criteria — evidence

1. **WorkspaceMembership entity matches spec** — verified in
   `gyre-domain/src/workspace_membership.rs` (id, user_id, workspace_id, role,
   invited_by, accepted_at, created_at, plus the pre-existing `accepted` flag).
2. **WorkspaceRole enum** — Owner/Admin/Developer/Viewer with a privilege rank;
   `parse_role`/`as_str` round-trip; all four variants used by the permission
   model.
3. **Cannot remove or downgrade last Owner** — `remove_member`/`update_member_role`
   in `api/users.rs` count active Owners (membership role Owner whose user
   record still exists), excluding the target, and return 409 when zero.
   Tested (`last_owner_cannot_be_removed_or_downgraded`) with membership-state
   assertions after rejection.
4. **Ownership transfer PUT + audit-logged** — PUT (both `/members/:user_id`
   and the spec-named `/members/:user_id/role` alias) updates the role, emits
   `workspace_ownership_transferred` (Custom audit event with previous/new
   role, previous owner, reason, actor) and clears ownerless tracking. Tested
   (`ownership_transfer_unblocks_old_owner_removal_and_is_audited`) including
   the audit-event content and the unblocking of the old Owner's removal.
5. **Tenant Admin notified when no active Owner** — `workspace_ownership.rs`:
   `WorkspaceOwnerless` notification (priority rank 2) to every TenantAdmin of
   the workspace's tenant, deduplicated, fired inline on user removal (SCIM
   delete hook `on_user_removed`) and by the daily `orphaned_workspace_check`
   job. Tests cover tenant isolation (other-tenant admins not notified),
   non-admin members not notified, dedup across job re-runs, and
   never-owned workspaces not flagged (departure-triggered, per spec wording).
6. **Orphaned detection, 30-day configurable** — `GYRE_ORPHANED_WORKSPACE_DAYS`
   (default 30, zero/unparsable → default), kv-persisted `ownerless_since`,
   `GET /api/v1/admin/orphaned-workspaces` (admin-gated route, resolver
   registered). Threshold test seeds a 40-day record: flagged at 30, not at 60.
7. **Repo access permissions per spec table** — `gyre-domain/repo_access.rs`:
   Owner = full incl. ManageBudget; Admin = full except budget; Developer =
   read/write/create-MR/create-task/spawn-agent; Viewer = read-only. Domain
   tests assert the table exactly (8/8 pass locally).
8. **Overrides restrict, never expand** — `RepoAccessOverride::validate`
   rejects a cap more privileged than the workspace role (400 at the API,
   tested); `effective_repo_role` takes `most_restrictive` so a stored
   expansion override still cannot grant more (defense in depth, tested).
   Enforced on the git smart-HTTP transport (`git_http.rs`
   `authorize_repo_access`: upload-pack→Read, receive-pack→Write, with
   RbacDenied audit on denial) — transport test covers Developer
   read+write, Viewer read-only, non-member denied, and a Developer
   restricted to Viewer losing push but keeping fetch.
   Mutation probe: breaking `most_restrictive` fails
   `override_restricts_developer_to_viewer` — the tests are real guards.
9. **cargo test --all** — could not run here (wrapper rejects gyre-server test
   builds; 4 GiB budget). Local: gyre-domain 474/474, gyre-common 124/124,
   gyre-adapters migration suite 4/4, cargo check clean for server + cli.
   The verifier must run the full suite plus the focused server tests listed
   in `/tmp/stage/review-evidence/evidence-summary.md`.

## ABAC integration

- `subject.workspace_role` resolved from the real membership store for
  workspace-addressed routes (path param) and repo-addressed routes (repo →
  workspace lookup; unresolvable repo ⇒ no attribute ⇒ policies requiring the
  role deny). New builtin policies `builtin-workspace-member-management`
  (transfer_ownership/delete on workspace_member for Owner/Admin members) and
  `builtin-repo-access-override-management` (admin on repo for Owner/Admin
  members) seeded at startup (main.rs) and refreshed idempotently.
- The repo-access API tests run the production pipeline (router + ABAC
  middleware + seeded policies) and assert the Allow decision's matched policy
  is the override-management policy — the 201 provably came through policy
  evaluation, not around it. The Developer-denied test asserts the 403 is the
  ABAC layer's.
- Method-scoped action overrides (`PUT`→`transfer_ownership` on the member
  routes; `PUT/DELETE`→`admin` on the override route) are a clean extension of
  the frozen resolver contract; `check-abac-route-registry.sh` passes with the
  new routes registered, including the spec-named `.../members/:user_id/role`
  alias.

## Additional real-work verification

- Membership management (invite/list/role/remove) gained real authorization:
  tenant scoping (fail-closed on unresolvable scope, tested for the repo
  sibling `load_repo_scoped`), Owner/Admin/TenantAdmin gate (Developer denied
  through the production pipeline with the ABAC decision asserted Allow so the
  403 is the handler's), cross-tenant invite rejection (invitee must exist in
  the workspace's tenant), duplicate membership 409.
- `create_workspace` seeds the creator's active Owner membership (tenant-matched
  creators only), so fresh workspaces are not falsely orphaned; tested including
  a `workspace_ownership::run_once` pass.
- Durable-state fix retained: `workspace_memberships` moved from unconditional
  mem adapter to `store!` (SQLite/PostgreSQL adapters implement the port), so
  DB-backed servers no longer lose memberships on restart.
- Migration `2026-11-12-000061_repo_access_overrides` uses the next unused
  sequence (000061, past the 000060 collision with task-118), portable SQL for
  both backends; the SQLite migration test asserts both sides of the old
  collision applied. mem adapter guards the duplicate contract in code
  (check-mem-port-contracts OK); duplicate override PUT → 409 on all adapters
  (guard-before-insert + whole-chain UNIQUE-violation mapping, unit-tested
  against each adapter's error shape).
- All 20+ repo invariant scripts pass (arch, ABAC registry, migration versions,
  mem port contracts, dead message kinds — `WorkspaceOwnerless` has an emitter
  in `notify_tenant_admins` — scope/secret/path/timeout classes).

## Observations (non-blocking, for future tasks / coverage auditing)

1. `GET /api/v1/repos/:id/access/:user_id` checks only the caller's tenant
   (via `load_repo_scoped`), not workspace membership — any same-tenant
   principal can read any user's role/override/effective permissions. Same
   within-tenant read posture as `GET /workspaces/:id/members` (which this task
   upgraded from fully unauthenticated to tenant-scoped). The handler comment
   ("Any workspace member ... may read access info") overstates the check;
   consider a membership or self-only gate in a follow-up.
2. The spec's literal ABAC condition `subject.global_role == TenantAdmin AND
   action == transfer_ownership` is not expressible as written —
   `subject.global_role` holds the platform UserRole string. The TenantAdmin
   reassignment path works because TenantAdmins are provisioned with
   UserRole::Admin (create_user invariant) and match
   `builtin-admin-all-operations` (priority 900); the handler also independently
   allows a stored-GlobalRole TenantAdmin. Edge: a user whose roles list has
   Admin not-first is a TenantAdmin denied at the middleware — a pre-existing
   attribute-ordering quirk of `auth.roles.first()` affecting all admin routes,
   not introduced by this task.
3. §Repo Access permissions beyond Read/Write (Admin, ManageGates,
   ManageBudget, CreateMr, CreateTask, SpawnAgent) are modeled, derived, and
   exposed via the access view, but enforced only at the git transport (plus
   `admin` on override routes). Pre-existing REST repo write routes (e.g.
   `PUT /api/v1/repos/:id`, no auth extractor, global-role ABAC only) were not
   retrofitted with membership checks in this task — out of the diff, worth a
   follow-up task if §Repo Access is to cover the REST surface.
4. Cosmetic: `#[cfg(test)] #[cfg(test)]` duplicated attribute on the
   `api/repo_access.rs` tests module (harmless).

## Test-inflation check

None found. Production-pipeline tests assert ABAC decisions (matched policy,
Allow/Deny), the handler-403 test proves the request passed ABAC first, the
git-transport test uses real user principals and real membership/override
state, the transfer test asserts audit-event contents, and the domain tests
were mutation-killed locally (restrict-only break → FAILED).
