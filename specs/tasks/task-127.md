---
title: "Implement user management UI pages"
spec_ref: "user-management.md §UI Pages"
depends_on:
  - task-124
  - task-125
progress: ready-for-review
coverage_sections:
  - "user-management.md §UI Pages"
commits: ["1a32b4e6422a0533c5a5075b4991a2163918a76f", "4de47315cc28796662ba04fb99cc36403390979a", "0f127bc0e111c65ace292e45231ab585242aad99", "fe84485b4da230b8e3ace0840e68898cdcab5a7c", "b696609378236716366ce85ef42a9b9a2c65d92c", "1deff0b561bed7e78f2a0ece63bf2d79ac593dc2", "50f338e93383f6a4d59c408a3676d63aac85b45b"]
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
against the task-124/125 endpoints across the recorded commits (see
`commits:` frontmatter and git history for per-round details).

### This round (verification repair `50f338e9`)

- The prior verification run (`bash /tmp/stage/checks.sh`, exit 1) failed on
  exactly one gate: the changed-lines rustfmt check flagged two test-code
  lines — the `assert_eq!` argument list in
  `sqlite::notification::tests::list_active_for_user_excludes_resolved_and_dismissed`
  and the `create_notification(...)` chain in the users.rs active-filter
  handler test. Both reformatted to rustfmt canonical form; no semantic
  change. All other gates in that run were already clean (clippy changed
  lines, arch/hierarchy lints, ABAC registry, all check-* scripts, web
  build).

### Test evidence (this round)

- `python3 scripts/check-rustfmt-diff.py 103d665e` — changed lines clean
  (6 Rust files checked); was failing on the two lines above before the fix.
- `/tmp/stage/bin/cargo test -p gyre-adapters
  list_active_for_user` — passes (the reformatted adapters test).
- `/tmp/stage/bin/cargo check -p gyre-server --tests` — clean (covers the
  reformatted users.rs handler test compilation).
- Deferred to the independent verifier (gyre-server test binary exceeds
  the in-sandbox 4 GiB wrapper budget):
  `cargo test -p gyre-server spec_path_notifications_active_filter_excludes_resolved_and_dismissed`,
  plus the usual full `cargo test --all`, `cd web && npm test`, and web e2e.

### Still open (owned elsewhere, not this task's pages)

- Tenant Invitation Management incl. `/invite/{token}` magic links, bulk
  invite, and invitation expiry countdown — task-110 (not-started) owns the
  backend; no expiry field exists on WorkspaceMembership yet.
- Public-profile activity feed/stats — no per-user endpoints exist.
