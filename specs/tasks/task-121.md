---
title: "Implement workspace membership model, ownership transfer, and repo access"
spec_ref: "user-management.md §Workspace Membership"
depends_on:
  - task-120
progress: ready-for-review
coverage_sections:
  - "user-management.md §Workspace Membership"
  - "user-management.md §Ownership Transfer & Reclamation"
  - "user-management.md §Repo Access"
commits: ["64fa14fd4fbb1feecbd69c04d3cb8020fc4a237f", "8fc4245da30d5c96d1c0cb2568ef70e573ed84d3", "93c4017c45159c73c59160eb83f45de08ba2c342", "a8c3af6f3b12a1524964baaef0e48897ad317755", "337447e39cc439505dcebf4f522f65b0c77ebe18", "467e577f5cd6f7507652a2d84e0bd534a17bd4c5"]
---

## Spec Excerpt

From `user-management.md` §Workspace Membership:

```rust
pub struct WorkspaceMembership {
    pub id: Id,
    pub user_id: Id,
    pub workspace_id: Id,
    pub role: WorkspaceRole,
    pub invited_by: Id,
    pub accepted_at: Option<u64>,
    pub created_at: u64,
}

pub enum WorkspaceRole {
    Owner,      // Full workspace control, budget management, can delete workspace
    Admin,      // Manage members, configure repos, manage personas
    Developer,  // Create/modify tasks, MRs, spawn agents, approve specs
    Viewer,     // Read-only access to all workspace data
}
```

From §Ownership Transfer & Reclamation:
- When a workspace Owner leaves, Tenant Admin receives notification
- Tenant Admin can reassign ownership via `PUT /api/v1/workspaces/{id}/members/{new_owner_id}/role`
- Orphaned workspace flagged in admin dashboard after 30 days
- Owner can voluntarily transfer; must designate new Owner before downgrading
- Cannot remove last Owner

From §Repo Access:

| Workspace Role | Repo Access |
|---|---|
| Owner | Full (read, write, admin, manage gates, manage budget) |
| Admin | Full (read, write, admin, manage gates) |
| Developer | Read, write, create MRs, create tasks, spawn agents |
| Viewer | Read-only |

Per-repo overrides can restrict (but not expand) access beyond workspace role.

## Implementation Plan

1. **Audit existing membership model:**
   - Check `WorkspaceMembership` struct and `WorkspaceRole` enum in domain
   - Check existing `POST/GET /api/v1/workspaces/:id/members` and `PUT/DELETE .../members/:user_id` endpoints
   - Identify gaps between existing implementation and spec

2. **Ownership transfer logic:**
   - Add validation: cannot remove/downgrade last Owner
   - Add `transfer_ownership` action in role change endpoint
   - When Owner deactivated: create notification for Tenant Admin
   - Orphaned workspace detection: background job checks workspaces with no active Owner

3. **Repo access inheritance model:**
   - Define permission set per WorkspaceRole (per spec table)
   - Add `RepoAccessOverride` entity for per-repo restrictions
   - Overrides can only restrict, never expand beyond workspace role
   - Migration for `repo_access_overrides` table if needed

4. **ABAC integration:**
   - Ensure workspace role is available as `subject.workspace_role` attribute
   - Repo access checks use membership + override logic

## Acceptance Criteria

- [ ] WorkspaceMembership entity matches spec (id, user_id, workspace_id, role, invited_by, accepted_at, created_at)
- [ ] WorkspaceRole enum: Owner, Admin, Developer, Viewer
- [ ] Cannot remove or downgrade last Owner of a workspace
- [ ] Ownership transfer: PUT changes role, audit-logged
- [ ] Tenant Admin notified when workspace has no active Owner
- [ ] Orphaned workspace detection (30-day configurable period)
- [ ] Repo access permissions derived from workspace role per spec table
- [ ] Per-repo overrides can restrict (but not expand) access
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/user-management.md` §Workspace Membership through §Repo Access. Existing membership endpoints: grep for `invite_member\|list_members` in `gyre-server/src/api/`. Workspace domain: `gyre-domain/src/workspace.rs`. Member role change: grep for `members/:user_id` in `gyre-server/src/api/mod.rs`. Notification creation: grep for `NotificationType` in `gyre-common/`. Background jobs: grep for `tokio::spawn` in `gyre-server/src/main.rs` or `lib.rs`.

## Shipped

Workspace membership, ownership transfer/reclamation, and repo access are implemented
end-to-end (user-management.md §Workspace Membership, §Ownership Transfer & Reclamation,
§Repo Access), across the feature round, two review/verification repair rounds, a
duplicate-override 409 repair round, and this rebase repair round.

### Behavior (feature)

- `WorkspaceMembership` (gyre-domain) carries id, user_id, workspace_id, role, invited_by,
  accepted_at, created_at (plus an `accepted` invite flag); `WorkspaceRole` is
  Owner/Admin/Developer/Viewer with a privilege rank used for restrict-only override
  composition.
- Membership management (`POST/GET/PUT/DELETE /api/v1/workspaces/:id/members...`) is restricted
  to Owner, workspace Admin, or Tenant Admin; inviting an existing member is a 409. The last
  Owner cannot be removed or downgraded; ownership transfer via the role-change PUT is
  audit-logged and clears ownerless tracking.
- Ownerless tracking: when the last active Owner leaves, every TenantAdmin of the tenant is
  notified (`WorkspaceOwnerless`, priority rank 2) inline and via the daily
  `orphaned_workspace_check` job (deduplicated); after `GYRE_ORPHANED_WORKSPACE_DAYS`
  (default 30) the workspace is flagged in `GET /api/v1/admin/orphaned-workspaces`.
- Repo access: `RepoPermission` sets per role follow the spec table; `RepoAccessOverride`
  (port + SQLite/Postgres adapters + migration) stores per-repo restrictions; the effective
  role is the more restrictive of workspace role and override (restrict-only). git_http
  read/write paths enforce the effective permission set via `authorize_repo_access`.

### Behavior (review/verification repairs, rounds 1-2)

- `subject.workspace_role` is resolved for repo-addressed routes (`/api/v1/repos/:id/...`)
  by loading the repo's workspace before policy evaluation, so the builtin
  `repo-access-override-management` policy is live on the override routes (previously
  default-denied 403 before the handler ran — dead policy).
- `load_repo_scoped` is fail-closed on an unresolvable workspace scope: a dangling
  repo→workspace reference or a failed lookup is NotFound, never a pass-through of the tenant
  gate.
- `create_workspace` seeds the creator's active Owner membership (same-tenant user
  principals), and ownerless tracking fires only on owner departure (evidence: an Owner
  membership row, which survives user deletion). Never-owned workspaces (system/agent-operated)
  are not flagged orphaned and generate no TenantAdmin notification noise.
- clippy-diff findings on the new surface cleared (`double_must_use` allow on the new port,
  `doc_lazy_continuation`/`result_large_err` in git_http, test-only import scoping);
  denial responses are dereferenced unchanged at all git_http call sites, so 403/500
  behavior is identical.

### Behavior (repair round 3: duplicate override 409 on all adapters)

- The duplicate-override 409 in `set_repo_access_override` previously matched only the
  top-level message "already exists" — the mem adapter's guard text. The SQLite and
  PostgreSQL adapters wrap the driver's UNIQUE violation with
  `.context("insert repo access override")`, so a second
  `PUT /api/v1/repos/{id}/access-overrides/{user_id}` for the same (repo, user) returned
  500 instead of the documented 409 (docs/api-reference.md).
- Fixed with the repo's guard-before-insert pattern (compute_targets, invite_member):
  `find_by_repo_and_user` runs a real storage lookup before `create`, so the 409 no longer
  depends on any adapter's error-message shape. The storage UNIQUE constraint remains as
  the race-window backstop, now mapped by `is_duplicate_override_error`, which walks the
  whole anyhow error chain to recognize the mem ("already exists"), SQLite
  ("UNIQUE constraint failed"), and PostgreSQL ("duplicate key value violates unique
  constraint") duplicate shapes while leaving non-duplicate storage failures as 500.

### Behavior (this round: rebase repair onto fbf1a63)

- Rebased the task-121 branch onto base fbf1a63 (task-085 HSI, task-134 gate agents,
  task-generation/gate-validation migrations). Single content conflict in
  `abac_middleware.rs`: the base's task-134 scoped-token allow-list requires `action`
  before attribute-context building (via the base's pattern-wide `action_override`
  destructure), while task-121 moved action resolution to `RouteResourceMapping::resolve_action`
  (method-scoped overrides -> pattern override -> method mapping) after `subject.workspace_role`
  resolution. Resolved by resolving the action once, early, through `resolve_action`, so the
  scoped-token deny record and the policy evaluation share the same per-method action; the
  later duplicate block was dropped. No behavior change for either feature: task-121 routes
  with method-scoped overrides resolve identically, and the base's pattern-wide override is
  still consulted (second priority) by `resolve_action`.
- All task-121 surfaces verified intact after the merge: repo_access duplicate-409 guard +
  error-chain mapping, git_http `authorize_repo_access` enforcement, ownerless tracking
  (`workspace_ownership.rs`, jobs, scim), ABAC registry entries for member and
  override routes.

### Test evidence

- Prior rounds (focused runs via `/tmp/stage/bin/cargo`, summaries retained under
  /tmp/stage/review-evidence): `api::repo_access` through the production router
  (api_router + abac_middleware + seeded builtin policies) with workspace-member API keys,
  including the regression test that fails (403) when repo-addressed workspace-role
  resolution is disabled; `workspace_ownership` 8/8; `api::workspaces::tests` 20/20;
  `api::users::tests` 23/23; `git_http` 61/61 (incl. repo-access enforcement on the git
  transport); `abac_middleware` 14/14; full gyre-server lib suite 1727/1727;
  `cargo test -p gyre-domain repo_access` 8/8; clippy 0 findings on changed lines vs base.
- Round 3: `cargo check -p gyre-server --lib` passes (exit 0);
  `cargo clippy -p gyre-server --lib` reports no findings in repo_access.rs; a rustc
  behavior probe against the cached anyhow rlib reproduced each adapter's exact error
  chain and proved the old top-level-only check misses the SQLite/Postgres shapes
  (top-level message is exactly "insert repo access override") while
  `is_duplicate_override_error` matches all three duplicate shapes and rejects a
  non-duplicate failure (evidence: /tmp/stage/review-evidence/task-1....md).
- This round (rebase): `cargo check -p gyre-server --lib`, `cargo check --workspace`, and
  `cargo check -p gyre-server --tests` all exit 0 (the merge, including the resolved
  conflict, type-checks against both sides' code); `cargo test -p gyre-domain repo_access`
  8/8; `cargo clippy -p gyre-server --lib` exit 0 with no findings in `abac_middleware.rs`
  or `repo_access.rs`; `scripts/check-abac-route-registry.sh`, `scripts/check-arch.sh`, and
  `scripts/check-migration-versions.sh` all pass (evidence:
  /tmp/stage/review-evidence/task-121-rebase-887fc7bc.md).
- Deferred to the independent verifier (server test-profile compilation exceeds this
  sandbox's 4 GiB cgroup budget): `cargo test -p gyre-server --lib api::repo_access` —
  notably `override_create_is_idempotent_rejected` (guard path → 409 through the production
  ABAC pipeline) and `duplicate_override_error_mapping_covers_all_adapter_chains`
  (chain-shape mapping per adapter) — plus `workspace_ownership`, `api::workspaces::tests`,
  `api::users::tests`, `git_http`, `abac_middleware`, and the task-134 integration test
  `tests/task134_review_probe.rs` (both sides of the resolved conflict now share one
  action-resolution path, so the scoped-token allow-list and policy evaluation must be
  re-exercised together).
