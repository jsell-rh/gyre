# Review — task-127 (Implement user management UI pages)

Spec: `user-management.md` §UI Pages (coverage row 31).
Candidate: `62ed1272` against base `103d665e` (branch
`pipeline/task-127/23feffcaddf94bf1b0318163975a3e32-1`).
Verdict: **approved**.

## Scope reviewed

Authored frontend source + tests, plus the backend additions the pages
depend on (all in the base..candidate diff):

- `web/src/App.svelte` — routes `/@username` (public profile) and
  `/invite/{workspaceId}` (invitation accept), notification bell wired to
  `GET /notifications/unread-count`, drawer deep-link navigation
  (`navigateToPath`/`applyParsed`), popstate/history handling for the two
  new modes.
- New components: `PublicUserProfile.svelte`, `InvitationAccept.svelte`,
  `NotificationDrawer.svelte`.
- Reworked: `UserProfile.svelte` (preferences fieldset via
  `PUT /users/me`, delivery channels via `PUT /notifications/preferences`
  — also fixed the prefs tab to the real `{preferences, channels}`
  contract; Sessions tab; pending-invitations tab),
  `WorkspaceSettings.svelte` (Members tab: role change incl.
  ownership-transfer reason prompt, remove, invite-by-username, pending
  invitations + revoke; Teams tab reworked to real team CRUD +
  membership), `TenantSettings.svelte` (TenantAdmin user list + search +
  deactivate + provisioning modal with one-time API key display),
  `Inbox.svelte` (dismiss now calls the dismiss endpoint — was mark-read).
- `web/src/lib/api.js` — all client methods map 1:1 onto routes that
  exist in `crates/gyre-server/src/api/mod.rs` (verified by grep:
  `/users/:username`, `/users` list/provision, `/users/:id/deactivate`,
  `/users/me/sessions` + revoke + revoke-all, `/notifications` +
  `unread-count` + `mark-all-read` + per-id `read`/`unread`/`dismiss`,
  `/workspaces/:id/members/:user_id/role`, `/invite`, `/invite/accept`,
  `/invitations`, `/invitations/:invitation_id`, teams CRUD + members).
- Backend additions: `NotificationRepository::list_active_for_user`
  (port + SQLite/PostgreSQL/mem + broadcasting wrapper; `?active=true`
  on `GET /api/v1/notifications`) and invitee self-decline on
  `DELETE /workspaces/:id/invitations/:invitation_id` (404-masked for
  foreign invitations, distinct audit event).
- Spec lifecycle: `ui-navigation.md` §1 bell amendment (dated note,
  supersedes the old decisions-count badge click), `docs/ui.md` updated
  to match.

## Probes run (evidence in /tmp/stage/review-evidence)

- `cd web && npm ci && npm test` → **exit 0**: 119 files, 2403 passed,
  41 skipped.
- `/tmp/stage/bin/cargo test -p gyre-adapters list_active_for_user` →
  **exit 0**: `list_active_for_user_excludes_resolved_and_dismissed`
  passes (real SQLite storage; the test fails if the active guard is
  removed — it asserts resolved/dismissed rows are excluded while a
  read-but-active row is kept, and that `list_for_user` still returns
  all four).
- `/tmp/stage/bin/cargo check -p gyre-server --tests` → **exit 0**
  (covers the new handler tests' compilation, incl.
  `spec_path_notifications_active_filter_excludes_resolved_and_dismissed`
  and `invitee_can_decline_own_pending_invitation`).
- `python3 scripts/check-rustfmt-diff.py 103d665e` → **exit 0** — this
  was the single gate the prior verification round failed on (durable
  finding cd6bf66c); fixed by candidate commit `50f338e9`.
- i18n key audit: every static `$t('...')` key in the seven touched
  Svelte files resolves in `web/src/locales/en.json` (1974 keys).
- Endpoint-consumption audit: every `api.*` method introduced by the
  diff was matched against a registered route.

## Findings

None blocking.

## Notes

- The invitation-accept page keys the URL on the workspace id rather
  than a token: task-124 models a pending workspace invitation as a
  membership row keyed by workspace, and no token exists server-side.
  This is documented in the component, the task file, and coverage row
  31; the `/invite/{token}` magic-link form belongs to task-110
  (tenant invitations, not-started). Acceptance criteria do not require
  the token form. Same split applies to Tenant Invitation Management
  (bulk invite) and the invitation expiry countdown (no expiry field on
  `WorkspaceMembership`) — all owned by task-110's backend, none in
  this task's acceptance criteria.
- Tests are real, not self-confirming: the bell test pins that the badge
  reads `unreadNotificationCount` (mocked to 3) and *not* the legacy
  decisions count (mocked to 7); the drawer test pins
  `{ limit: 50, active: true }` on the request; InvitationAccept /
  UserProfile / WorkspaceSettings / TenantSettings tests pin exact
  endpoint calls, payloads (`user_id` resolution from `@handle`, role
  change with `{role, reason}`), 404/not-admin states, and row removal
  after revoke.
- Generated-output observation (non-blocking, verifier-owned): the
  working tree's `web/dist/` differs from HEAD (leftovers from the
  controller's earlier `checks.sh` build; `npm test` does not write
  dist). Per stage policy `web/dist/` is owned by the deterministic
  verifier; no source defect.

## Verifier must run (deferred from this sandbox)

- `cargo test --all` (full suite; gyre-server test binary exceeds the
  in-sandbox 4 GiB wrapper budget).
- Focused server tests for this task's backend additions:
  `cargo test -p gyre-server spec_path_notifications_active_filter_excludes_resolved_and_dismissed`
  and `cargo test -p gyre-server invitee_can_decline_own_pending_invitation`.
- Web e2e (`web/tests/e2e/app.spec.js` — updated to the
  `notification-bell` testid).
