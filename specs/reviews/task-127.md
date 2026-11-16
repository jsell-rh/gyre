# Review — task-127 (Implement user management UI pages)

Spec: `specs/system/user-management.md` §UI Pages.
Candidate: `23dd976d51609a0124df27ad173a91b4e432f3cb` (branch
`pipeline/task-127/c37ef410488f4477b1e1cef311c70304-1`, assignment base
`e5089d92`). Commits under review: the 14 SHAs in the task frontmatter
plus merge/process commits that fold main in; process commits touch only
`specs/tasks/task-127.md`.
Verdict: **approved** — every acceptance criterion is backed by real
production code that I exercised independently, with two mutation probes
confirming the tests are not vacuous. Deferred items for the verifier are
recorded below (Rust suite cannot compile in the review sandbox: no
crates.io network, no vendored registry).

## Probes run (all on the exact candidate HEAD, clean tree)

- `cd web && npm ci` → exit 0.
- Focused vitest on the 10 task-127 test files → 204/204 passed
  (`/tmp/stage/review-evidence/web-focused-vitest.log`).
- Full `cd web && npm test` → **123 files, 2435 passed / 41 skipped,
  exit 0** (`web-npm-test-full.log`).
- Mutation probe A: removed `active: true` from the drawer listing →
  `NotificationDrawer.test.js` fails the active-only assertion; restored.
- Mutation probe B: no-op `refreshUnreadCount` → `notifications-store.test.js`
  fails; restored. Tree clean after both.
- `scripts/check-dead-components.sh` (4 violations) and
  `check-assertionless-tests.sh` (exit 2): byte-identical behavior at the
  assignment base — pre-existing, not introduced by this candidate.
- `scripts/check-notification-toggle-coverage.sh` → OK (41/41 lockstep).
- No route-registry/ABAC/scripts changes in `git diff e5089d92 HEAD` —
  the `?active=true` param extends an existing handler; no new routes.
- `cargo check -p gyre-ports -p gyre-adapters`: **blocked by sandbox**
  (index.crates.io unreachable, no cargo registry cache — infrastructure
  restriction, not a code defect). Rust diff reviewed by inspection.

## Verified working (acceptance criteria)

- **User Settings** (`UserProfile.svelte`): real edit form (display name,
  timezone, locale) via `PUT /users/me`; preferences fieldset (theme,
  density, diff view, feed scope, font size) via the same partial update;
  notification per-type toggles + delivery channels (email digest/min
  priority, webhook, slack) via `GET/PUT
  /users/me/notification-preferences` using the real
  `{preferences, channels}` contract with canonical `NotificationType`
  names; Session Management tab (`GET /users/me/sessions`, per-session
  `DELETE /users/me/sessions/{id}`, `POST .../revoke-all`).
- **User Profile at `/@username`** (`PublicUserProfile.svelte`): route
  parsed/rendered in `App.svelte` (`user_profile` mode +
  `goToUserProfile` context, avatar-menu link); consumes
  `api.publicUserProfile/publicUserStats/publicUserActivity`; cross-tenant
  and unknown handles collapse to 404.
- **Tenant User Management** (`TenantSettings.svelte` Users tab):
  TenantAdmin-gated listing (`GET /users`, `usersNotAdmin` render guard),
  search, deactivate with reason+confirm (`PUT /users/{id}/deactivate`),
  provisioning (`POST /users`); i18n keys present and complete.
- **Workspace Members** (`WorkspaceSettings.svelte` Members tab): member
  list with role badges, role-change dropdown with ownership-transfer
  reason prompt (`api.updateMemberRole` with `{role, reason}`), remove
  member, invite modal resolving username → user id before
  `POST /workspaces/{id}/invite`, pending invitations tab with revoke.
- **Invitation Accept page** at `/invite/{id}` (`InvitationAccept.svelte`):
  workspace preview, accept (`POST /workspaces/{id}/invite/accept`),
  decline (invitee self-decline via `DELETE
  /workspaces/{id}/invitations/{invitation_id}` — server-side
  authorization verified in `api/users.rs::revoke_invitation`: manager or
  the invitee's own row only, 404-masked for foreign invitations).
- **Notification Drawer** (`NotificationBell.svelte` +
  `NotificationDrawer.svelte` + `notifications.svelte.js`): unread badge
  from `GET /notifications/unread-count` with realtime `NotificationPush`
  bumps on the shared /ws plus 60s resync; drawer lists active-only
  (`?active=true`), groups by priority band, deep links (incl.
  `InvitationReceived` → `/invite/{ws}`) with no fabricated targets, mark
  read/unread and mark-all-read.
- **Team Management** within workspace settings (Teams tab): real team
  CRUD + membership over the task-122/125 endpoints
  (`createTeam`/`updateTeam`/`addTeamMember`/`removeTeamMember`).
- **Endpoints**: every page consumes task-124 (user/membership/invitation)
  and task-125 (teams/notifications) endpoints through `web/src/lib/api.js`
  — verified each binding against the registered server routes.
- **Svelte 5 runes**: all new/edited components use `$state`/`$derived`/
  `$effect`/`$props` (spot-verified in every reviewed component).

## Rust delta (reviewed by inspection; verifier must execute)

`list_active_for_user` added to the `NotificationRepository` port and
implemented in **all four** implementors (SQLite, PostgreSQL, mem, and the
`BroadcastingNotificationRepo` wrapper) with the resolved/dismissed guard
in the query itself; `GET /api/v1/notifications?active=true` routes to it;
`list_for_user` keeps the full Inbox contract. Real adapter test
(`list_active_for_user_excludes_resolved_and_dismissed`, incl. the
read-but-active row kept) and real router-driven handler test
(`spec_path_notifications_active_filter_excludes_resolved_and_dismissed`).
The invitee self-decline addition to `revoke_invitation` enforces
per-handler authorization (manager or own membership row; foreign
invitation ids 404) with a distinct audit event.

**Verifier must run** (cannot compile in this sandbox — no crates.io
access): `cargo test --all`; specifically the gyre-adapters
`list_active_for_user_excludes_resolved_and_dismissed` test, the
gyre-server `spec_path_notifications_active_filter_excludes_resolved_and_dismissed`
test, and the user/session/invitation/team handler suites.

## Honesty checks

- Coverage row 31 (§UI Pages) honestly stays **task-assigned** (not
  `implemented`): the remaining rows (Tenant Invitation Management incl.
  `/invite/{token}` magic links and bulk invite — task-110; invitation
  expiry countdown — no expiry field on WorkspaceMembership; inline global
  role editing — no endpoint) are documented as unbuilt rather than
  claimed.
- Task doc acceptance checkboxes remain unchecked; progress is
  `ready-for-review`, not self-marked complete.
- The bell/unread reconciliation with `ui-navigation.md` was made through
  a dated spec-lifecycle amendment (`Bell amendment (2026-11, task-127)`),
  not an unamended contradiction.

## Observations (non-blocking)

- The Invitation Accept page routes by **workspace id**
  (`/invite/{workspaceId}`), not by token: task-124 models workspace
  invitations as membership rows with no token, so a token-keyed page
  cannot consume the dependency endpoints (the acceptance criterion also
  requires consuming task-124/125 endpoints). The `/invite/{token}`
  magic-link form belongs to the tenant-invitation flow (task-110) and is
  recorded as an open coverage row there. Documented in the component
  header and the coverage matrix.
- `web/dist/` is owned by the deterministic verifier (per review
  assignment); no agent-run build was required or checked here.
- Pre-existing `check-dead-components.sh` violations (4) and
  `check-assertionless-tests.sh` exit 2 exist identically at the
  assignment base; not this candidate's regressions.

Test-inflation check: none found — tests assert exact endpoint calls with
argument shapes, and both mutation probes (drawer active-only param,
unread-count refresh) were killed by the suites.
