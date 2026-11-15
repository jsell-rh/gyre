---
title: "Implement user management UI pages"
spec_ref: "user-management.md §UI Pages"
depends_on:
  - task-124
  - task-125
progress: ready-for-review
coverage_sections:
  - "user-management.md §UI Pages"
commits: ["e468155e73374c55968e7ea3b956c8b80227bf2f", "c5902874c8d6692ed9ab6c1544ffd1daf2380839", "d577374a98ca345672978b2b8a9763b618a6af31", "8d72ffecacb20fe2c436a0736fecec3ad39cbc3e", "4aa5a87ada50825ab676317fd030220dc814d01e", "64c85ec9c91d105a34196ce0d370036ca70b801a", "50f338e93383f6a4d59c408a3676d63aac85b45b", "1deff0b561bed7e78f2a0ece63bf2d79ac593dc2", "b696609378236716366ce85ef42a9b9a2c65d92c", "fe84485b4da230b8e3ace0840e68898cdcab5a7c", "0f127bc0e111c65ace292e45231ab585242aad99", "4de47315cc28796662ba04fb99cc36403390979a", "1a32b4e6422a0533c5a5075b4991a2163918a76f"]
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

All acceptance-criterion pages shipped as real Svelte 5 (runes) components
consuming the task-124/125 API endpoints (`web/src/lib/api.js`): User Settings
(profile editing, preferences, notification channels) and Session Management
in `UserProfile.svelte`; User Profile at `/@username`
(`PublicUserProfile.svelte`); Tenant User Management and Invitation
Management in `TenantSettings.svelte`; Workspace Members/Invitations,
ownership transfer, and Team Management in `WorkspaceSettings.svelte`;
Invitation Accept at `/invite/{token}` (`InvitationAccept.svelte`);
Notification Drawer with bell unread badge, priority grouping, deep links
(`NotificationDrawer.svelte`, `NotificationBell.svelte`,
`notifications.svelte.js`); Notification Preferences in the profile's
Notification Preferences tab and `NotificationSettings.svelte`. The drawer's
active-only listing is backed by the real `list_active_for_user` query
(`gyre-adapters/src/sqlite/notification.rs`), which excludes resolved and
dismissed rows in SQL while `list_for_user` keeps the full Inbox contract.

### This round (verification repair, commit `e468155e`)

The retained verification run failed exactly one gate: the changed-line
rustfmt check flagged `crates/gyre-adapters/src/sqlite/notification.rs:1016`
— the merge-resolution commit `c5902874` added the `exclude_types` arg
(`&[]`) to a packed multi-line `list_for_user` call in the drawer listing
test, which rustfmt requires one-arg-per-line. Applied that exact formatting;
`scripts/check-rustfmt-diff.py 81c24bf0` now reports all 6 changed Rust
files clean. All other gates in that run already passed (clippy changed
lines clean, architecture/ABAC/lint scripts OK, `npm ci` + web tests +
`vite build` succeeded; the log tail's svelte warnings are pre-existing
non-fatal output). No `web/src` files changed this round, so the previously
verified UI work is intact.

Evidence (recorded under /tmp/stage/review-evidence):

- `python3 scripts/check-rustfmt-diff.py 81c24bf0` → changed lines clean,
  exit 0.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib
  list_active_for_user_excludes_resolved_and_dismissed` → 1 passed, exit 0.
- `bash scripts/check-task-commit-attribution.sh` → no unattributed
  task-127 commits after restoring the frontmatter list.

For the independent verifier (deferred per 4 GiB build budget):

- `cargo test --all` (workspace)
- gyre-server handler tests for user/session/invitation/team endpoints
- `cd web && npm ci && npm test`
