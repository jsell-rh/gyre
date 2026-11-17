---
title: "Implement user management UI pages"
spec_ref: "user-management.md §UI Pages"
depends_on:
  - task-124
  - task-125
progress: ready-for-review
coverage_sections:
  - "user-management.md §UI Pages"
commits: ["770378cc0687b56bf37abb52faf823de8f0670fd", "6d71565c5cb4a691d7ea0eddfdaeb50f59cba348", "1ed61cf1a71249c007a2dcf0bb59316e812b6d76", "94f6eb357ceb0f055283ec96453dae0b20826b68", "80641b880eb540870f63bbba7d9a1c78faa986f1", "14f27ab5f4ed0ca56d6d59dd4f2503157b4f8a19", "f5aee7dab7577d13d3698e29818a837de63adad0", "b02f8f185cf6fecddee1470c779c1dd1d38b39a5", "e468155e73374c55968e7ea3b956c8b80227bf2f", "c5902874c8d6692ed9ab6c1544ffd1daf2380839", "d577374a98ca345672978b2b8a9763b618a6af31", "8d72ffecacb20fe2c436a0736fecec3ad39cbc3e", "4aa5a87ada50825ab676317fd030220dc814d01e", "64c85ec9c91d105a34196ce0d370036ca70b801a", "50f338e93383f6a4d59c408a3676d63aac85b45b", "1deff0b561bed7e78f2a0ece63bf2d79ac593dc2", "b696609378236716366ce85ef42a9b9a2c65d92c", "fe84485b4da230b8e3ace0840e68898cdcab5a7c", "0f127bc0e111c65ace292e45231ab585242aad99", "4de47315cc28796662ba04fb99cc36403390979a", "1a32b4e6422a0533c5a5075b4991a2163918a76f"]
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

All §UI Pages rows ship as real Svelte 5 (runes) components consuming
the task-110/124/125 endpoints: User Settings + Session Management
(`UserProfile.svelte`, `/profile` — display name, timezone, locale,
theme/density/diff preferences, notification channel config, active
sessions with revoke), User Profile at `/@{username}`
(`PublicUserProfile.svelte`), Tenant User Management (Users tab in
`TenantSettings.svelte`, TenantAdmin-gated: list/search, invite,
deactivate, inline global-role editing via `PUT /users/{id}/role`),
Tenant Invitation Management (Invitations tab: status filter, single
+ bulk invite modals, revoke, one-time invite_url with copy),
Workspace Members/Invitations with role change and ownership transfer
(`WorkspaceSettings.svelte`), Team Management within workspace
settings, the Notification Drawer (bell with unread badge,
priority-grouped notifications, deep links, mark read/unread),
Notification Preferences, and the `/invite/{token}` magic-link accept
page (unauthenticated preview + accept/decline, local-mode first API
key displayed once; disjoint from the authenticated
`/invite/{workspaceId}` flow by token shape).

### Review-repair round (6aa0d0fc)

Both findings fixed in 770378cc:

- `WorkspaceSettings.svelte` `invitationExpiry()`: the sub-hour branch
  now computes `Math.floor(secs / 60)` — a pending invitation expiring
  in 59 minutes renders "Expires in 59m", not "Expires in 3540m".
- `UserProfile.svelte` edit-form Save and Cancel buttons now carry
  `type="button"`, so a Save click fires `saveEdit()` once (no
duplicate `PUT /users/me` via the form's default submit) and Cancel
  cannot trigger a spurious save.

### Contract-repair round (6d9a4b89)

The executor restored the assigned task document (Agent Instructions
now read `web/src/lib/` only); no production code changed in this
round — the 770378cc fix and all shipped surfaces were re-verified
intact on the restored checkout (invitation expiry division,
`type="button"` edit actions, TenantSettings Users/Invitations tabs
with inline role editing and bulk invite, and the
`/invite/{token}` magic-link preview/accept/decline flow).

### Test evidence

- `web/src/__tests__/WorkspaceSettings.test.js` (85 passed): the
  expiry test asserts a sub-hour pending row renders exactly
  `Expires in 30m` — previously verified to fail with `Expires in
  1800m` when the division is reverted.
- `web/src/__tests__/UserProfile.test.js` (38 passed): the test
  asserts one Save click issues exactly one `PUT /users/me` with the
  edited display name — previously verified to fail with two calls
  when `type="button"` is removed.
- Contract-repair round re-ran both suites plus the other task-surface
  suites (`InvitationAccept`, `InvitationDeclineNavigate`,
  `NotificationBell`, `NotificationDrawer`, `NotificationSettings`,
  `PublicUserProfile`, `TenantSettings`, `notifications-store`):
  224/224 passed via `npx vitest run` with the locked `npm ci`
  versions; full web suite and `cargo test --all` remain with the
  independent verifier per the build-budget rule.
