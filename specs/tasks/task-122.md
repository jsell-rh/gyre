---
title: "Enhance team management with lead, slug, and integration points"
spec_ref: "user-management.md §Team Management"
depends_on:
  - task-121
progress: ready-for-review
coverage_sections:
  - "user-management.md §Team Management"
commits: ["bbd72bc37ffd0bd4b286171ad73d236fca06c64b", "abaee5ea1e3f5e945f236f58b0b64609f2c5727b"]
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

- `Team` (gyre-domain/src/team.rs) carries every spec field: `slug` (workspace-unique, URL-safe) and `lead` (user id), plus `validate_slug` (1–64 chars `[a-z0-9-]`, no edge/consecutive `-`; explicit invalid slugs → 400, never silently rewritten) and `sanitize_slug` (name → slug derivation; 400 when nothing URL-safe remains). `member_ids` is gone from the entity — membership is the spec's `TeamMembership { team_id, user_id, added_at }`, persisted in a new `team_memberships` table (composite PK).
- Migration `2026-11-13-000062_team_slug_lead_membership` (portable SQL, runs on SQLite + PostgreSQL): adds `slug`/`lead` to `teams` (backfill: slug ← team id, stable/unique/URL-safe; lead ← workspace Owner), creates `idx_teams_workspace_slug` (unique per workspace) and `team_memberships`, drops the superseded `member_ids` JSON column (it was never writable through any endpoint — no membership endpoints existed — so it was always `'[]'`).
- `TeamRepository` port gains `find_by_slug`, `add_member` (fails on duplicate membership), `remove_member` (idempotent), `list_members` (ordered by `added_at`), `list_for_user`. All three adapters implement the duplicate contracts: SQLite/PG via the unique index/composite PK with stable error labels, mem via in-code guards (check-mem-port-contracts passes). `delete` removes memberships with the team in every adapter.

### API (gyre-server/src/api/users.rs)

- Team CRUD endpoints now authenticate, scope to the caller's tenant (`load_workspace_scoped`), gate on Owner/Admin/TenantAdmin (`can_manage_membership`), and verify the path's workspace owns the team (cross-workspace team id → 404; previously `update`/`delete` ignored the workspace entirely). Create/update validate `lead` against real workspace membership (non-member → 400) and reject slug collisions → 409.
- New endpoints (user-management.md §API > Teams): `POST /api/v1/workspaces/:id/teams/:team_id/members` (member must be a workspace member — teams never expand access beyond workspace membership — else 400; duplicate → 409; returns `added_at`) and `DELETE .../members/:user_id` (idempotent 204). Routes registered in `api/mod.rs` with ABAC resolver entries in `abac_middleware.rs`.

### Integration points (user-management.md §Team Management)

- **Notification routing (`team:slug`)**: `resolve_team_members` (notifications.rs) parses `team:{slug}` references and resolves them to real member ids via `find_by_slug` + `list_members`; unresolvable references log and skip — never fabricate recipients. Wired into the `SpecApprovalRequested` emitter (specs_assist save flow): when the spec's registry manifest names a team in `human_approvers`, its members receive the approval card in addition to the role-based fan-out.
- **Spec ownership (`team:slug` approvers)**: `resolve_new_approval_status` (api/specs.rs) expands `team:{slug}` entries in a manifest's `human_approvers` into the approver-id strings those members' human approvals record, before the pure §9 resolver runs. A team member's approval approves the spec; a non-member's does not; an unresolvable team reference fails closed (stays Pending). spec-registry.md manifest schema documents the entry form.
- **Budget reporting by team**: `GET /api/v1/workspaces/:id/budget/by-team` (api/budget.rs, ABAC-registered) aggregates the workspace's real `BudgetCallRecord` audit trail: agent-caused records attribute to the agent's spawning user (via the agents store), then to every team that user belongs to; records with no attributable user are reported under `unattributed` rather than dropped.
- **Default reviewers** ("MRs in repos owned by team X") is NOT implemented: no spec models a repo→team ownership association (the `Repository` entity has no team field and no spec defines one), so there is nothing to auto-assign against. This needs a modeled repo-team ownership link first and remains open for a future task — noted here rather than faked.

### Test evidence

Implementation history: `bbd72bc37` (production code) and `abaee5ea1` (unused-import cleanup). The executor merged base `199c1bb78` (task-203 search scoping) as `f0b06a8cb`; the team production files are byte-identical to `bbd72bc37` after the merge (evidence: `/tmp/stage/review-evidence/task-122/team-files-diff-bbd72bc3-to-HEAD.txt`, empty).

Executed in this sandbox (via the stage build wrapper):

- `cargo test -p gyre-domain` — 478 passed, 0 failed (includes team slug validation/derivation and membership struct tests).
- `cargo test -p gyre-adapters` — full suite 475 passed, 0 failed, 6 ignored (post-merge integration run), including the SQLite `team` repository tests (slug round-trip via `find_by_slug`, duplicate-slug create/update rejection, duplicate-membership rejection, `added_at` ordering, delete-removes-memberships) and `migration_000062_backfills_team_slug_and_lead` (real pre-000062 DB → 000062 alone: slug/lead backfills, unique index rejects duplicates, `member_ids` dropped, composite PK rejects duplicate membership).
- `cargo check -p gyre-server --all-targets` — clean (includes server test targets).
- Mechanical gates pass: check-arch, check-migration-versions, check-migration-sql-portability, check-mem-port-contracts, check-abac-route-registry, check-abac-exempt-handlers, check-task-commit-attribution.

`cargo test --all` remains open: gyre-server test execution exceeds this sandbox's 4 GiB build budget (stage build wrapper restriction — infrastructure, not a code defect). Verifier must run:

- `cargo test -p gyre-server --lib api::users::tests::team` — API flow (slug derivation, lead validation, member add/remove with `added_at`, duplicate slug 409, invalid slug 400, non-member lead/member 400, workspace scoping 404, Developer 403).
- `cargo test -p gyre-server --lib api::specs::tests::team_slug_human_approvers_resolve_through_membership_e2e` — real git manifest + real team API + real API-key human approvals: team member approves, non-member and unresolvable-team cases stay Pending.
- `cargo test -p gyre-server --lib api::specs_assist::tests::save_spec_notifies_team_members_from_manifest_approvers` — a Viewer team member (outside the role-based fan-out) receives `SpecApprovalRequested` via `team:platform` routing.
- `cargo test -p gyre-server --lib api::budget::tests::budget_by_team_aggregates_usage_through_membership` — aggregation + unattributed bucket over real records.
- `cargo test -p gyre-server --lib mem::team_contract_tests` — mem adapter duplicate guards.
- Full `cargo test --all` (CI).
