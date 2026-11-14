---
title: "Implement user management UI pages"
spec_ref: "user-management.md §UI Pages"
depends_on:
  - task-124
  - task-125
progress: ready-for-review
coverage_sections:
  - "user-management.md §UI Pages"
commits: ["1a32b4e6422a0533c5a5075b4991a2163918a76f", "4de47315cc28796662ba04fb99cc36403390979a"]
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

All acceptance-criterion pages shipped as real Svelte 5 (runes) components against
the task-124/125 endpoints. Product commit: `1a32b4e6` (plus asset build
`4de47315`); this round (merge commit `67eb6ac1`) rebased the branch onto
current main (`8d07a41a`, task-126 CLI) — sole conflict was the coverage-matrix
rows 30/31 in `specs/coverage/system/user-management.md`, resolved by keeping
both sides' notes (main's task-126 CLI note + this task's UI Pages note).

### Pages

- **User Settings (`/profile`)** — profile edit + preferences fieldset (theme,
  density, diff view, feed scope, code font size) via `PUT /users/me`;
  notification-preferences tab fixed to the real `{preferences, channels}`
  contract and extended with email/webhook/Slack channel config via
  `PUT /api/v1/notifications/preferences`; new Invitations tab (accept/decline
  my pending memberships).
- **User Profile (`/@username`)** — `PublicUserProfile.svelte`: display name,
  username, avatar, timezone, member-since via `GET /users/{username}`;
  accepted workspace memberships with role badges; team memberships;
  honest 404 state.
- **Tenant User Management (TenantSettings → Users tab)** — real `GET /users`
  list (TenantAdmin-gated; 403 renders a gated note, no fake list), search,
  Deactivate with reason → `PUT /users/{id}/deactivate`, Provision modal →
  `POST /users`, rows link to `/@username`.
- **Workspace Members (WorkspaceSettings → Members tab)** — member list with
  role badges + inline role change (ownership-transfer reason prompt), remove,
  invite-by-username (handle resolved via `GET /users/{username}` then
  `POST /invite`), pending invitations table with revoke.
- **Team Management (WorkspaceSettings → Teams tab)** — reworked from a
  mislabeled member list to real team CRUD + membership management over the
  task-122 endpoints.
- **Invitation Accept (`/invite/{workspaceId}` + `InvitationAccept.svelte`)** —
  workspace preview, Accept → `POST /invite/accept`, Decline → self-decline;
  honest terminal states. The segment is the workspace id because task-124
  models pending invitations as membership rows; the `/invite/{token}` form is
  the tenant-invitation flow owned by task-110.
- **Notification Drawer (`NotificationDrawer.svelte`)** — bell opens a
  slide-out grouped by priority band, per-item read/unread, mark-all-read,
  unread pill, deep links (SPA router; `InvitationReceived` →
  `/invite/{workspace_id}`).

### Backend change

`DELETE /workspaces/{id}/invitations/{invitation_id}` (users.rs) now also
allows the invitee to decline their own pending invitation (§Workspace
Invitation Flow step 4 had no decline path): scoped to the caller's own
membership row, foreign invitations 404-masked, distinct
`workspace_invitation_declined` audit event. No new route, no ABAC registry
change.

### Known gaps (blocked on other tasks, not faked)

- Session Management page — `/users/me/sessions` endpoints owned by task-111
  (not-started).
- Tenant Invitation Management incl. `/invite/{token}` magic links and bulk
  invite — `/api/v1/tenant/invite*` owned by task-110 (not-started).
- Invitation expiry countdown — `WorkspaceMembership` has no expiry field
  (task-110); invited date shown instead.
- Public-profile activity feed/stats — no per-user endpoints exist.
- Inline global-role editing — no endpoint exists; roles display read-only.

### Test evidence

- This round (post-merge, tree at `67eb6ac1`): `cd web && npm ci && npm test` —
  **2382 passed, 41 skipped, 0 failed** (118 files); `cargo check -p gyre-cli`
  (merged task-126 code) — clean; `scripts/check-task-commit-attribution.sh`
  — clean after this file's `commits:` frontmatter lists `1a32b4e6`.
- Server tests the independent verifier must execute (4 GiB budget blocks
  server test compilation in-sandbox):
  - `cargo test -p gyre-server invitee_can_decline_own_pending_invitation`
  - `cargo test -p gyre-server workspace_invitation_lifecycle`
  - Full `cargo test --all` (task acceptance criterion)
