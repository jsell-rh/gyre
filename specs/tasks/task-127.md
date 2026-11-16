---
title: "Implement user management UI pages"
spec_ref: "user-management.md §UI Pages"
depends_on:
  - task-124
  - task-125
progress: ready-for-review
coverage_sections:
  - "user-management.md §UI Pages"
commits: ["b02f8f185cf6fecddee1470c779c1dd1d38b39a5", "e468155e73374c55968e7ea3b956c8b80227bf2f", "c5902874c8d6692ed9ab6c1544ffd1daf2380839", "d577374a98ca345672978b2b8a9763b618a6af31", "8d72ffecacb20fe2c436a0736fecec3ad39cbc3e", "4aa5a87ada50825ab676317fd030220dc814d01e", "64c85ec9c91d105a34196ce0d370036ca70b801a", "50f338e93383f6a4d59c408a3676d63aac85b45b", "1deff0b561bed7e78f2a0ece63bf2d79ac593dc2", "b696609378236716366ce85ef42a9b9a2c65d92c", "fe84485b4da230b8e3ace0840e68898cdcab5a7c", "0f127bc0e111c65ace292e45231ab585242aad99", "4de47315cc28796662ba04fb99cc36403390979a", "1a32b4e6422a0533c5a5075b4991a2163918a76f", "a0b7e9cf9c1d5b7153045a07080ba0521bc90bf6", "fdc9962e68bd9e9fe82997167a67b5372a4cf8f2"]
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

### This round (merge repair of main head e5df7847)

The assignment base moved to main head `e5df7847` (fix/weekend-
reconciliation PR), which carries task-110's tenant/workspace invitation
model and task-126's completed CLI. The rebase merge conflicted in
`crates/gyre-server/src/api/users.rs` and
`specs/coverage/system/user-management.md`; resolved in merge commit
`a0b7e9cf`:

- `api/users.rs`: task-124's parallel workspace-invitation handlers
  (membership-row list/accept/revoke + lifecycle test) are removed — the
  merged `api/mod.rs` registers those paths on `api/invitations.rs`
  (task-110's `WorkspaceInvitation` model, the assigned superset), and
  axum rejects duplicate registrations. No dangling references remain;
  the still-used membership helpers (`can_manage_membership`,
  `MembershipResponse`, `emit_membership_audit`) are untouched.
- Coverage matrix: merged recount (11 n/a, 0 not-started, 4
  task-assigned, 21 implemented); row 30 takes main's task-126
  completion; row 31 keeps task-127's notes re-audited for the merged
  contract.

The merge changed the invitation API contract under three task-127 UI
surfaces, repaired in `fdc9962e` (real contract breakage, not cosmetic):

- `InvitationAccept.svelte` (`/invite/{workspaceId}`) previously
  resolved the pending invitation as an unaccepted membership row in the
  tenant-public profile — that model no longer exists, so the page would
  always render "no pending invitation". It now resolves via the
  invitee-facing `GET /users/me/invitations` (Pending-status
  `WorkspaceInvitation` row), accepts via `POST
  /workspaces/{id}/invite/accept` (204), declines via `POST
  /workspaces/{id}/invite/decline` (the invitee self-decline DELETE
  route was removed with the parallel handlers), and shows the expiry
  countdown the new rows carry.
- `UserProfile.svelte` Invitations tab: same contract; filters to
  Pending rows (terminal statuses are not actionable).
- `WorkspaceSettings.svelte` invitations table: renders the status
  badge and expiry countdown from `WorkspaceInvitationResponse`, with
  revoke offered only for Pending rows (the endpoint 409s on terminal
  statuses).
- `api.js`: `request()` now attaches the HTTP status to thrown errors —
  the accept page's 404 not-found branch was previously dead in
  production (the thrown `Error` carried no `status`, and the old test
  mocked a property production never set).

Evidence this round (retained under /tmp/stage/review-evidence):

- `web-focused-vitest-round5.log`: after `npm ci`, focused vitest on
  the task-127 surfaces (UserProfile, PublicUserProfile, TenantSettings,
  InvitationAccept, InvitationDeclineNavigate, WorkspaceSettings,
  NotificationBell, NotificationDrawer, NotificationSettings,
  notifications-store, AppShell) → 11 files / 289 tests passed, exit 0.
  New distinguishing cases: an Expired invitation renders the none
  state (no accept button), terminal invitation rows get no revoke
  action, and the accept page shows the expiry countdown.
- `cargo-check-gyre-server-round5.log`: `cargo check -p gyre-server`
  at the merge resolution → finished clean, exit 0.
- `check-scripts-round5.log`: arch, ABAC route registry, migration
  versions, ABAC exempt handlers, dead message kinds, mem port
  contracts, MCP write tools, byte-slice truncation, fail-open ref
  resolution, scope-literal/fabricated-scope defaults, lossy secret
  conversion, inert enforcement, relative path defaults, forwarded
  header trust, unbounded external HTTP, forged scope fields,
  in-memory state stores, migration SQL portability → all OK.
- Environment note for remote verification: this sandbox's `awk` is
  mawk 1.3.4, so several `scripts/check-*.sh` that use gawk-specific
  constructs fail with awk syntax errors here (e.g.
  check-crypto-verify, check-tenant-filter); that is an infrastructure
  restriction, not a code defect. `check-dead-components.sh` flags 4
  pre-existing violations on main-side files untouched by this task
  (CanvasProbeStub, PipelineOverview, accordion helpers).

Remaining open for this section (kept task-assigned, row 31): Tenant
Invitation Management page (task-110's `/tenant/invite`,
`/tenant/invite/bulk`, `/tenant/invitations`, revoke endpoints are now
in-tree but no page consumes them), the `/invite/{token}` magic-link
accept page (token accept/decline endpoints exist in-tree; there is no
unauthenticated token-preview endpoint for the specced workspace
preview, and the local-mode accept must display the minted first
credential), and inline global-role editing (no endpoint).

Prior-round history retained as context: the published head was
previously rebased onto main when task-114's independently-approved
public profile page landed (route model unified on main's `user_profile`
mode + `goToUserProfile` context; main's `PublicUserProfile.svelte` won
over task-127's client-side team derivation), the task-113 bell/drawer
merge was reconciled (task-113's `NotificationBell` + shared store as
the single bell, both drawers' feature sets merged), and the round-4
contract-repair round verified the tree byte-identical to the round-3
full-suite state (full `npm test` 2435 passed at that head).

For the independent verifier (deferred per 4 GiB build budget):

- `cargo test --all` (workspace)
- gyre-server handler tests: user/session/invitation/team endpoints
  (task-110's `api::invitations` suite covers the invitation lifecycle
  through the routed paths),
  `spec_path_notifications_active_filter_excludes_resolved_and_dismissed`,
  task-114 profile/activity/stats suites
- `cd web && npm ci && npm test`


## Implementation Notes

Cargo tests were deferred to independent verification, not claimed
passed. The merge resolution and contract repairs require fresh
independent review and remote verification.
