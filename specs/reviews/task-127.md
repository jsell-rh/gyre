# Review: TASK-127 — User Management UI Pages

**Reviewer:** Independent review agent
**Date:** 2026-11-14
**Candidate:** `1fa493cac1464ac9d3753ac4649bab1fa7d697c1` (base `44cf25bf0bea544979fcae88c4d80b1165964fbe`, merge head `ef28d208`)
**Verdict:** approved — real implementations, no fakery found; evidence below.

---

## Scope reviewed

`git diff 44cf25bf..1fa493ca` — 21 files, +3179/−100: new
`PublicUserProfile.svelte`, `InvitationAccept.svelte`,
`NotificationDrawer.svelte`; reworked `UserProfile.svelte`,
`TenantSettings.svelte` (Users tab), `WorkspaceSettings.svelte` (new
Members tab + real Teams tab); `App.svelte` routes (`/@username`,
`/invite/:id`), `api.js` bindings, `en.json`; one backend handler change
(`revoke_invitation` invitee self-decline) with a new test; 6 test files
(new + extended).

## Independent evidence (saved under /tmp/stage/review-evidence)

1. **Endpoint existence.** Every `api.js` call added by the candidate was
   verified against `crates/gyre-server/src/api/mod.rs` route
   registrations: `GET /users`, `POST /users`, `PUT /users/:id/deactivate`,
   `GET /users/:username`, `GET/PUT /users/me`, `GET /notifications`,
   `GET /notifications/unread-count`, `PUT|POST /notifications/:id/read`,
   `POST /notifications/:id/unread`, `POST /notifications/mark-all-read`,
   `GET/PUT /notifications/preferences`, `GET /workspaces/:id`,
   `POST /workspaces/:id/invite`, `GET /workspaces/:id/invitations`,
   `POST /workspaces/:id/invite/accept`,
   `DELETE /workspaces/:id/invitations/:invitation_id`,
   `PUT /workspaces/:id/members/:user_id/role`, and the task-122 team
   CRUD/membership routes. No fabricated endpoint.
2. **`cd web && npm ci && npm test`** — 2382 passed, 41 skipped, 0 failed
   (118 files), exit 0. Run by the reviewer on the merged tree.
3. **`SKIP_WEB_BUILD=1 cargo check -p gyre-server`** (via the enforced
   build wrapper) — clean, exit 0. The modified `revoke_invitation`
   handler compiles.
4. **Mutation probe.** Removing the `api.markNotificationRead` /
   `api.markNotificationUnread` calls from `NotificationDrawer.svelte`
   makes `NotificationDrawer.test.js` fail 2 of 8 tests (restored after
   the probe). The new tests kill real bugs — they are not
   self-confirming. Read review of the remaining suites confirms they
   assert actual API call shapes and rendered states (e.g. decline calls
   `revokeWorkspaceInvitation('ws-9','inv-1')`; prefs PUT asserts the
   real `{preferences, channels}` contract).
5. **Backend change line review.** `revoke_invitation` now allows the
   invitee to decline their own pending invitation
   (§Workspace Invitation Flow step 4): manager paths unchanged
   (`can_manage_membership`), non-manager non-invitee callers get 404
   (indistinguishable from nonexistent — same containment pattern as the
   tenant-public profile), accepted invitations still 409, distinct audit
   event `workspace_invitation_declined`. Test
   `invitee_can_decline_own_pending_invitation` covers third-party 404,
   self-decline, and row removal. No new route → no ABAC registry change.
6. **Attribution.** `scripts/check-task-commit-attribution.sh` passes at
   the candidate commit (verified with the candidate's `task-127.md`
   temporarily checked out; the failure against the working tree is the
   executor's uncommitted frontmatter reset, not the candidate).
7. **i18n.** All new keys (`notification_drawer.*`, `invite_accept.*`,
   `public_profile.*`, `workspace_settings.members.*`,
   `tenant_settings.users.*`, `user_profile.tabs.invitations`) present in
   `web/src/locales/en.json`.
8. **Generated output.** `web/dist/` untouched by the branch — owned by
   the deterministic verifier, as required.

## Acceptance-criteria mapping

- User Settings with profile editing + preferences — ✅ (`/profile`
  preferences fieldset via `PUT /users/me`, delivery channels via
  `PUT /notifications/preferences`, Invitations tab).
- User Profile at `/@username` — ✅ (route + component; accepted
  memberships, team memberships, honest 404 state).
- Tenant User Management (TenantAdmin only) — ✅ (403 renders a
  requires-TenantAdmin note; list/search/deactivate/provision).
- Workspace Members with invite, role change, ownership transfer — ✅
  (Members tab; Owner-role changes prompt for the audit reason; invite
  resolves @username → user id; pending invitations + revoke).
- Invitation Accept page — ✅ with a documented deviation: the path
  segment is the workspace id because task-124 models pending
  invitations as membership rows with no token; `/invite/{token}` magic
  links belong to task-110 (not started). No fake token is fabricated.
- Notification Drawer with unread badge, priority grouping, deep links —
  ✅ (unread pill, Urgent/High/Medium/Low grouping, action_url deep links
  incl. InvitationReceived → `/invite/{ws}`, mark read/unread, bulk).
- Team Management within workspace settings — ✅ (real team CRUD +
  membership modal over the task-122 endpoints).
- Consumes task-124/125 endpoints — ✅ (verified, see 1).
- Svelte 5 runes — ✅ (`$state`, `$derived(.by)`, `$props`, `$bindable`,
  `$effect`, `untrack`).
- `npm test` passes — ✅ (verified, see 2).
- `cargo test --all` — deferred to the verifier per the 4 GiB build
  constraint; commands recorded below.

## Non-blocking observations

- **Bell click vs ui-navigation.md §2.** The top-nav bell now opens the
  notification drawer (mandated by user-management.md §UI Pages, the
  assigned section). ui-navigation.md §2 still says the decisions badge
  "clicking navigates to the workspace home with the Decisions section
  expanded" — that line is now stale. The implementer documented the
  reconciliation in the Shipped notes and kept the badge count semantics
  (principle 4). The bell unread-count semantics belong to §In-App
  Notification UI (coverage row 19, task-113, not started). Recommend a
  spec-lifecycle amendment of ui-navigation.md §2 when task-113 closes
  row 19, rather than leaving the two texts contradictory.
- **AppShell routing tests mirror `parseUrl`/`urlFor`** in the test file
  (pre-existing convention of that file) rather than importing the
  production functions — weak evidence on its own, but component-level
  tests cover the routed pages' behavior.
- **Coverage row 31 stays `task-assigned`** — correct: Session Management
  (task-111), Tenant Invitation Management / token links / bulk invite /
  expiry countdown (task-110), per-user activity feed, and global-role
  editing have no backing API; nothing is faked for them.

## Verifier must execute (recorded per 4 GiB constraint)

- `cargo test -p gyre-server invitee_can_decline_own_pending_invitation`
- `cargo test -p gyre-server workspace_invitation_lifecycle`
- Full `cargo test --all` (task acceptance criterion)
- Full web production build (dist is verifier-owned)
