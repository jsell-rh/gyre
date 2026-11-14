---
title: "Implement user management UI pages"
spec_ref: "user-management.md §UI Pages"
depends_on:
  - task-124
  - task-125
progress: ready-for-review
coverage_sections:
  - "user-management.md §UI Pages"
commits: ["1a32b4e6422a0533c5a5075b4991a2163918a76f", "4de47315cc28796662ba04fb99cc36403390979a", "0f127bc0e111c65ace292e45231ab585242aad99", "fe84485b4da230b8e3ace0840e68898cdcab5a7c", "b696609378236716366ce85ef42a9b9a2c65d92c"]
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

All acceptance-criterion pages ship as real Svelte 5 (runes) components against
the task-124/125 endpoints in `1a32b4e6` (asset build `4de47315`), repaired per
review in `0f127bc0` (asset build `fe84485b`). The assigned rebase onto current
main (`9fb371ac`, task-111 session management) is resolved by merge `737c37ee`:
the only content conflict was the `specs/coverage/system/user-management.md`
rows 30/31 audit trail, resolved as a union (main's row 30 noting the session
endpoints landed; the retained row 31), and this round's `b6966093` ships the
one §UI Pages row the merge unblocked.

### Pages

- **User Settings (`/profile`)** — profile edit + preferences fieldset (theme,
  density, diff view, feed scope, code font size) via `PUT /users/me`;
  notification-preferences tab on the real `{preferences, channels}` contract
  via `PUT /api/v1/notifications/preferences`; Invitations tab (accept/decline
  my pending memberships).
- **User Profile (`/@username`)** — `PublicUserProfile.svelte`: display name,
  username, avatar, timezone, member-since via `GET /users/{username}`;
  workspace memberships with role badges; team memberships; honest 404 state.
- **Tenant User Management (TenantSettings → Users tab)** — real `GET /users`
  list (TenantAdmin-gated; 403 renders a gated note, no fake list), search,
  Deactivate with reason → `PUT /users/{id}/deactivate`, Provision modal →
  `POST /users`, rows link to `/@username`.
- **Workspace Members (WorkspaceSettings → Members tab)** — member list with
  role badges + inline role change (ownership-transfer reason prompt), remove,
  invite-by-username (resolved via `GET /users/{username}` then `POST /invite`),
  pending invitations table with revoke (invitee self-decline allowed by the
  decline path in `DELETE /workspaces/{id}/invitations/{invitation_id}`).
- **Team Management (WorkspaceSettings → Teams tab)** — real team CRUD +
  membership management over the task-122 endpoints.
- **Invitation Accept (`/invite/{workspaceId}` + `InvitationAccept.svelte`)** —
  workspace preview, Accept → `POST /invite/accept`, Decline → self-decline;
  honest terminal states. The segment is the workspace id because task-124
  models pending invitations as membership rows; `/invite/{token}` is the
  tenant-invitation flow owned by task-110.
- **Notification Drawer (`NotificationDrawer.svelte`)** — bell with unread
  badge, slide-out grouped by priority band, per-item read/unread,
  mark-all-read, deep links (`InvitationReceived` → `/invite/{workspace_id}`).
- **Session Management (User Settings → Sessions tab, `b6966093`)** — the row
  the task-111 merge unblocked: active sessions (device from User-Agent with
  an explicit WebSocket-session label for UA-less rows, IP, started /
  last-active / expiry) via `GET /users/me/sessions`; per-session revoke via
  `DELETE /users/me/sessions/{id}`; Sign-out-everywhere via
  `POST /users/me/sessions/revoke-all` (tooltip states a fully-revoked key is
  rejected, matching the auth middleware).

### Test evidence

- `cd web && npm test`: 119 files / 2399 passed, 41 skipped (pre-existing
  skips) — includes the four session tests (list, single revoke removes only
  that row, revoke-all empties the list, eight-permitted-tabs) and all prior
  task-127 page tests. Log: `/tmp/stage/review-evidence/task-127-userprofile-tests.log`.
- `cargo test --all` and large gyre-server builds are deferred to the
  independent verifier (4 GiB build-wrapper budget); this round changed no
  Rust code — the merge `737c37ee` is main's already-reviewed task-111 state,
  and `b6966093` touches only `web/src`, `docs/ui.md`, and specs/coverage.

### Still open (section stays task-assigned)

- Tenant Invitation Management incl. `/invite/{token}` magic links, bulk
  invite, and invitation expiry countdown — task-110 (not-started) owns the
  backend; no expiry field exists on WorkspaceMembership yet.
- Public-profile activity feed/stats — no per-user endpoints exist.
- Inline global-role editing — no endpoint.
