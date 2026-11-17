---
title: "Implement user management UI pages"
spec_ref: "user-management.md §UI Pages"
depends_on:
  - task-124
  - task-125
progress: ready-for-review
coverage_sections:
  - "user-management.md §UI Pages"
commits: ["193727227472ea00bc07d26068506f6e51a89023", "7d598cc9d7e5b4211fdb2821409475defa2549e1", "ed9cdd578be1db26eb850244d953805375bf8447", "72a4bd097379b58fb05fdb843ae79649b0390d3b", "770378cc0687b56bf37abb52faf823de8f0670fd", "6d71565c5cb4a691d7ea0eddfdaeb50f59cba348", "1ed61cf1a71249c007a2dcf0bb59316e812b6d76", "94f6eb357ceb0f055283ec96453dae0b20826b68", "80641b880eb540870f63bbba7d9a1c78faa986f1", "14f27ab5f4ed0ca56d6d59dd4f2503157b4f8a19", "f5aee7dab7577d13d3698e29818a837de63adad0", "b02f8f185cf6fecddee1470c779c1dd1d38b39a5", "e468155e73374c55968e7ea3b956c8b80227bf2f", "c5902874c8d6692ed9ab6c1544ffd1daf2380839", "d577374a98ca345672978b2b8a9763b618a6af31", "8d72ffecacb20fe2c436a0736fecec3ad39cbc3e", "4aa5a87ada50825ab676317fd030220dc814d01e", "64c85ec9c91d105a34196ce0d370036ca70b801a", "50f338e93383f6a4d59c408a3676d63aac85b45b", "1deff0b561bed7e78f2a0ece63bf2d79ac593dc2", "b696609378236716366ce85ef42a9b9a2c65d92c", "fe84485b4da230b8e3ace0840e68898cdcab5a7c", "0f127bc0e111c65ace292e45231ab585242aad99", "4de47315cc28796662ba04fb99cc36403390979a", "1a32b4e6422a0533c5a5075b4991a2163918a76f"]
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
(`UserProfile.svelte`, `/profile`), User Profile at `/@{username}`
(`PublicUserProfile.svelte`), Tenant User Management (Users tab in
`TenantSettings.svelte`, TenantAdmin-gated, including inline global-role
editing via `PUT /users/{id}/role`), Tenant Invitation Management
(Invitations tab: status filter, single + bulk invite, revoke, one-time
invite_url with copy), Workspace Members/Invitations with role change
and ownership transfer (`WorkspaceSettings.svelte`), Team Management
within workspace settings, the Notification Drawer (bell with unread
badge, priority grouping, deep links, mark read/unread), Notification
Preferences, and the `/invite/{token}` magic-link accept page
(unauthenticated preview + accept/decline, local-mode first API key
displayed once; disjoint from the authenticated `/invite/{workspaceId}`
flow by token shape).

### Rebase-repair round (merge 39d59028)

Resolved the assigned rebase onto base e8f73c06 (task-082 navigation
rail). The only content conflict was the `docs/ui.md` shell-layout
paragraph; resolved by combining this task's notification-bell top-bar
wording with task-082's per-mode navigation-rail drawer model. The
auto-merged `App.svelte` was verified to render both the NavRail
(task-082) and the NotificationBell (task-127) with `REPO_TAB_IDS` as
the shared repo-tab source of truth; `ui-navigation.md` keeps both the
bell amendment (§1) and the rail shipped-status amendment (§1a).

### Verification-repair round (commit 19372722)

The verification full-suite run (attempt ed736d08) failed one test:
`api::invitations::tests::preview_resolves_tenant_invitation_with_
preassigned_workspaces` panicked unwrapping a missing `invite_url`.
Root cause was a test bug, not a production regression: the test
invited with `workspace_roles: ["Maintainer"]`, a role that exists
nowhere in the spec (user-management.md §Roles: Owner/Admin/Developer/
Viewer) nor in `WorkspaceRole::parse_role`, so `POST /api/v1/tenant/
invite` correctly rejected it with 400 InvalidInput. The Rust code on
this branch is byte-identical to the previously verified state
(`git diff 1489f740 39d59028 -- crates/` is empty). The test now uses
the spec-real `Admin` role (the spec's own invite example) with the
same exact round-trip assertion, plus an assert that the invite
response is 201 with the body in the failure message so a rejected
invite fails diagnosably. The `InvitationAccept` frontend mock fixture
used the same fictional role and now uses `Admin` — the preview API
can only ever emit real workspace roles.

### Test evidence

- Post-merge focused vitest (locked deps, `npm ci`): AppShell, NavRail,
  NoSidebar, NotificationBell, NotificationDrawer (120/120);
  InvitationAccept, InvitationDeclineNavigate, UserProfile,
  PublicUserProfile, TenantSettings, WorkspaceSettings, Inbox,
  BidirectionalNav (290/290); WorkspaceDrawerSectionNav,
  WorkspaceHome, WorkspaceHomeSections, CrossWorkspaceHome (85 passed,
  41 pre-existing skipped). Evidence:
  `/tmp/stage/review-evidence/probe-task127-rebase-merge.md`.
- Verification-repair round: `npx vitest run
  src/__tests__/InvitationAccept.test.js` — 15/15 passed after the
  fixture role fix; `cargo check -p gyre-server --lib --tests` clean;
  `cargo fmt --check -p gyre-server` clean. Evidence:
  `/tmp/stage/review-evidence/probe-task127-test-repair.md`.
- Full web suite and `cargo test --all` belong to independent
  verification. The verifier must run
  `cargo test -p gyre-server --lib api::invitations::tests::preview`
  (the repaired test is
  `preview_resolves_tenant_invitation_with_preassigned_workspaces`)
  before the full suite.
