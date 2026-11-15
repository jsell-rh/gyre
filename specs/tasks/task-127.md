---
title: "Implement user management UI pages"
spec_ref: "user-management.md §UI Pages"
depends_on:
  - task-124
  - task-125
progress: ready-for-review
coverage_sections:
  - "user-management.md §UI Pages"
commits: ["4aa5a87ada50825ab676317fd030220dc814d01e", "64c85ec9c91d105a34196ce0d370036ca70b801a", "50f338e93383f6a4d59c408a3676d63aac85b45b", "1deff0b561bed7e78f2a0ece63bf2d79ac593dc2", "b696609378236716366ce85ef42a9b9a2c65d92c", "fe84485b4da230b8e3ace0840e68898cdcab5a7c", "0f127bc0e111c65ace292e45231ab585242aad99", "4de47315cc28796662ba04fb99cc36403390979a", "1a32b4e6422a0533c5a5075b4991a2163918a76f"]
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
(runes) components against the task-124/125/111 endpoints — see the
`commits:` frontmatter history in git (earlier rounds recorded their commit
lists before this round's reset) and the detailed per-round notes in the
`specs/coverage/system/user-management.md` §UI Pages row. This round is a
review repair on top of that work.

### This round (review repair `4aa5a87a`)

The assigned review finding: the declined-state "Go to all workspaces"
button (data-testid="invite-go-all", `InvitationAccept.svelte`) was a
silent no-op in production — the component reads
`getContext('goToCrossWorkspace')`, but `App.svelte` (the only ancestor)
never provided that context, so a user declining an invitation was
stranded on the declined screen with a dead button.

- **Fix:** `App.svelte` now sets `setContext('goToCrossWorkspace', () =>
  goToCrossWorkspace())` alongside its sibling navigation contexts.
- **Same-path defect fixed:** the mount-time canonical `urlFor` rebuild
  dropped `inviteId`/`username`, so a full page load at `/invite/{ws}` or
  `/@{user}` rewrote the URL to `/invite/` / `/@`, destroying the deep
  link (state survived, refresh did not). The canonical call now carries
  the route parameter for those modes.
- **Test:** new `InvitationDeclineNavigate.test.js` renders the real
  `App.svelte` at `/invite/{ws}` (page-load path, api/ws mocked via the
  established `CrossWorkspaceEntry.test.js` harness pattern), declines
  the invitation, clicks go-to-all, and asserts navigation to `/all` with
  `cross_workspace` history state. Mutation-checked: fails with the fix
  stashed, passes with it applied.

### Test evidence (this round)

- `npm ci` (locked deps), then focused:
  `npx vitest run src/__tests__/InvitationDeclineNavigate.test.js
  src/__tests__/InvitationAccept.test.js` — 2 files, 6 tests passed.
- Full frontend suite `npm test`: 123 files passed, 2419 tests passed,
  41 skipped.
- No Rust changes this round; server suites from earlier rounds still
  apply for the verifier. Evidence retained at
  `/tmp/stage/review-evidence/task-127-repair.txt`.
