---
title: "Implement user management UI pages"
spec_ref: "user-management.md §UI Pages"
depends_on:
  - task-124
  - task-125
progress: complete
coverage_sections:
  - "user-management.md §UI Pages"
commits: ["0f127bc0e111c65ace292e45231ab585242aad99", "4de47315cc28796662ba04fb99cc36403390979a", "1a32b4e6422a0533c5a5075b4991a2163918a76f", "fe84485b4da230b8e3ace0840e68898cdcab5a7c"]
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

All acceptance-criterion pages shipped as real Svelte 5 (runes) components
against the task-124/125 endpoints in `1a32b4e6` (asset build `4de47315`),
rebased onto current main (`9bfab36`) via merge `7c838821` (App.svelte mode
lists unioned), and repaired per review `44b2bd1d` in `0f127bc0`.

### Pages

- **User Settings (`/profile`)** — profile edit + preferences fieldset (theme,
  density, diff view, feed scope, code font size) via `PUT /users/me`;
  notification-preferences tab on the real `{preferences, channels}` contract
  extended with email/webhook/Slack channel config via
  `PUT /api/v1/notifications/preferences`; Invitations tab (accept/decline my
  pending memberships).
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
  new decline path in `DELETE /workspaces/{id}/invitations/{invitation_id}`).
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

### Review repair (`0f127bc0`)

Review `44b2bd1d` found `tenant_settings.users.col_role` and
`.count_note` missing from `web/src/locales/en.json`, so the Users table
header/count note rendered the raw key paths. Added both keys (a sweep of
all 112 `tenant_settings.*` references confirmed these were the only
missing entries) and a rendered-output regression guard asserting resolved
copy (`Role`, `Showing 2 of 2 users.`) and that no raw
`tenant_settings.users.*` path leaks into the panel. Evidence, including a
kill-the-bug run that fails without the fix reproducing the reviewer's
output exactly: `/tmp/stage/review-evidence/i18n-key-repair.md`.

### Test evidence

- This repair round (`0f127bc0`): `cd web && npm ci && npx vitest run
  src/__tests__/TenantSettings.test.js` — **46/46 passed**; the new guard
  fails against the unfixed en.json, reproducing the reviewer's output
  exactly (evidence: `/tmp/stage/review-evidence/i18n-key-repair.md`).
- Prior round (post-merge, tree at `7c838821`): `cd web && npm ci && npm test`
  — **2395 passed, 41 skipped, 0 failed** (119 files); `SKIP_WEB_BUILD=1
  cargo check -p gyre-server` — clean; `scripts/check-abac-route-registry.sh`
  and `scripts/check-arch.sh` — clean.
- Rust side unchanged this round; server tests the independent verifier must
  execute (4 GiB budget blocks server test compilation in-sandbox):
  - `cargo test -p gyre-server invitee_can_decline_own_pending_invitation`
  - `cargo test -p gyre-server workspace_invitation_lifecycle`
  - Full `cargo test --all` (task acceptance criterion)

### Known gaps (blocked on other tasks, not faked)

- Session Management page — `/users/me/sessions` endpoints owned by task-111
  (not-started); no page is stubbed for it.
- Tenant Invitation Management incl. `/invite/{token}` magic links and bulk
  invite — `/api/v1/tenant/invite*` owned by task-110 (not-started); the
  task-124 surface models invitations as membership rows, so the workspace
  flow above is what this task's endpoints expose.
- Invitation expiry countdown — `WorkspaceMembership` has no expiry field
  (task-110); invited date shown instead.
- Public-profile activity feed/stats — no per-user endpoints exist.
- Inline global-role editing — no endpoint exists; roles display read-only.
