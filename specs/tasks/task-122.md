---
title: "Enhance team management with lead, slug, and integration points"
spec_ref: "user-management.md §Team Management"
depends_on:
  - task-121
progress: ready-for-review
coverage_sections:
  - "user-management.md §Team Management"
commits: ["9d520866bc8ea06c74f113c41b82c6a8e4010233", "3056a7edd757b9d36b4e7c0e485951bc715755fd", "a2b86d834da6710c12e34301c96434d2bd4a414f", "abaee5ea1e3f5e945f236f58b0b64609f2c5727b", "bbd72bc37ffd0bd4b286171ad73d236fca06c64b"]
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

- Team CRUD + membership endpoints authenticate, scope to the caller's tenant (`load_workspace_scoped`), gate on Owner/Admin/TenantAdmin, and verify the path's workspace owns the team (cross-workspace team id → 404). `lead` is validated against workspace membership (400), slug collisions → 409. `POST .../teams/:team_id/members` (duplicate → 409, returns `added_at`) and `DELETE .../members/:user_id` (idempotent 204); routes registered with ABAC resolver entries.

### Integration points (user-management.md §Team Management)

- **Notification routing (`team:slug`)**: `resolve_team_members` (notifications.rs) resolves `team:{slug}` references to real member ids via `find_by_slug` + `list_members`; unresolvable references log and skip — never fabricate recipients. Wired into the `SpecApprovalRequested` emitter (specs_assist save flow).
- **Spec ownership (`team:slug` approvers)**: `resolve_new_approval_status` (api/specs.rs) expands `team:{slug}` entries in a manifest's `human_approvers` into the approver-id forms members' approvals record, before the pure §9 resolver runs. An unresolvable reference (or memberless team) is kept verbatim — no approval ever records a `team:...` approver id, so the resolver can never match it (fail-closed, stays Pending).
- **Budget reporting by team**: `GET /api/v1/workspaces/:id/budget/by-team` aggregates real `BudgetCallRecord` audit rows through team membership; unattributable records are reported under `unattributed`, not dropped.
- **Default reviewers** is NOT implemented: no spec models a repo→team ownership association (the `Repository` entity has no team field), so there is nothing to auto-assign against. Noted here rather than faked; needs a modeled repo-team ownership link first.

### Merge repair (this round, commit `9d520866b`)

Merging main's task-171 into this branch (merge `b8bf2110a`) added `prompt_template_sha` to `gyre_domain::BudgetCallRecord`; the by-team budget test's record initializer was resolved without the new field and the server test target failed to compile (verification round `2681105b5`, E0063 at budget.rs:790, exit 1). Fixed by initializing `prompt_template_sha: None` — the by-team aggregation is keyed on membership, not template provenance. No other task-122 surface was affected by the merge (diff vs candidate `fdbb8d36` touches only `specs_assist.rs` from task-171's unrelated prompt-template work, plus the domain budget/ports fields).

### Test evidence

Executed in this sandbox (stage build wrapper; gyre-server test execution exceeds the 4 GiB budget and is deferred to the verifier):

- `cargo check -p gyre-server --all-targets` — clean after the fix (was E0063 before it).
- `cargo test -p gyre-domain` — 478 passed, 8 doctests passed.
- `cargo test -p gyre-adapters team` — 6 passed (team repository + migration 000062 backfill); `cargo test -p gyre-adapters budget` — 3 passed.
- Prior rounds (recorded history): full `cargo test -p gyre-adapters` 475 passed, 6 ignored.

Verifier must execute (gyre-server, blocked in this sandbox by the build wrapper):

- `cargo test -p gyre-server budget_by_team` — the by-team aggregation test repaired this round.
- `cargo test -p gyre-server team` — team CRUD/membership/spec-ownership/notification-routing suites (`api::users::tests::team_crud_slug_lead_and_membership_flow`, `api::specs::tests::team_slug_human_approvers_resolve_through_membership_e2e`, `api::specs_assist::tests::save_spec_notifies_team_members_from_manifest_approvers`).
- `cargo test --all` (final gate).
