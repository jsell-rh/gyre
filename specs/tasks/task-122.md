---
title: "Enhance team management with lead, slug, and integration points"
spec_ref: "user-management.md §Team Management"
depends_on:
  - task-121
progress: not-started
coverage_sections:
  - "user-management.md §Team Management"
commits: []
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
