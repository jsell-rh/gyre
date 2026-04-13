---
title: "Implement My Dashboard & user profile page"
spec_ref: "user-management.md §My Dashboard"
depends_on:
  - task-113
progress: not-started
coverage_sections:
  - "user-management.md §My Dashboard (Landing Page After Login)"
  - "user-management.md §User Profile Page (`/@{username}`)"
commits: []
---

## Spec Excerpt

From `user-management.md` §My Dashboard (Landing Page After Login):

| Section | Content |
|---|---|
| My Tasks | Tasks assigned to me, grouped by status |
| My MRs | MRs I authored (or was spawned to create), with gate/merge status |
| My Agents | Agents I spawned, with status and budget usage |
| Pending Approvals | Specs and personas waiting for my approval |
| My Notifications | Unread notifications, prioritized |
| Recent Activity | My recent activity across all workspaces |

From §User Profile Page (`/@{username}`):

Public within the tenant:
- Display name, username, avatar, timezone
- Workspace memberships and roles
- Team memberships
- Recent activity feed (public actions only)
- Stats: MRs reviewed, specs approved, agents spawned

## Implementation Plan

1. **Backend API:**
   - `GET /api/v1/users/me` already exists — verify it returns all needed fields
   - `GET /api/v1/users/me/agents` — verify returns spawned agents with status/budget
   - `GET /api/v1/users/me/tasks` — verify returns assigned tasks grouped by status
   - `GET /api/v1/users/me/mrs` — verify returns authored MRs with gate status
   - Add `GET /api/v1/users/me/pending-approvals` if not exists — specs and personas awaiting approval
   - Add `GET /api/v1/users/{username}` — public profile endpoint (tenant-scoped)
   - Add `GET /api/v1/users/{username}/activity` — public activity feed
   - Add `GET /api/v1/users/{username}/stats` — aggregate stats

2. **My Dashboard page (Svelte):**
   - Route: `/dashboard` (or root `/` after login)
   - Six sections in a responsive grid layout
   - Each section fetches data from the corresponding API
   - Tasks section: grouped by status (InProgress, Review, Backlog)
   - MRs section: status badges (Open, Approved, Merged), gate status indicators
   - Agents section: status + budget usage bar
   - Approvals section: action buttons to navigate to approval
   - Notifications section: top 5 unread, link to full drawer
   - Activity section: recent timeline

3. **User Profile page (Svelte):**
   - Route: `/@{username}`
   - Profile header: avatar, display name, username, timezone
   - Workspace memberships with role badges
   - Team memberships
   - Activity feed (public actions)
   - Stats bar: counts of MRs reviewed, specs approved, agents spawned

4. **Navigation integration:**
   - After login, redirect to dashboard
   - User avatar in top nav links to profile
   - Profile pages accessible from @mentions and activity feeds

## Acceptance Criteria

- [ ] Dashboard page with 6 sections (tasks, MRs, agents, approvals, notifications, activity)
- [ ] Each section fetches real data from APIs
- [ ] Dashboard is the landing page after login
- [ ] User profile page at `/@{username}`
- [ ] Profile shows workspace memberships, teams, activity, stats
- [ ] Profile is tenant-scoped (only visible to same-tenant users)
- [ ] Responsive layout
- [ ] `cargo test --all` and `npm test` pass

## Agent Instructions

Read `specs/system/user-management.md` §My Dashboard and §User Profile Page. Existing "my stuff" endpoints: `GET /api/v1/users/me/{agents,tasks,mrs}` — grep for these in `gyre-server/src/api/`. The Svelte app entry is in `web/src/App.svelte` or `web/src/routes/`. Check the existing router setup. User model: `gyre-domain/src/user.rs`. Route registration: `api/mod.rs`. ABAC mappings: `abac_middleware.rs`.
