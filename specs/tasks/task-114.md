---
title: "Implement public user profile page (`/@{username}`)"
spec_ref: "user-management.md §User Profile Page (`/@{username}`)"
depends_on:
  - task-113
  - task-208
progress: not-started
coverage_sections:
  - "user-management.md §User Profile Page (`/@{username}`)"
commits: []
---

## Spec Excerpt

**RESCOPED (2026-09-30 PM cycle):** this task originally included the "My Dashboard" landing page with My Tasks / My MRs / My Agents sections. That model is superseded:
- The landing-page role is taken by `ui-navigation.md` §2 — "The workspace home is a dashboard... It's the landing page after selecting a workspace." No separate `/dashboard` page is to be built.
- My Tasks / My MRs / My Agents surfaces are **forbidden** in `/profile` by `human-system-interface.md` §12 ("What the Profile Is NOT"); task-208 removes them and amends `user-management.md` §"My Stuff" Views.

The remaining scope is the public user profile page only.

From `user-management.md` §User Profile Page (`/@{username}`):

Public within the tenant:
- Display name, username, avatar, timezone
- Workspace memberships and roles
- Team memberships
- Recent activity feed (public actions only)
- Stats: MRs reviewed, specs approved, agents spawned

## Implementation Plan

1. **Backend API:**
   - Add `GET /api/v1/users/{username}` — public profile endpoint (tenant-scoped)
   - Add `GET /api/v1/users/{username}/activity` — public activity feed (public actions only)
   - Add `GET /api/v1/users/{username}/stats` — aggregate stats: MRs reviewed, specs approved, agents spawned

2. **User Profile page (Svelte):**
   - Route: `/@{username}`
   - Profile header: avatar, display name, username, timezone
   - Workspace memberships with role badges
   - Team memberships
   - Activity feed (public actions)
   - Stats bar: counts of MRs reviewed, specs approved, agents spawned

3. **Navigation integration:**
   - User avatar in top nav links to profile
   - Profile pages accessible from @mentions and activity feeds

## Acceptance Criteria

- [ ] User profile page at `/@{username}`
- [ ] Profile shows workspace memberships, teams, activity, stats
- [ ] Profile is tenant-scoped (only visible to same-tenant users)
- [ ] Responsive layout
- [ ] `cargo test --all` and `npm test` pass

## Agent Instructions

Read `specs/system/user-management.md` §User Profile Page (`/@{username}`) — the "My Dashboard" section above it is superseded (see rescoping note). Public profile endpoints: add `GET /api/v1/users/{username}` (+`/activity`, `/stats`) in `crates/gyre-server/src/api/users.rs`; register routes in `crates/gyre-server/src/api/mod.rs`; add ABAC mappings in `crates/gyre-server/src/abac_middleware.rs`. The Svelte app entry is `web/src/App.svelte` (router is `parseUrl`/`urlFor`, App.svelte:183-270). User model: `crates/gyre-domain/src/user.rs`.

Note: `/profile` (private settings surface) already exists and is governed by HSI §12 — do not add activity-hub features there (no My Tasks/MRs/Agents, per the HSI §12 NOT-list). This task's `/@{username}` page is a separate, public tenant-scoped surface.
