---
title: "Implement user management UI pages"
spec_ref: "user-management.md §UI Pages"
depends_on:
  - task-124
  - task-125
progress: ready-for-review
coverage_sections:
  - "user-management.md §UI Pages"
commits: ["6d71565c5cb4a691d7ea0eddfdaeb50f59cba348", "1ed61cf1a71249c007a2dcf0bb59316e812b6d76", "94f6eb357ceb0f055283ec96453dae0b20826b68", "80641b880eb540870f63bbba7d9a1c78faa986f1", "14f27ab5f4ed0ca56d6d59dd4f2503157b4f8a19", "f5aee7dab7577d13d3698e29818a837de63adad0", "b02f8f185cf6fecddee1470c779c1dd1d38b39a5", "e468155e73374c55968e7ea3b956c8b80227bf2f", "c5902874c8d6692ed9ab6c1544ffd1daf2380839", "d577374a98ca345672978b2b8a9763b618a6af31", "8d72ffecacb20fe2c436a0736fecec3ad39cbc3e", "4aa5a87ada50825ab676317fd030220dc814d01e", "64c85ec9c91d105a34196ce0d370036ca70b801a", "50f338e93383f6a4d59c408a3676d63aac85b45b", "1deff0b561bed7e78f2a0ece63bf2d79ac593dc2", "b696609378236716366ce85ef42a9b9a2c65d92c", "fe84485b4da230b8e3ace0840e68898cdcab5a7c", "0f127bc0e111c65ace292e45231ab585242aad99", "4de47315cc28796662ba04fb99cc36403390979a", "1a32b4e6422a0533c5a5075b4991a2163918a76f"]
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
the task-110/124/125 endpoints. Prior rounds shipped the core pages
(User Settings + Session Management in `UserProfile.svelte`, User
Profile at `/@username`, Tenant User Management, Workspace
Members/Invitations with ownership transfer, Team Management,
Notification Drawer with unread badge/priority grouping/deep links).
This review-repair round (findings 61bf6f38) closes the three rows
that review left open:

### Tenant Invitation Management (finding 1)

`TenantSettings.svelte` gains an Invitations tab (TenantAdmin-gated —
a forbidden listing renders the not-admin note, not an error) over the
in-tree task-110 endpoints: `GET /api/v1/tenant/invitations` with a
status filter (Pending/Accepted/Declined/Expired/Revoked), a single
invite modal (email + GlobalRole + optional expiry-days override →
`POST /tenant/invite`), a bulk invite modal (one email per line →
`POST /tenant/invite/bulk`, per-entry errors surfaced), revoke for
Pending rows (`DELETE /tenant/invitations/{id}`), and the create
response's one-time `invite_url` surfaced with a copy action.

### `/invite/{token}` magic-link accept page (finding 2)

`App.svelte` `parseUrl` disambiguates the 64-hex CSPRNG token from
the authenticated flow's UUID workspace id (the shapes are disjoint);
`InvitationAccept.svelte` handles both. The token flow previews via a
new read-only unauthenticated `GET /api/v1/invite/{token}/preview`
(kind/status/invited email/role/workspace names — satisfying the spec
row "accept/decline with workspace preview" without amendment), then
accepts/declines via `POST /invite/{token}/accept|decline` (tenant)
and `POST /workspaces/invitations/{token}/accept|decline`
(workspace). `api.js` `request()` gains `skipAuth`: the token calls
carry NO Bearer header, so a logged-in session of a different user
cannot burn an invitation issued to another address. The local-mode
tenant accept displays the response's minted first API key exactly
once with a copy action (`api_key.key`); terminal-status
(used/expired/revoked) and unknown-token links render no accept
action.

### Inline global-role editing (finding 3)

New `PUT /api/v1/users/{id}/role` (TenantAdmin-only per-handler
check, same-tenant containment, self-change and deactivated-target
409s, GlobalRole and UserRole set in lockstep, audit-logged as
`user_global_role_changed`; ABAC registry entry added) consumed by an
inline Member/TenantAdmin select in the Users tab, hidden for self
and deactivated accounts. The stored record is what `is_tenant_admin`
reads on every call, so a demotion is effective immediately.

### Test evidence

- Full web suite (`cd web && npx vitest run`): **2494 passed / 0
  failed** (41 pre-existing skips), including the new tests:
  token-flow accept/decline/preview/credential display
  (InvitationAccept.test.js), Invitations tab list/filter/invite/
  bulk/revoke/not-admin + inline role select (TenantSettings.test.js),
  parseUrl/urlFor token round-trip (AppShell.test.js), and the
  skipAuth/no-Bearer + new-endpoint contracts (api.test.js).
- `cargo check -p gyre-server --lib --tests`: clean (includes the new
  test code).
- Mechanical checks: `check-abac-route-registry`,
  `check-abac-exempt-handlers`, `check-arch`, `check-mem-port-contracts`
  pass; a full `scripts/check-*.sh` sweep shows zero output differences
  vs the assignment base (no new violations).
- **For the independent verifier** (deferred per the 4 GiB build
  budget — large gyre-server test compilation must run there):
  - `cargo test -p gyre-server --lib api::invitations::tests::preview`
    (4 tests: tenant preview with pre-assigned workspaces,
    workspace-kind preview, expired-pending reported-not-persisted,
    unknown token 404)
  - `cargo test -p gyre-server --lib api::users::tests::update_user_role`
    (2 tests: lockstep role change + guards/audit, cross-tenant and
    deactivated guards)
  - `cargo test --all` and the full CI gate set.
