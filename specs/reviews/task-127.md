# Review — task-127 (Implement user management UI pages)

Spec: `specs/system/user-management.md` §UI Pages (coverage row 31).
Candidate: `a00839114fd6e749db7f519ecc6d189483a01b50` (base `81c24bf0`).
Verdict: **approved** — no blocking findings.

## Round 1 (this review)

### Probes run (independent, evidence under /tmp/stage/review-evidence)

- `cd web && npm ci && npm test` → **123 files, 2426 passed / 41 skipped, exit 0** (`web-npm-test.txt`). Reproduces the implementer's recorded result exactly.
- **Mutation probe** (`mutation-probe-drawer.txt`): with `NotificationDrawer.svelte` `load()` params changed `{ limit: 50, active: true }` → `{ limit: 50 }`, `npx vitest run src/__tests__/NotificationDrawer.test.js` fails 1/12 ("loads the active-only listing when opened" — `toHaveBeenCalledWith({ limit: 50, active: true })`). File restored; `git status web/src/` clean afterwards. The drawer test pins the production behavior; not self-confirming.
- `cargo test -p gyre-adapters list_active_for_user` → **1 passed** (`cargo-adapters-focused.txt`): `sqlite::notification::tests::list_active_for_user_excludes_resolved_and_dismissed` — real storage query asserting resolved/dismissed rows are excluded while a read-but-active row is kept and `list_for_user` keeps all four (Inbox contract).
- `cargo check -p gyre-server` → **clean, exit 0** (`cargo-check-server.txt`).
- `bash scripts/check-arch.sh` → OK. `bash scripts/check-mem-port-contracts.sh` → OK.
- All 12 commits in the candidate's `specs/tasks/task-127.md` frontmatter verified ancestors of `a0083911` via `git merge-base --is-ancestor` (attribution scoping). (`check-task-commit-attribution.sh` itself timed out in this sandbox on full-history traversal — infrastructure, not a defect; manual check above covers the scoping question for this candidate.)

### Verified against the acceptance criteria

- **User Settings page** — `UserProfile.svelte`: profile editing (`PUT /users/me`), preferences fieldset (theme/density/diff view/feed scope/font size), notification delivery channels (`GET/PUT /notifications/preferences`), pending-invitations tab (accept via `POST .../invite/accept`, decline via `DELETE .../invitations/{id}`), link to session management. Real endpoint calls throughout (`api.updateMe`, `api.updateNotificationChannels`, `api.revokeWorkspaceInvitation`, ...).
- **Session Management** — Sessions tab: `GET /users/me/sessions`, per-session `DELETE /users/me/sessions/{id}` (row removed on 204), `POST /users/me/sessions/revoke-all`. Task-111 endpoints, real consumption.
- **User Profile at `/@username`** — `PublicUserProfile.svelte` + `App.svelte` route parsing (`parseUrl`/`urlFor`/popstate). Renders profile fields from `GET /users/{username}`, accepted-only memberships (pending invitations correctly hidden), team memberships derived from per-workspace team listings, 404 state shared by nonexistent and cross-tenant handles.
- **Tenant User Management (TenantAdmin only)** — `TenantSettings.svelte` Users tab: `GET /users` listing with search, `PUT /users/{id}/deactivate` with reason, `POST /users` provisioning. Non-admin viewers get an explicit not-admin state (the 403 is handled, not an error splash).
- **Workspace Members** — `WorkspaceSettings.svelte` Members tab: role change via `PUT .../members/{user_id}/role` with ownership-transfer audit reason (prompt-cancelled transfer aborts rather than sending without a reason), member removal, invite-by-username modal, pending invitations listing + revoke.
- **Team Management** — Teams tab reworked to real CRUD + membership over the task-122 endpoints (`createTeam`/`updateTeam`/`deleteTeam`/`addTeamMember`/`removeTeamMember`).
- **Invitation Accept page** — `/invite/{id}` (`InvitationAccept.svelte`): workspace preview, accept, decline. The path segment is the workspace id, documented in-code and in the coverage row: task-124 models a pending workspace invitation as a membership row keyed by workspace — there is no token; the spec's `/invite/{token}` magic-link form is the tenant-invitation flow (§Tenant-Level User Onboarding, `POST /api/v1/invite/{token}/accept`), owned by not-started task-110. The deferral is recorded in coverage row 31 (stays task-assigned) rather than claimed complete — honest partial delivery, not reinterpretation-by-silence.
- **Notification Drawer** — bell (`NotificationBell.svelte`, shared `notificationState` store with realtime `NotificationPush` on the app `/ws` + 60 s resync) with unread badge; `NotificationDrawer.svelte` slide-out grouped Urgent→Low by `priority_level`, per-item read/unread, mark-all-read, filter bar (type/priority/workspace/repo), deep links via the app's router with **no fabricated targets** (items without `action_url`/invitation workspace are disabled), `InvitationReceived` → `/invite/{ws}` deep link, preferences link. Backend `?active=true` routes to the new `list_active_for_user` port method (SQLite/PostgreSQL/mem all implement it; the query itself excludes `resolved_at`/`dismissed_at` non-null rows).
- **Notification Preferences** — `NotificationSettings.svelte` at `/settings/notifications` + channel config in the profile prefs tab.
- **All pages consume task-124/125 (and 111) endpoints** — verified by grep of every `api.*` call site in the touched components; no fabricated/mocked data paths in production components.
- **Svelte 5 runes** — `$state`/`$derived`/`$derived.by`/`$effect`/`$props`/`$bindable` used throughout; no legacy stores in the new code.
- **Backend addition (invitee self-decline)** — `DELETE /workspaces/{id}/invitations/{invitation_id}` now allows the invitee to decline their own pending invitation: tenant scope still enforced by `load_workspace_scoped`, authorization is manager (Owner/Admin/TenantAdmin, stored-role authoritative) OR self on the caller's own membership row, third parties get 404 masking (invitation ids are not a probe surface), distinct `workspace_invitation_declined` audit event. The server test `invitee_can_decline_own_pending_invitation` drives the real router: third-party 404, invitee 204, membership row gone. Properly scoped, no missing enforcement found.
- **Spec lifecycle handling** — the bell's supersession of the old decisions-badge click is recorded as a dated amendment in `ui-navigation.md` §1 (not an unamended contradiction), with `docs/ui.md` updated consistently.

### Test-inflation check

None found. The new suites assert endpoint calls with exact payloads (`{ limit: 50, active: true }`, mark-read PUT, mark-all-read with workspace param), exact priority-group ordering (`['Urgent','High','Medium','Low']`), pending-invitation hiding on the public profile, 404 states, and popstate-replayed deep links. The mutation probe confirms at least the drawer's central assertion fails when the behavior is disabled. The adapter test asserts exclusion semantics against real SQLite storage.

### Deferred to the independent verifier (per 4 GiB build budget)

Recorded in the candidate's `specs/tasks/task-127.md`:
- `cargo test --all` (workspace)
- gyre-server handler tests for user/session/invitation/team endpoints, in particular `api::users::tests::invitee_can_decline_own_pending_invitation`, `spec_path_notifications_active_filter_excludes_resolved_and_dismissed`

### Generated-output observation (non-blocking, verifier-owned)

`web/dist/` freshness is the deterministic verifier's responsibility per the review standing rules; not evaluated as a finding. Authored frontend source and tests are the reviewed surface.
