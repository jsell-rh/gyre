---
title: "Implement user management UI pages"
spec_ref: "user-management.md §UI Pages"
depends_on:
  - task-124
  - task-125
progress: ready-for-review
coverage_sections:
  - "user-management.md §UI Pages"
commits: ["1a32b4e6"]
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

### Pages shipped (all backed by real task-124/125 endpoints)

- **User Settings (`/profile`, extended):** the profile edit form now
  carries a Preferences fieldset (theme Light/Dark/System, UI density
  Compact/Comfortable/Spacious, diff view SideBySide/Unified, activity
  feed scope, code font size) persisted through `PUT /users/me`
  `preferences` (task-120 partial-update semantics). The Notification
  Preferences tab now also configures **delivery channels** (email
  enabled/digest/min-priority, webhook URL/secret/min-priority, Slack
  webhook/channel/min-priority; in-app locked on) via
  `PUT /api/v1/notifications/preferences` — and the tab was fixed to the
  real API contract: it previously read the response as a flat
  type→bool map and sent a map, so nothing round-tripped. A new
  **Invitations** tab lists my pending workspace memberships with
  Accept (`POST /workspaces/{id}/invite/accept`) and Decline (see backend
  note below).
- **User Profile (`/@username`, new route + `PublicUserProfile.svelte`):**
  display name, username, avatar, timezone, member-since from
  `GET /users/{username}`; accepted workspace memberships with role
  badges (names resolved via the workspace list); team memberships
  derived from the per-workspace team listings filtered by
  `member_ids`. 404 renders an honest not-found state (cross-tenant
  handles are indistinguishable from nonexistent ones server-side).
- **Tenant User Management (TenantSettings → Users tab, reworked):**
  real user list from `GET /users` (TenantAdmin-only; a 403 renders a
  "requires TenantAdmin" note instead of a fake list), client-side
  search over username/display name/email, Deactivate with
  reason → `PUT /users/{id}/deactivate` (self-deactivation hidden — the
  server rejects it), status badges from `deactivated_at`, and a
  Provision user modal → `POST /users` (admin user provisioning; the
  raw API key is shown once). Rows link to `/@username`.
- **Workspace Members (WorkspaceSettings → new Members tab):** member
  list with role badges + inline role-change select
  (`PUT /members/{user_id}/role`; an Owner-role change prompts for the
  audit reason), pending/active status, remove member
  (`DELETE /members/{user_id}`), invite-by-username modal (resolves the
  handle via `GET /users/{username}` then `POST /invite`), and a pending
  invitations table with revoke
  (`DELETE /invitations/{invitation_id}`). The member table was also
  fixed to unwrap the API's `{members: [...]}` envelope — the old tab
  assigned the envelope object to the list.
- **Team Management (WorkspaceSettings → Teams tab, reworked):** the tab
  previously showed the member list mislabeled as teams; it now does
  real team CRUD — create/edit team (name, slug, description, lead
  resolved from @username to a user id), delete team, and a membership
  panel (add by @username, remove, lead badge) over the task-122
  endpoints.
- **Invitation Accept page (`/invite/{workspaceId}` +
  `InvitationAccept.svelte`):** workspace preview (name, description,
  invited role, invited date), Accept → `POST /invite/accept`, Decline
  → `DELETE /invitations/{id}` (self-decline), and honest terminal
  states (accepted / declined / no pending invitation / workspace not
  found). The path segment is the workspace id: task-124 models a
  pending invitation as a membership row keyed by workspace — there is
  no token. The `/invite/{token}` magic-link form is the tenant-invitation
  flow owned by task-110.
- **Notification Drawer (`NotificationDrawer.svelte`):** the top-nav
  bell now opens a slide-out panel with recent notifications
  (`GET /api/v1/notifications`) **grouped by priority band**
  (Urgent/High/Medium/Low), per-item mark read/unread
  (`PUT /notifications/{id}/read`, `POST /notifications/{id}/unread`),
  mark-all-read, an unread count pill, and deep links — `action_url`
  paths navigate through the SPA router, and `InvitationReceived`
  items deep-link to `/invite/{workspace_id}`. The bell badge keeps the
  global active-decisions count (ui-navigation.md principle 4). Note:
  the bell previously scrolled to the workspace-home Decisions section
  (HSI "My Stuff" amendment); user-management.md §In-App Notification UI
  and this task's contract specify the drawer, which now owns the
  bell click. The Decisions section itself is unchanged.

### Backend change (one handler, spec-backed)

`DELETE /workspaces/{id}/invitations/{invitation_id}`
(`revoke_invitation` in `crates/gyre-server/src/api/users.rs`) now also
allows the **invitee to decline their own pending invitation** —
user-management.md §Workspace Invitation Flow step 4 ("Invitee accepts
or declines") had no decline path at all. The decline is the same
deletion as a revocation, scoped to the caller's own membership row;
non-managers see foreign invitations as 404 (indistinguishable from
nonexistent, same containment pattern as the tenant-public profile);
managers keep full revocation; the audit event distinguishes
`workspace_invitation_declined` from `workspace_invitation_revoked`.
No new route, no ABAC registry change.

### Known gaps (blocked on other tasks, not faked)

- **Session Management page** — the three `/users/me/sessions` endpoints
  are owned by task-111 (not started); no page is built against them.
- **Tenant Invitation Management page** (bulk invite, token magic links)
  — the `/api/v1/tenant/invite*` endpoints are owned by task-110 (not
  started). The Tenant User Management page's "invite" surface is the
  admin user-provisioning endpoint that exists (`POST /users`).
- **Invitation expiry countdown** — `WorkspaceMembership` has no expiry
  field (task-110 owns §Invitation Expiry); the invitations table shows
  the invited date instead of a fabricated countdown.
- **Public profile activity feed + stats** — no per-user activity or
  stats endpoints exist anywhere (`GET /activity` is tenant-global,
  unfiltered by user); nothing is rendered from fabricated data.
- **`/users/me` global-role editing** — no endpoint changes a user's
  global role; the tenant user list displays roles read-only.

### Test evidence

- `cd web && npm test`: **2339 passed, 41 skipped, 0 failed** (116
  files), including new suites `PublicUserProfile.test.js` (4 tests),
  `NotificationDrawer.test.js` (8), `InvitationAccept.test.js` (5), and
  new cases in `UserProfile.test.js` (invitations accept/decline,
  preferences PUT shape, channels payload contract),
  `WorkspaceSettings.test.js` (Members tab role-change/invite/invitations,
  Teams tab real CRUD), `TenantSettings.test.js` (user list/search/
  deactivate/not-admin), `AppShell.test.js` (`/@username` and
  `/invite/:id` parse/urlFor), `Inbox.test.js` (dismiss now uses the
  dismiss endpoint — `markNotificationRead` previously called
  `/dismiss`, conflating read and dismissed state).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server`: clean.
- `cargo fmt -p gyre-server -- --check`: clean.
- SPA production build (temp outDir, web/dist untouched): builds clean.

### Server tests the verifier must execute

- `cargo test -p gyre-server invitee_can_decline_own_pending_invitation`
  (new test in `api/users.rs`)
- `cargo test -p gyre-server workspace_invitation_lifecycle` (existing
  suite covering the modified handler's revocation path)
- Full `cargo test --all` (task acceptance criterion; server test
  compilation does not fit this sandbox's memory budget)
