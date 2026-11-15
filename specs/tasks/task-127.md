---
title: "Implement user management UI pages"
spec_ref: "user-management.md §UI Pages"
depends_on:
  - task-124
  - task-125
progress: ready-for-review
coverage_sections:
  - "user-management.md §UI Pages"
commits: ["64c85ec9c91d105a34196ce0d370036ca70b801a", "237f9a5b8ec9a98e619fc44e5533968d809dc154", "50f338e93383f6a4d59c408a3676d63aac85b45b", "1deff0b561bed7e78f2a0ece63bf2d79ac593dc2", "b696609378236716366ce85ef42a9b9a2c65d92c", "fe84485b4da230b8e3ace0840e68898cdcab5a7c", "0f127bc0e111c65ace292e45231ab585242aad99", "4de47315cc28796662ba04fb99cc36403390979a", "1a32b4e6422a0533c5a5075b4991a2163918a76f"]
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

All acceptance-criterion pages ship as real Svelte 5 (runes) components
against the task-124/125/111 endpoints across the recorded commits (see
`commits:` frontmatter and git history for per-round details).

### This round (rebase repair `237f9a5b`)

The assignment was a rebase repair: main's task-113 head `b8d63b0`
(in-app notification UI & email templates) had to be integrated into this
branch, and both sides had independently built a notification bell +
drawer (add/add conflicts on `NotificationDrawer.svelte` and its test;
content conflicts in `App.svelte`, `api.js`, `en.json`, and the two
coverage files). The merge reconciles them into one surface preserving
both feature sets:

- **Bell:** task-113's `NotificationBell` component (shared
  `notificationState` store, realtime `NotificationPush` bumps on the
  app's single `/ws` connection, unread-count endpoint badge) is the
  single top-bar bell, per this task's spec-lifecycle amendment to
  `ui-navigation.md` §1 (the old decisions-count badge click is
  superseded; the duplicate inline bell this branch had added to
  `App.svelte` is removed). The candidate's 60s full-resync interval was
  kept (drift guard alongside the realtime pushes).
- **Drawer:** one `NotificationDrawer` merging task-113's filter bar
  (type/priority/workspace/repo driving the task-125 list filters),
  realtime reload on push, live-connection dot and preferences link with
  task-127's active-only listing (`?active=true` → `list_active_for_user`,
  server-enforced), unread pill, `InvitationReceived` → `/invite/{ws}`
  deep link, and no-fabricated-target disable. Deep links route through
  the app's `navigateToPath` when rendered in the app, falling back to
  pushState + popstate replay.
- **API client:** `markNotificationRead` uses the spec PUT method (POST
  remains a server-side legacy alias); task-113's spec-path preferences
  client (`notificationChannels`/`updateNotificationChannels`) kept.
- **Routes:** `App.svelte` keeps both sides' modes — `/@username`,
  `/invite/:id` (task-127) and `/settings/notifications` (task-113) —
  through parseUrl/urlFor/pushState/popstate/title handling.
- **Coverage:** `specs/coverage/system/user-management.md` header and
  rows 19/31 carry a dated merge-reconciliation note; `SUMMARY.md`
  regenerated mechanically via `scripts/update-coverage-summary.sh`
  (user-management now 10 task-assigned / 15 implemented — rows 19/20
  are task-113's, landed via this merge).

### Test evidence (this round)

- `cd web && npm ci && npm test` — **2418 passed**, 41 skipped, 122 files
  (includes the merged NotificationDrawer suite covering both sides'
  assertions: active-only request shape, priority grouping, unread pill,
  read/unread/bulk endpoints, action_url + InvitationReceived deep links,
  disabled no-target items, filter pass-through; plus NotificationBell,
  notifications-store, NotificationSettings, and the AppShell bell tests
  re-pinned to the unread-count endpoint via the store's fetch).
- `/tmp/stage/bin/cargo check -p gyre-server` — clean.
- `/tmp/stage/bin/cargo check -p gyre-server --tests` — clean (covers the
  auto-merged `users.rs`/`notifications.rs` test code).
- `/tmp/stage/bin/cargo test -p gyre-adapters list_active_for_user` —
  passes (`list_active_for_user_excludes_resolved_and_dismissed`, real
  SQLite storage).
- Deferred to the independent verifier (gyre-server test binary exceeds
  the in-sandbox 4 GiB wrapper budget):
  `cargo test -p gyre-server spec_path_notifications_active_filter_excludes_resolved_and_dismissed`,
  plus the usual full `cargo test --all` and web e2e.

### Still open (owned elsewhere, not this task's pages)

- Tenant Invitation Management incl. `/invite/{token}` magic links, bulk
  invite, and invitation expiry countdown — task-110 (not-started) owns the
  backend; no expiry field exists on WorkspaceMembership yet.
- Public-profile activity feed/stats — no per-user endpoints exist.
