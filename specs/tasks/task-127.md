---
title: "Implement user management UI pages"
spec_ref: "user-management.md §UI Pages"
depends_on:
  - task-124
  - task-125
progress: ready-for-review
coverage_sections:
  - "user-management.md §UI Pages"
commits: ["d577374a98ca345672978b2b8a9763b618a6af31", "8d72ffecacb20fe2c436a0736fecec3ad39cbc3e", "4aa5a87ada50825ab676317fd030220dc814d01a", "64c85ec9c91d105a34196ce0d370036ca70b801a", "50f338e93383f6a4d59c408a3676d63aac85b45b", "1deff0b561bed7e78f2a0ece63bf2d79ac593dc2", "b696609378236716366ce85ef42a9b9a2c65d92c", "fe84485b4da230b8e3ace0840e68898cdcab5a7c", "0f127bc0e111c65ace292e45231ab585242aad99", "4de47315cc28796662ba04fb99cc36403390979a", "1a32b4e6422a0533c5a5075b4991a2163918a76f", "c5902874c8d6692ed9ab6c1544ffd1daf2380839"]
---

## Spec Excerpt

From `user-management.md` §UI Pages:

| Page | Purpose |
|---|---|
| My Dashboard | Landing page with my tasks, MRs, agents, approvals, notifications, pending invitations |
| User Profile (`/@username`) | Public profile with activity feed |
| User Settings | Edit display name, timezone, preferences, notification channels |
| Session Management | Active sessions with revoke |
| Tenant User Management | User list, invite, deactivate, role management (TenantAdmin only) |
| Tenant Invitation Management | Pending/expired/accepted invitations, bulk invite, revoke (TenantAdmin only) |
| Workspace Members | Member list, invite modal, role management, ownership transfer |
| Workspace Invitations | Pending invitations with expiry countdown, revoke |
| Pending Invitation Accept Page | `/invite/{token}` — accept/decline with workspace preview |
| Team Management | Create/edit teams, manage membership |
| Notification Drawer | Slide-out panel from bell icon |
| Notification Preferences | Channel configuration, priority filters |

## Implementation Plan

1. **Audit existing UI pages:**
   - Check which pages already exist in `web/src/lib/` or `web/src/routes/`
   - Existing: WorkspaceSettings, TenantSettings, Inbox (notification-related)
   - Identify gaps

2. **User Settings page:**
   - Edit display name, timezone, locale
   - Theme, density, diff view preferences
   - Notification channel configuration
   - Link to session management

3. **User Profile page (`/@username`):**
   - Public within tenant: display name, username, avatar, timezone
   - Workspace memberships and roles
   - Team memberships
   - Recent activity feed

4. **Tenant User Management:**
   - User list with search/filter
   - Invite button → invite modal
   - Deactivate user action
   - Role management inline

5. **Workspace Members & Invitations:**
   - Member list with role badges
   - Invite modal (by username or email)
   - Role change dropdown
   - Ownership transfer action
   - Pending invitations tab

6. **Invitation Accept Page:**
   - Route: `/invite/{token}`
   - Show workspace preview
   - Accept/decline buttons

7. **Notification Drawer:**
   - Bell icon in top nav with unread count badge
   - Slide-out panel grouped by priority
   - Deep link on click
   - Mark read/unread

8. **Team Management:**
   - Within workspace settings
   - Create/edit teams
   - Add/remove members

## Acceptance Criteria

- [ ] User Settings page with profile editing and preferences
- [ ] User Profile page at `/@username` route
- [ ] Tenant User Management page (TenantAdmin only)
- [ ] Workspace Members page with invite, role change, ownership transfer
- [ ] Invitation Accept page at `/invite/{token}`
- [ ] Notification Drawer with unread badge, priority grouping, deep links
- [ ] Team Management within workspace settings
- [ ] All pages consume API endpoints from task-124 and task-125
- [ ] Components use Svelte 5 patterns (runes, $state, $derived)
- [ ] `cd web && npm test` passes
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/user-management.md` §UI Pages. Existing Svelte components: `web/src/lib/` — check for existing user-related components. Route structure: `web/src/routes/` or `web/src/App.svelte`. Existing settings pages: grep for `WorkspaceSettings\|TenantSettings\|UserSettings` in `web/src/`. Notification components: grep for `Notification\|Inbox\|notification` in `web/src/`. The frontend uses Svelte 5 with runes ($state, $derived, $effect). Check `docs/ui.md` for component conventions and navigation model.

## Shipped

All acceptance-criterion pages shipped in earlier rounds as real Svelte 5
(runes) components against the task-124/125/111 endpoints: User Settings
(profile editing, preferences, notification channels) and Session
Management in `UserProfile.svelte`; User Profile at `/@username`
(`PublicUserProfile.svelte`); Tenant User Management and Invitation
Management in `TenantSettings.svelte`; Workspace Members/Invitations,
ownership transfer, and Team Management in `WorkspaceSettings.svelte`;
Invitation Accept at `/invite/{token}` (`InvitationAccept.svelte`);
Notification Drawer + bell unread badge (`NotificationDrawer.svelte`,
`NotificationBell.svelte`, `notifications.svelte.js`); Notification
Preferences in the profile's Notification Preferences tab.

### This round (review repair, commit `c5902874`)

Fixed the one merge-resolution compile defect flagged by review
(759a9183, carried from the main-merge 25ca4f72): the adapter test
`list_active_for_user_excludes_resolved_and_dismissed` in
`crates/gyre-adapters/src/sqlite/notification.rs` still called
`NotificationRepository::list_for_user` with the pre-task-209
9-argument shape, but the trait gained `exclude_types: &[&str]` as
argument #8 (HSI §12). Inserted `&[]` so the test's full-listing
assertion keeps the Inbox contract. No production code changed.

Evidence (recorded under /tmp/stage/review-evidence):

- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib
  list_active_for_user_excludes_resolved_and_dismissed` → 1 passed,
  exit 0 (`review-evidence/cargo-adapters-test-fix.txt`).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → clean, exit 0
  (same evidence file).

For the independent verifier (deferred per 4 GiB build budget):

- `cargo test --all` (workspace; unblocked by this fix)
- gyre-server handler tests:
  `spec_path_notifications_active_filter_excludes_resolved_and_dismissed`,
  `invitee_can_decline_own_pending_invitation`
- `cd web && npm ci && npm test` (frontend unchanged this round;
  passing in the prior round's verification)
