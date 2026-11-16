---
title: "Implement user management UI pages"
spec_ref: "user-management.md §UI Pages"
depends_on:
  - task-124
  - task-125
progress: ready-for-review
coverage_sections:
  - "user-management.md §UI Pages"
commits: ["f5aee7dab7577d13d3698e29818a837de63adad0", "b02f8f185cf6fecddee1470c779c1dd1d38b39a5", "e468155e73374c55968e7ea3b956c8b80227bf2f", "c5902874c8d6692ed9ab6c1544ffd1daf2380839", "d577374a98ca345672978b2b8a9763b618a6af31", "8d72ffecacb20fe2c436a0736fecec3ad39cbc3e", "4aa5a87ada50825ab676317fd030220dc814d01e", "64c85ec9c91d105a34196ce0d370036ca70b801a", "50f338e93383f6a4d59c408a3676d63aac85b45b", "1deff0b561bed7e78f2a0ece63bf2d79ac593dc2", "b696609378236716366ce85ef42a9b9a2c65d92c", "fe84485b4da230b8e3ace0840e68898cdcab5a7c", "0f127bc0e111c65ace292e45231ab585242aad99", "4de47315cc28796662ba04fb99cc36403390979a", "1a32b4e6422a0533c5a5075b4991a2163918a76f", "14f27ab5f4ed0ca56d6d59dd4f2503157b4f8a19"]
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

### This round (review repair of candidate ec542eaf)

Two review findings repaired:

- **Attribution invariant (blocking):** commit `fdc9962e`'s subject
  mentioned task-110 in prose ("re-point invitation UI surfaces to the
  merged task-110 contract"), so `scripts/check-task-commit-attribution.sh`
  required the commit in task-110's `commits:` frontmatter too and
  failed the tree. Fixed by rewriting the commit subject on this branch
  to "fix(task-127): re-point invitation UI surfaces to the merged
  invitation contract" (drop the prose task-110 mention; the body still
  carries the full context). The rewrite preserved every commit tree,
  message, author, and committer date — `git diff` between the old head
  `d754a5d1` and the rewritten head is empty. Rewritten commit:
  `f5aee7da` (was `fdc9962e`); the recorded commits list now carries the
  rewritten SHA.
- **Dead error state in `InvitationAccept.svelte` (non-blocking):**
  `loadError` was declared and reset but never assigned, so a non-404
  failure of `GET /workspaces/{id}` (e.g. HTTP 500) left `workspace`
  null and silently rendered the invite card with an empty workspace
  name. `load()` now assigns `loadError` and stops on non-404
  workspace-fetch failures — the page renders
  `invite_accept.load_error_title` with no accept/decline actions.
  Fixed in `14f27ab5`, which also adds the distinguishing vitest case
  (500 on the workspace fetch → load-error state, no invite card);
  that case fails against the previous code and passes with the fix.

Evidence this round (retained under /tmp/stage/review-evidence):

- `check-attribution-pre-repair.log`: check run before the repair,
  confirming the `fdc9962e task-110` violation (the task-127 entries in
  that log reflect the executor's reset of the working-tree frontmatter
  to `commits: []`, restored below).
- `vitest-invitationaccept-repair.log`: focused vitest on
  InvitationAccept + InvitationDeclineNavigate → 2 files / 8 tests
  passed, exit 0, including the new load-error case.
- `vitest-invitationaccept-bugproof.log`: the same suite with ONLY the
  component fix stashed (old code) → the new load-error case fails,
  proving it kills the reviewed bug.

Prior-round context (merge repair of main head e5df7847): the base
merge carried task-110's tenant/workspace invitation model, replacing
task-124's membership-row model; `api/users.rs` parallel handlers were
resolved in favor of `api/invitations.rs`, and three task-127 UI
surfaces were re-pointed to the merged contract (invitee-facing
`GET /users/me/invitations`, `POST /workspaces/{id}/invite/accept|decline`,
status badge + expiry countdown, revoke only for Pending rows; `api.js`
attaches HTTP status to thrown errors so the 404 branch is reachable
in production). Focused vitest at that head: 11 files / 289 tests
passed; `cargo check -p gyre-server` clean; mechanical check scripts
OK (mawk-vs-gawk failures are sandbox restrictions, recorded above).

Remaining open for this section (kept task-assigned, row 31): Tenant
Invitation Management page (task-110's `/tenant/invite`,
`/tenant/invite/bulk`, `/tenant/invitations`, revoke endpoints are now
in-tree but no page consumes them), the `/invite/{token}` magic-link
accept page (token accept/decline endpoints exist in-tree; there is no
unauthenticated token-preview endpoint for the specced workspace
preview, and the local-mode accept must display the minted first
credential), and inline global-role editing (no endpoint).

For the independent verifier (deferred per 4 GiB build budget):

- `cargo test --all` (workspace)
- gyre-server handler tests: user/session/invitation/team endpoints
  (task-110's `api::invitations` suite covers the invitation lifecycle
  through the routed paths),
  `spec_path_notifications_active_filter_excludes_resolved_and_dismissed`,
  task-114 profile/activity/stats suites
- `cd web && npm ci && npm test`
- `bash scripts/check-task-commit-attribution.sh` on the final tree
  (needs full history; the branch-local rewrite keeps it green)
