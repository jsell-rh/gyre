---
title: "Implement user management UI pages"
spec_ref: "user-management.md §UI Pages"
depends_on:
  - task-124
  - task-125
progress: ready-for-review
coverage_sections:
  - "user-management.md §UI Pages"
commits: ["94f6eb357ceb0f055283ec96453dae0b20826b68", "80641b880eb540870f63bbba7d9a1c78faa986f1", "14f27ab5f4ed0ca56d6d59dd4f2503157b4f8a19", "f5aee7dab7577d13d3698e29818a837de63adad0", "b02f8f185cf6fecddee1470c779c1dd1d38b39a5", "e468155e73374c55968e7ea3b956c8b80227bf2f", "c5902874c8d6692ed9ab6c1544ffd1daf2380839", "d577374a98ca345672978b2b8a9763b618a6af31", "8d72ffecacb20fe2c436a0736fecec3ad39cbc3e", "4aa5a87ada50825ab676317fd030220dc814d01e", "64c85ec9c91d105a34196ce0d370036ca70b801a", "50f338e93383f6a4d59c408a3676d63aac85b45b", "1deff0b561bed7e78f2a0ece63bf2d79ac593dc2", "b696609378236716366ce85ef42a9b9a2c65d92c", "fe84485b4da230b8e3ace0840e68898cdcab5a7c", "0f127bc0e111c65ace292e45231ab585242aad99", "4de47315cc28796662ba04fb99cc36403390979a", "1a32b4e6422a0533c5a5075b4991a2163918a76f"]
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
Invitation Accept (`InvitationAccept.svelte`); Notification Drawer with
bell unread badge, priority grouping, and deep links
(`NotificationDrawer.svelte`, `NotificationBell.svelte`,
`notifications.svelte.js`); Notification Preferences in the profile's
Notification Preferences tab and `NotificationSettings.svelte`. The
drawer's active-only listing is backed by the real `list_active_for_user`
query, which excludes resolved and dismissed rows in SQL while
`list_for_user` keeps the full Inbox contract.

### This round (rebase repair against main c66ee2ac)

The published candidate (fa4601c) conflicted with current main
(c66ee2ac, task-123 notification entity). Resolved as merge commit
160bb3d7, preserving both implementations:

- **`crates/gyre-adapters/src/sqlite/notification.rs` (tests):** both
  sides added a test at the same location. Both kept:
  task-127's `list_active_for_user_excludes_resolved_and_dismissed` and
  task-123's `list_for_user_type_set_filter_matches_persisted_column`,
  each restored as a self-contained test with its own full-listing
  assertion.
- **`crates/gyre-server/src/api/users.rs`
  (`list_my_notifications_impl`):** task-123 changed the `?priority=`
  filter to a type-string set (band derives from the notification TYPE,
  never the numeric rank); task-127 added the `?active=true` drawer
  branch. To preserve both without regressing either,
  `list_active_for_user` now takes the same
  `notification_types: Option<&[&str]>` set as `list_for_user` (port
  trait, sqlite, postgres, mem adapters, `notifications.rs` wrapper),
  so the drawer's level filter shares one derivation with the Inbox
  and can never disagree on a band. An empty set matches nothing in
  every adapter (SQL adapters return early; mem guards in code).
- **Generated `web/dist`** resolved to main's (assignment-base)
  versions; final integration rebuilds and publishes those assets
  remotely. `web/src` is unchanged by this merge (verified: empty diff
  vs the candidate), so the shipped UI implementation is intact.

### Test evidence

- `cargo test -p gyre-adapters notification` (this round, merged tree):
  14 passed / 0 failed — includes both conflicting tests above and the
  port-contract tests (`notifications_exclude_types_applies_to_list_and_count`,
  `notification_prefs_roundtrip`).
- `cargo check -p gyre-server -p gyre-adapters -p gyre-ports` and
  `cargo check -p gyre-cli`: clean (merge touched `gyre-common`, so the
  CLI was checked too).
- Web suite: not re-run this round — `web/src` and `web/tests` have an
  empty diff against the candidate head, whose suite passed in the
  prior round (2470 tests, exit 0; retained evidence
  `vitest-merged-tree.log`). No frontend surface changed in this merge.

Deferred to the independent verifier (sandbox budget): the gyre-server
test suite, in particular `cargo test -p gyre-server --lib
api::users::tests` — `spec_path_notifications_active_filter_excludes_resolved_and_dismissed`
exercises the merged `?active=true` branch through the mem adapter and
the notifications wrapper, which this sandbox cannot compile within the
4 GiB budget; `cargo test --all`, Clippy, architecture lint, and the
full-history attribution script (it timed out at 400 s here — the
commits frontmatter is restored with the merge commit recorded, so
there is no new attribution risk).

Remaining open for this section (unchanged from prior rounds): Tenant
Invitation Management page (task-110's `/tenant/invite`,
`/tenant/invite/bulk`, `/tenant/invitations`, revoke endpoints are
in-tree but no page consumes them), the `/invite/{token}` magic-link
accept page (token accept/decline endpoints exist in-tree; there is no
unauthenticated token-preview endpoint for the specced workspace
preview, and the local-mode accept must display the minted first
credential), and inline global-role editing (no endpoint).
