---
title: "Implement user management UI pages"
spec_ref: "user-management.md §UI Pages"
depends_on:
  - task-124
  - task-125
progress: ready-for-review
coverage_sections:
  - "user-management.md §UI Pages"
commits: ["1deff0b561bed7e78f2a0ece63bf2d79ac593dc2", "b696609378236716366ce85ef42a9b9a2c65d92c", "fe84485b4da230b8e3ace0840e68898cdcab5a7c", "0f127bc0e111c65ace292e45231ab585242aad99", "4de47315cc28796662ba04fb99cc36403390979a", "1a32b4e6422a0533c5a5075b4991a2163918a76f"]
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
against the task-124/125 endpoints (prior rounds; see git history), and this
repair round (`1deff0b5`) closes both review findings on the Notification
Drawer page.

### This round (review repair `1deff0b5`)

- **Bell unread badge** — the top-nav bell badge now shows the unread count
  from `GET /api/v1/notifications/unread-count` (`api.unreadNotificationCount`,
  previously defined but consumed by no component) instead of the legacy
  active-decisions count; it refreshes on init, workspace change, the 60s
  interval, and on drawer close (an `$effect` cleanup on the open→false
  transition) so read-state changes made inside the drawer are reflected.
  `decisionsCount` remains only as the workspace home Decisions section prop.
  i18n keys moved to `topbar.unread_notifications`/`no_unread_notifications`;
  the bell testid is `notification-bell` (e2e locator updated).
- **Bell-click spec reconciliation** — `user-management.md` §In-App
  Notification UI / §UI Pages (drawer slides out from the bell, unread badge)
  supersedes `ui-navigation.md` §1's old navigate-to-Decisions click; recorded
  as a dated spec-lifecycle amendment in `ui-navigation.md` (principle 4, Top
  Bar bullet + amendment note, repo-mode bullet, Daily/Exception flows), and
  `docs/ui.md` matches. No unamended contradiction remains.
- **Active-only drawer listing** — new `list_active_for_user` port method
  (same filters/pagination as `list_for_user` plus `resolved_at IS NULL AND
  dismissed_at IS NULL`), implemented by the SQLite and PostgreSQL adapters,
  the mem adapter (code guard), and the `BroadcastingNotificationRepo`
  wrapper; `GET /api/v1/notifications?active=true` routes to it and the
  drawer requests it — a notification dismissed or resolved in the Inbox
  never reappears in the drawer. The default listing keeps the full row set
  the Inbox's show-dismissed toggle depends on.

### Test evidence

- `cd web && npm test`: 119 files / 2403 passed, 41 skipped (pre-existing) —
  includes three new AppShell bell tests (unread badge uses the unread-count
  endpoint and not the decisions count; bell click opens the drawer; badge
  refreshes on drawer close) and the drawer active-filter request-shape test.
- `cargo test -p gyre-adapters list_active_for_user`: passes
  (`list_active_for_user_excludes_resolved_and_dismissed` — resolved,
  dismissed excluded; read-but-active kept; `list_for_user` unchanged).
- `cargo check -p gyre-server --lib` and `--tests`: clean (covers the port,
  adapters, mem, broadcasting wrapper, handler dispatch, and the new handler
  test).
- Deferred to the independent verifier (gyre-server test binary exceeds the
  in-sandbox 4 GiB wrapper budget):
  `cargo test -p gyre-server spec_path_notifications_active_filter_excludes_resolved_and_dismissed`
  (new handler test for `?active=true`), plus the usual full
  `cargo test --all` and web e2e. Evidence:
  `/tmp/stage/review-evidence/task-127-repair-probes.md`.

### Still open (section stays task-assigned)

- Tenant Invitation Management incl. `/invite/{token}` magic links, bulk
  invite, and invitation expiry countdown — task-110 (not-started) owns the
  backend; no expiry field exists on WorkspaceMembership yet.
- Public-profile activity feed/stats — no per-user endpoints exist.
- Inline global-role editing — no endpoint.
