---
title: "Implement user management UI pages"
spec_ref: "user-management.md §UI Pages"
depends_on:
  - task-124
  - task-125
progress: ready-for-review
coverage_sections:
  - "user-management.md §UI Pages"
commits: ["14f27ab5f4ed0ca56d6d59dd4f2503157b4f8a19", "f5aee7dab7577d13d3698e29818a837de63adad0", "b02f8f185cf6fecddee1470c779c1dd1d38b39a5", "e468155e73374c55968e7ea3b956c8b80227bf2f", "c5902874c8d6692ed9ab6c1544ffd1daf2380839", "d577374a98ca345672978b2b8a9763b618a6af31", "8d72ffecacb20fe2c436a0736fecec3ad39cbc3e", "4aa5a87ada50825ab676317fd030220dc814d01e", "64c85ec9c91d105a34196ce0d370036ca70b801a", "50f338e93383f6a4d59c408a3676d63aac85b45b", "1deff0b561bed7e78f2a0ece63bf2d79ac593dc2", "b696609378236716366ce85ef42a9b9a2c65d92c", "fe84485b4da230b8e3ace0840e68898cdcab5a7c", "0f127bc0e111c65ace292e45231ab585242aad99", "4de47315cc28796662ba04fb99cc36403390979a", "1a32b4e6422a0533c5a5075b4991a2163918a76f", "80641b880eb540870f63bbba7d9a1c78faa986f1", "70d85873a96e01e563985a6688cbe771e0f993a5"]
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
(`web/src/lib/api.js`): User Settings (profile editing, preferences,
notification channels) and Session Management in `UserProfile.svelte`;
User Profile at `/@username` (`PublicUserProfile.svelte`); Tenant User
Management in `TenantSettings.svelte`; Workspace Members/Invitations,
ownership transfer, and Team Management in `WorkspaceSettings.svelte`;
Invitation Accept at `/invite/{workspaceId}` (`InvitationAccept.svelte`);
Notification Drawer with bell unread badge, priority grouping, and deep
links (`NotificationDrawer.svelte`, `NotificationBell.svelte`,
`notifications.svelte.js`); Notification Preferences in the profile's
Notification Preferences tab and `NotificationSettings.svelte`. The
drawer's active-only listing is backed by the real `list_active_for_user`
query (`gyre-adapters/src/sqlite/notification.rs`), which excludes
resolved and dismissed rows in SQL while `list_for_user` keeps the full
Inbox contract.

### This round (merge repair against main head 6f5f4082)

The published candidate (486ab25f) conflicted with current main.
Resolved by merging main (6f5f4082, task-185 LLM-Assisted Spec Editing)
into the task-127 branch as merge commit 70d85873:

- **Source resolution:** main's changes since the merge base
  (047d6524) touch only task-185 surfaces — `SpecChatInput.svelte`,
  `SpecSuggestion.svelte`, `specAssist.js`, `DetailPanel.svelte`,
  `EditorSplit.svelte` and their tests — none of which task-127
  modified, so all sources merged cleanly with both implementations
  intact (verified: disjoint file sets, task-127 components present in
  the merged tree).
- **Dist resolution:** conflicts were confined to generated
  `web/dist` assets, resolved to the assignment-base (main) versions;
  final integration rebuilds and publishes those assets remotely.

Test evidence this round (retained under /tmp/stage/review-evidence):

- `vitest-merged-tree.log`: full web suite on the merged tree —
  124 test files / 2470 tests passed, 41 skipped, exit 0. This is the
  first probe covering both task-127 and task-185 surfaces together.

Server-side suites (`cargo test --all`) and full-history mechanical
checks (attribution script) are owned by remote verification; the
attribution script needs full git history and timed out in this
sandbox (same restriction recorded in prior rounds). No new
attribution risk: the only new non-merge commit is this process-only
task-file update, and merge commits are out of the check's scope.

Remaining open for this section (unchanged from prior rounds): Tenant
Invitation Management page (task-110's `/tenant/invite`,
`/tenant/invite/bulk`, `/tenant/invitations`, revoke endpoints are
in-tree but no page consumes them), the `/invite/{token}` magic-link
accept page (token accept/decline endpoints exist in-tree; there is no
unauthenticated token-preview endpoint for the specced workspace
preview, and the local-mode accept must display the minted first
credential), and inline global-role editing (no endpoint).
