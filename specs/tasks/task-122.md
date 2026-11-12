---
title: "Enhance team management with lead, slug, and integration points"
spec_ref: "user-management.md §Team Management"
depends_on:
  - task-121
progress: ready-for-review
coverage_sections:
  - "user-management.md §Team Management"
commits: ["bbd72bc37ffd0bd4b286171ad73d236fca06c64b", "abaee5ea1e3f5e945f236f58b0b64609f2c5727b", "a2b86d834da6710c12e34301c96434d2bd4a414f", "3056a7edd757b9d36b4e7c0e485951bc715755fd"]
---

## Spec Excerpt

From `user-management.md` §Team Management:

```rust
pub struct Team {
    pub id: Id,
    pub workspace_id: Id,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub lead: Id,              // User responsible for the team
    pub created_at: u64,
}

pub struct TeamMembership {
    pub team_id: Id,
    pub user_id: Id,
    pub added_at: u64,
}
```

Teams are used for:
- Notification routing ("notify the platform team")
- Spec ownership ("this spec is owned by team:platform")
- Default reviewers (MRs in repos owned by team X auto-assign team X for review)
- Budget reporting (aggregate budget usage by team)

Teams don't grant additional permissions beyond workspace membership. They're organizational, not authorization.

## Implementation Plan

1. **Audit existing team model:**
   - Check `POST/GET /api/v1/workspaces/:id/teams` and `PUT/DELETE .../teams/:team_id` endpoints (these exist per mod.rs)
   - Check existing Team domain entity fields
   - Identify missing fields: `slug`, `description`, `lead`

2. **Enhance Team entity:**
   - Add `slug` field (unique within workspace, URL-safe)
   - Add `description` field (optional)
   - Add `lead` field (user_id reference)
   - Migration to add columns

3. **Team integration points:**
   - Notification routing: when a notification targets a team, resolve to team members
   - Spec ownership: support `team:slug` in spec manifest `approvers` field
   - Default reviewers: teams can be configured as default MR reviewers for repos
   - Budget reporting: aggregate agent budget usage by team membership

4. **TeamMembership audit:**
   - Verify `POST .../teams/:team_id/members` and `DELETE .../teams/:team_id/members/:user_id` endpoints
   - Add `added_at` timestamp if missing

## Acceptance Criteria

- [ ] Team entity has all spec'd fields: id, workspace_id, name, slug, description, lead, created_at
- [ ] TeamMembership has team_id, user_id, added_at
- [ ] Slug is unique within workspace, URL-safe
- [ ] Lead field references a valid workspace member
- [ ] Team can be referenced in notification routing (team:slug format)
- [ ] Existing team CRUD endpoints work with new fields
- [ ] Migration adds missing columns
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/user-management.md` §Team Management. Existing team endpoints: grep for `create_team\|list_teams` in `gyre-server/src/api/`. Team domain model: grep for `struct Team` in `gyre-domain/`. Team adapter: grep for `TeamRepository` in `gyre-adapters/`. Check migration numbering: `ls crates/gyre-adapters/migrations/ | tail -5`.

## Shipped

### Entity + storage

- `Team` (gyre-domain/src/team.rs) carries every spec field: `slug` (workspace-unique, URL-safe; `validate_slug`/`sanitize_slug` — explicit invalid slugs are 400, never silently rewritten) and `lead` (user id, validated against real workspace membership). Membership is the spec's `TeamMembership { team_id, user_id, added_at }`, persisted in a `team_memberships` table (composite PK).
- Migration `2026-11-13-000062_team_slug_lead_membership` (portable SQLite + PostgreSQL SQL): adds `slug`/`lead` to `teams` (backfilled: slug ← team id, lead ← workspace Owner), unique `idx_teams_workspace_slug`, `team_memberships`; drops the superseded never-writable `member_ids` JSON column.
- `TeamRepository` port gains `find_by_slug`, `add_member` (fails on duplicate), `remove_member` (idempotent), `list_members` (ordered by `added_at`), `list_for_user`; all adapters enforce the duplicate contracts (mem in code, SQLite/PG via indexes with stable error labels).

### API (gyre-server/src/api/users.rs)

- Team CRUD + membership endpoints authenticate, scope to the caller's tenant (`load_workspace_scoped`), gate on Owner/Admin/TenantAdmin, and verify the path's workspace owns the team (cross-workspace team id → 404). `lead` is validated against workspace membership (400), slug collisions → 409. New `POST .../teams/:team_id/members` (duplicate → 409, returns `added_at`) and `DELETE .../members/:user_id` (idempotent 204); routes registered with ABAC resolver entries.

### Integration points (user-management.md §Team Management)

- **Notification routing (`team:slug`)**: `resolve_team_members` (notifications.rs) resolves `team:{slug}` references to real member ids via `find_by_slug` + `list_members`; unresolvable references log and skip — never fabricate recipients. Wired into the `SpecApprovalRequested` emitter (specs_assist save flow).
- **Spec ownership (`team:slug` approvers)**: `resolve_new_approval_status` (api/specs.rs) expands `team:{slug}` entries in a manifest's `human_approvers` into the approver-id forms members' approvals record, before the pure §9 resolver runs.
- **Budget reporting by team**: `GET /api/v1/workspaces/:id/budget/by-team` aggregates real `BudgetCallRecord` audit rows through team membership; unattributable records are reported under `unattributed`, not dropped.
- **Default reviewers** is NOT implemented: no spec models a repo→team ownership association (the `Repository` entity has no team field), so there is nothing to auto-assign against. Noted here rather than faked; needs a modeled repo-team ownership link first.

### Verification repair (this round, commit `3056a7edd`)

The prior verification round (`cargo test --all`, candidate `fea1a77e9`) failed three gyre-server tests — the first full-suite execution of this task's server tests (sandbox build budget had deferred them). Root causes and fixes:

- `api::specs::tests::team_slug_human_approvers_resolve_through_membership_e2e` — `expand_team_approvers` dropped an unresolvable `team:nonexistent` reference, leaving an **empty** `human_approvers` list, which the §9 resolver treats as "any human may approve" — a fail-open (lead's approval → Approved). Fix: an unresolvable reference (or a team with no members) is kept verbatim in the approver list; no approval records a `team:...` approver id, so the resolver can never match it — the spec stays Pending, fail-closed.
- `api::specs_assist::tests::save_spec_notifies_team_members_from_manifest_approvers` — the test's repo record pointed at a path `NoopGitOps` never initialized on disk (git spawn → NotFound), and `git commit-tree` lacked an author identity in the test environment. Fix (test helper `commit_to_bare_repo_head`): `git init --bare` the recorded path when missing and pass explicit author/committer identity to `commit-tree`. Also fixed a real routing bug this exposed: the save flow matched manifest entries against the raw client-facing `specs/...` path, while manifest paths are stored relative to `specs/` — team routing never matched. The handler now normalizes (same strip the conflict check uses), so `team:slug` manifest approvers receive the approval card.
- `api::users::tests::team_crud_slug_lead_and_membership_flow` — duplicate team membership let the adapter's duplicate guard bubble up as 500. Fix: `add_team_member` pre-checks `list_members` and returns 409 (mirroring `reject_duplicate_slug`); the adapter guard remains as the race backstop.

### Test evidence

Executed in this sandbox (stage build wrapper; gyre-server test execution exceeds the 4 GiB budget and is deferred to the verifier):

- `cargo check -p gyre-server --all-targets` — clean (includes all test targets).
- `cargo clippy -p gyre-server --all-targets` + changed-line analysis vs base `f98b17925` — 0 warnings/errors on changed lines.
- Mechanical gates pass: check-arch, check-abac-route-registry, check-abac-exempt-handlers, check-mcp-write-tools, check-migration-versions, check-migration-sql-portability, check-mem-port-contracts, check-byte-slice-truncation, check-relative-path-defaults, check-fail-open-ref-resolution, check-fabricated-scope-defaults, check-lossy-secret-conversion, check-scope-literal-defaults, check-inert-enforcement, check-forged-scope-fields, check-forwarded-header-trust, check-in-memory-state-stores, check-unbounded-external-http, check-dead-message-kinds, check-task-commit-attribution, check-rustfmt-diff (base `f98b17925`).
- Prior rounds (recorded history): `cargo test -p gyre-domain` 478 passed; `cargo test -p gyre-adapters` 475 passed, 6 ignored (team repository + migration 000062 backfill tests green).

Verifier must execute (gyre-server, blocked in this sandbox by the build wrapper):

- `cargo test -p gyre-server --lib api::users::tests::team` — API flow (slug derivation, lead validation, membership add/remove with `added_at`, duplicate membership 409, duplicate slug 409, invalid slug 400, non-member lead/member 400, workspace scoping 404, Developer 403).
- `cargo test -p gyre-server --lib api::specs::tests::team_slug_human_approvers_resolve_through_membership_e2e` — member approves, non-member stays Pending, unresolvable team fails closed.
- `cargo test -p gyre-server --lib api::specs_assist::tests::save_spec_notifies_team_members_from_manifest_approvers` — Viewer team member receives `SpecApprovalRequested` via `team:platform` routing.
- `cargo test -p gyre-server --lib api::budget::tests::budget_by_team_aggregates_usage_through_membership`
- `cargo test -p gyre-server --lib mem::team_contract_tests`
- Full `cargo test --all` (CI).
