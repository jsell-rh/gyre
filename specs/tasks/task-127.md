---
title: "Implement user management UI pages"
spec_ref: "user-management.md §UI Pages"
depends_on:
  - task-124
  - task-125
progress: ready-for-review
coverage_sections:
  - "user-management.md §UI Pages"
commits: ["d1736a94", "6d71565c", "1ed61cf1", "94f6eb35", "80641b88", "14f27ab5", "f5aee7da", "b02f8f18", "e468155e", "c5902874", "d577374a", "8d72ffec", "4aa5a87a", "64c85ec9", "50f338e9", "1deff0b5", "b6966093", "fe84485b", "0f127bc0", "4de47315", "1a32b4e6"]
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

All §UI Pages rows ship as real Svelte 5 (runes) components
consuming the task-110/124/125 endpoints: User Settings + Session
Management (`UserProfile.svelte`, `/profile` — display name, timezone,
locale, theme/density/diff preferences, notification channel config,
active sessions with revoke), User Profile at `/@{username}`
(`PublicUserProfile.svelte` — identity header, workspace memberships
with roles, teams, public-action activity feed), Tenant User
Management (Users tab in `TenantSettings.svelte`, TenantAdmin-gated:
list/search, invite, deactivate, inline global-role editing),
Workspace Members/Invitations with role change and ownership
transfer (`WorkspaceSettings.svelte`), Team Management within
workspace settings, and the Notification Drawer (bell with unread
badge, priority-grouped active notifications, deep links, mark
read/unread) plus Notification Preferences.

### Tenant Invitation Management

`TenantSettings.svelte` has an Invitations tab (TenantAdmin-gated — a
forbidden listing renders the not-admin note, not an error) over the
task-110 endpoints: `GET /api/v1/tenant/invitations` with a status
filter (Pending/Accepted/Declined/Expired/Revoked), a single invite
modal (email + GlobalRole + optional expiry-days override →
`POST /tenant/invite`), a bulk invite modal (one email per line →
`POST /tenant/invite/bulk`, per-entry errors surfaced), revoke for
Pending rows (`DELETE /tenant/invitations/{id}`), and the create
response's one-time `invite_url` surfaced with a copy action.

### `/invite/{token}` magic-link accept page

`App.svelte` `parseUrl` disambiguates the 64-hex CSPRNG token from
the authenticated flow's UUID workspace id. The token flow previews
via the unauthenticated read-only `GET /api/v1/invite/{token}/preview`
(kind/status/invited email/role/workspace names — the spec row's
"accept/decline with workspace preview"), then accepts/declines via
`POST /invite/{token}/accept|decline` (tenant) and
`POST /workspaces/invitations/{token}/accept|decline` (workspace).
Token calls carry no Bearer header, so a logged-in session of a
different user cannot burn another address's invitation. Local-mode
tenant accept displays the minted first API key exactly once;
terminal-status (used/expired/revoked) and unknown-token links render
no accept action.

### Inline global-role editing

`PUT /api/v1/users/{id}/role` (TenantAdmin-only per-handler check,
same-tenant containment, self-change and deactivated-target 409s,
GlobalRole and UserRole set in lockstep, audit-logged) consumed by an
inline role select in the Users tab, hidden for self and deactivated
accounts.

### This round: rebase onto main 9f446d0e

Resolved the docs/ui.md conflict in the Shell Layout section: kept
this branch's notification-bell top-bar wording (the dated bell
re-pointing the Top Bar section and user-management.md §In-App
Notification UI already describe) and took main's ui-navigation.md
§1a nav-rail amendment paragraphs (2026-11-16 decision
`16871182b1370b61693fc691`, task-246 decomposition) verbatim,
including the no-*permanent*-sidebar emphasis. All other files
auto-merged; `web/src` and `web/dist` are byte-identical to the
retained source.

### Test evidence

- Mechanical gates after the rebase: `check-arch.sh`,
  `check-abac-route-registry.sh`, `check-mcp-write-tools.sh`,
  `check-migration-versions.sh` — all pass.
- Prior round's full web suite (`cd web && npx vitest run`): 2494
  passed / 0 failed (41 pre-existing skips), including
  token-flow accept/decline/preview/credential display
  (InvitationAccept.test.js), Invitations tab list/filter/invite/
  bulk/revoke/not-admin + inline role select (TenantSettings.test.js).
  This round changed no `web/src` file, so those results remain
  the evidence; verifier should re-run `cd web && npm ci && npm test`
  plus `cargo test --all` (focused server packages:
  `cargo test -p gyre-server users::` for the role/invitation
  handler tests) at the integrated head.
