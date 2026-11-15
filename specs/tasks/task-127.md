---
title: "Implement user management UI pages"
spec_ref: "user-management.md §UI Pages"
depends_on:
  - task-124
  - task-125
progress: ready-for-review
coverage_sections:
  - "user-management.md §UI Pages"
commits: ["e468155e73374c55968e7ea3b956c8b80227bf2f", "c5902874c8d6692ed9ab6c1544ffd1daf2380839", "d577374a98ca345672978b2b8a9763b618a6af31", "8d72ffecacb20fe2c436a0736fecec3ad39cbc3e", "4aa5a87ada50825ab676317fd030220dc814d01e", "64c85ec9c91d105a34196ce0d370036ca70b801a", "50f338e93383f6a4d59c408a3676d63aac85b45b", "1deff0b561bed7e78f2a0ece63bf2d79ac593dc2", "b696609378236716366ce85ef42a9b9a2c65d92c", "fe84485b4da230b8e3ace0840e68898cdcab5a7c", "0f127bc0e111c65ace292e45231ab585242aad99", "4de47315cc28796662ba04fb99cc36403390979a", "1a32b4e6422a0533c5a5075b4991a2163918a76f", "b02f8f185cf6fecddee1470c779c1dd1d38b39a5", "a4847b26d3a78f0598671eef58966237fcd68b77"]
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

- [x] User Settings page with profile editing and preferences
- [x] User Profile page at `/@username` route
- [x] Tenant User Management page (TenantAdmin only)
- [x] Workspace Members page with invite, role change, ownership transfer
- [x] Invitation Accept page at `/invite/{token}`
- [x] Notification Drawer with unread badge, priority grouping, deep links
- [x] Team Management within workspace settings
- [x] All pages consume API endpoints from task-124 and task-125
- [x] Components use Svelte 5 patterns (runes, $state, $derived)
- [x] `cd web && npm test` passes
- [ ] `cargo test --all` passes (deferred to the independent verifier per the 4 GiB build wrapper)

## Agent Instructions

Read `specs/system/user-management.md` §UI Pages. Existing Svelte components: `web/src/lib/` — check for existing user-related components. Route structure: `web/src/routes/` or `web/src/App.svelte`. Existing settings pages: grep for `WorkspaceSettings\|TenantSettings\|UserSettings` in `web/src/`. Notification components: grep for `Notification\|Inbox\|notification` in `web/src/`. The frontend uses Svelte 5 with runes ($state, $derived, $effect). Check `docs/ui.md` for component conventions and navigation model.

## Shipped

All acceptance-criterion pages ship as real Svelte 5 (runes) components
consuming the task-124/125/111 endpoints (`web/src/lib/api.js`): User
Settings (profile editing, preferences, notification channels) and Session
Management in `UserProfile.svelte`; User Profile at `/@username`
(`PublicUserProfile.svelte`); Tenant User Management in
`TenantSettings.svelte`; Workspace Members/Invitations, ownership
transfer, and Team Management in `WorkspaceSettings.svelte`; Invitation
Accept at `/invite/{workspaceId}` (`InvitationAccept.svelte`);
Notification Drawer with bell unread badge, priority grouping, and deep
links (`NotificationDrawer.svelte`, `NotificationBell.svelte`,
`notifications.svelte.js`); Notification Preferences in the profile's
Notification Preferences tab and `NotificationSettings.svelte`. The
drawer's active-only listing is backed by the real `list_active_for_user`
query (`gyre-adapters/src/sqlite/notification.rs`), which excludes
resolved and dismissed rows in SQL while `list_for_user` keeps the full
Inbox contract.

### This round (rebase resolution, merge commit `a4847b26`)

The published task-127 head (b1b58451) conflicted with current main
(6cacb4bf), which had since landed task-114's independently-approved
public user profile page — the two sides both implemented the
`/@{username}` surface from the common 81c24bf0 base. Resolution,
preserving both feature sets:

- **Route model unified on main's `user_profile` mode** + `goToUserProfile`
  context: main's breadcrumb, avatar-menu "Public profile" link, and
  `selfUsername` state are kept; task-127's consumers (own-profile link in
  `UserProfile.svelte`, user-list links in `TenantSettings.svelte`) and
  the drawer's `navigateToPath` deep-link machinery are re-pointed to the
  unified route. Task-127's `/invite/{workspaceId}` route, `applyParsed`,
  and drawer deep links are unchanged.
- **`PublicUserProfile.svelte` + test: main's task-114 implementation
  wins** — it consumes the `/users/{username}/teams`, `/activity`, and
  `/stats` endpoints that landed with this merge (exactly the
  "public-profile activity feed/stats (no endpoint)" gap row 31
  previously recorded); task-127's client-side team derivation is dropped
  in favor of the server-side `teams` field. Task-127's 404 containment
  behavior is preserved (main's version shares it).
- **Locales:** the textual auto-merge produced a duplicate top-level
  `public_profile` key (both sides' blocks); deduplicated to main's keys —
  every key referenced by the retained components verified present.
- **Backend auto-merges verified:** `?active=true` drawer listing branch
  and invitee self-decline on
  `DELETE /workspaces/{id}/invitations/{invitation_id}` layered cleanly
  onto main's task-114 handlers (`cargo check -p gyre-server` clean);
  `list_active_for_user` mem adapter intact.
- **Coverage matrix union recount:** row 23 (User Profile Page)
  implemented by task-114; row 31's former activity-feed/stats gap closed
  by task-114's endpoints; 16 implemented / 9 task-assigned; row 31 stays
  task-assigned pending task-110 (tenant invitations, `/invite/{token}`
  magic links, bulk invite, expiry countdown) and inline global-role
  editing (no endpoint).
- **web/dist** carried at main's (assignment-base) state, content-equal to
  the executor's restore; final integration rebuilds and publishes.

Evidence (recorded under /tmp/stage/review-evidence):

- `cd web && npm ci && npm test` → 123 files / 2435 passed, 41 skipped,
  exit 0 (merged tree, pre- and post-commit).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → clean, exit 0.
- `scripts/check-abac-route-registry.sh`, `check-arch.sh`,
  `check-mem-port-contracts.sh`, `check-dead-message-kinds.sh` → all OK.
- `scripts/check-task-commit-attribution.sh` → no unattributed task-127
  commits after restoring the frontmatter list.

For the independent verifier (deferred per 4 GiB build budget):

- `cargo test --all` (workspace)
- gyre-server handler tests: user/session/invitation/team endpoints,
  `spec_path_notifications_active_filter_excludes_resolved_and_dismissed`,
  task-114 profile/activity/stats suites
- `cd web && npm ci && npm test`
