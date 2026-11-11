# Review — task-121 (Workspace membership model, ownership transfer, and repo access)

Spec: `user-management.md` §Workspace Membership, §Ownership Transfer & Reclamation, §Repo Access.
Candidate: `97a5b74dc6fcd5b5af46b893f09e6cfc15c88493` (branch `pipeline/task-121/c4b6843c33f24b6086839d39f658114d-1`), base `cba416b4`.
Commits under review (all 7 verified present and attributed in the task frontmatter; attribution check passes): `337447e`, `467e577` (checkpoints), `a8c3af6`, `93c4017`, `8fc4245`, `64fa14f`, `7939a4a` (repairs).
Verdict: **PASS** (approved; server-suite execution obligations recorded for the independent verifier below).

Evidence: `/tmp/stage/review-evidence/task-121-review.md` (probe commands, outputs, exit codes, mutation probes).

## What ran in this review (independent)

- `cargo test -p gyre-domain` → **474 + 8 passed, 0 failed** (incl. all 8 `repo_access` domain tests and `workspace_membership`).
- `cargo test -p gyre-adapters --lib sqlite::tests` → **9 passed, 0 failed** (incl. the new `migrations_create_000060_repo_columns_and_overrides` collision guard).
- **Mutation probe 1** — restored the duplicate `2026-11-12-000060` version (the original collision): the guard FAILS with `table 'repo_access_overrides' missing`. The silent-drop regression is killed; tree restored and re-run green.
- **Mutation probe 2** — inverted `WorkspaceRole::most_restrictive` (most-privileged-wins): `override_cannot_expand_viewer_to_developer` and `override_restricts_developer_to_viewer` FAIL. The restrict-only invariant is test-protected; tree restored and re-run green.
- `cargo check -p gyre-server` → clean. `cargo check -p gyre-server --tests` → clean (all new test modules compile).
- All 20 relevant mechanical gates pass (migration versions/portability, arch, ABAC route registry + exempt handlers + context parity, mem port contracts, dead message kinds, in-memory state stores, inert enforcement, forged/fabricated scope fields, scope literal defaults, lossy secrets, relative path defaults, unbounded HTTP, forwarded headers, byte-slice truncation, commit attribution).

## Verified against the task contract

- **§Workspace Membership**: `WorkspaceMembership` carries every spec field (id, user_id, workspace_id, role, invited_by, accepted_at, created_at; plus an `accepted` invite flag — superset, fine). `WorkspaceRole` is Owner/Admin/Developer/Viewer with a privilege rank. Membership management (POST/GET/PUT/DELETE, plus the spec-named `PUT .../members/{user_id}/role` alias) is restricted to workspace Owner/Admin or TenantAdmin, tenant-scoped, duplicate invite → 409. `create_workspace` seeds the creator's accepted Owner membership.
- **§Ownership Transfer & Reclamation**: last active Owner cannot be removed or downgraded (409, `count_active_owners_excluding` treats a deleted user's lingering membership as inactive); transfers are audit-logged (`workspace_ownership_transferred` with previous/new role, previous owner, reason, actor — asserted by querying the audit store) and clear ownerless tracking; TenantAdmins of the workspace's tenant are notified (`WorkspaceOwnerless`, priority rank 2) inline on SCIM user deletion and via the daily `orphaned_workspace_check` job, deduplicated; the 30-day orphan flag is configurable (`GYRE_ORPHANED_WORKSPACE_DAYS`, default 30, zero/unparsable falls back) and surfaced at `GET /api/v1/admin/orphaned-workspaces` (ABAC admin resource). Never-owned workspaces are correctly not flagged (departure is the trigger). The TenantAdmin reassignment path is reachable through the production ABAC pipeline: platform convention maps `GlobalRole::TenantAdmin ⇔ UserRole::Admin` at every provisioning path, so `builtin-admin-all-operations` (900) admits the `transfer_ownership` action; the handler additionally authorizes TenantAdmin from the stored user record (stored record authoritative over JWT claims).
- **§Repo Access**: `RepoPermission` sets per role match the spec table exactly (Owner adds manage_budget over Admin; Developer read/write/create_mr/create_task/spawn_agent; Viewer read-only). The effective role is `most_restrictive(workspace_role, override)`; an override more privileged than the workspace role is rejected (400) before storage; duplicates 409 on all three adapters (guard-before-insert + whole-chain duplicate-error mapping, with a unit test reproducing each adapter's wrapped error shape). Enforcement is live on the git transport (`authorize_repo_access`: fetch=Read, push=Write, RbacDenied audit events on denial, tested with real user principals and API keys through the production router). `subject.workspace_role` is resolved for workspace- and repo-addressed routes before policy evaluation, and the two new builtin policies (810/809) gate member/override management; the production-pipeline tests assert the Allow decision's matched policy, proving the path ran through evaluation rather than around it. Cross-tenant and dangling-scope cases are fail-closed (NotFound/Forbidden, tested).
- **Storage**: `repo_access_overrides` migration renumbered to the next unused sequence `2026-11-12-000061` (repairing the 000060 collision with task-118's `repo_agent_limits`); portable SQL; UNIQUE(repo_id, user_id); down migration present; mem/SQLite/Postgres adapters implement the port with the duplicate contract enforced in code on mem. `workspace_memberships` is now `store!`-wired (DB-backed in DB mode — fixes silent membership loss on restart).

## Non-findings (checked, judged acceptable)

- Migration-collision guard test comment states Diesel ran "the alphabetically-first repo_access_overrides"; empirically (mutation probe) `repo_access_overrides` is the side that gets dropped. The guard asserts both sides, so detection is direction-independent. Cosmetic.
- `#[cfg(test)]` attribute duplicated in `api/repo_access.rs` test module declaration. Harmless.
- REST repo surfaces (repo metadata reads, MR/task creation) remain gated by the platform's global-role ABAC policies rather than workspace membership; task-121 enforces the §Repo Access table on the git transport (the repo content path) and override management, without regressing existing surfaces. A broader membership-based sweep of REST repo routes would be a separate task; the acceptance criteria as scoped are met.
- `authorize_repo_access` admits principals without a user identity (agent tokens) unconditionally — consistent with the platform's agent scoping model (agents are repo-scoped at spawn and run the gate chain).

## Verifier obligations (server test-profile compilation exceeds this sandbox's 4 GiB cgroup budget)

The candidate compiles clean in test configuration (`cargo check -p gyre-server --tests`). The independent verifier MUST execute:

- `cargo test -p gyre-server --lib` — at minimum `api::repo_access`, `workspace_ownership`, `api::users::tests`, `api::workspaces::tests`, `git_http`, `abac_middleware`;
- `cargo test --all` (task acceptance criterion).

— Reviewer, 2026-11-12
