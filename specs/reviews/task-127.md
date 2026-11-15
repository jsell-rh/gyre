# Review — task-127 (Implement user management UI pages)

Spec: `user-management.md` §UI Pages (canonical), amended `ui-navigation.md` §1 (dated bell amendment).
Candidate under review: `2cc6ab93` (branch `pipeline/task-127/f94725959caf4ad18576551feb35c0ec-1`) against assigned base `706bc7bd` (task-144 attestation head). Product commits in the branch history (recorded in the task's `commits:` frontmatter before this round's reset): `8d72ffec`, `4aa5a87a`, `64c85ec9`, `50f338e9`, `1deff0b5`, `b6966093`, `fe84485b`, `0f127bc0`, `4de47315`, `1a32b4e6`.
Verdict: **approved** (this round). Prior rounds' findings (below) verified repaired.

## This round's diff (base `706bc7bd` → candidate `2cc6ab93`)

Merge `fe4782f3` (base into branch) + review repair `8d72ffec` (restore topbar user-btn/auth-dot styles lost in an earlier merge) + process commits. Prior rounds shipped the pages; this round is merge reconciliation plus the style repair.

## Prior durable findings — repair verification

- **goToCrossWorkspace no-op (InvitationAccept declined state):** FIXED. `App.svelte:791` now sets `setContext('goToCrossWorkspace', ...)`; `web/src/__tests__/InvitationDeclineNavigate.test.js` renders the REAL `App` at `/invite/ws-9`, declines, clicks `invite-go-all`, and asserts `window.location.pathname === '/all'` and `history.state.mode === 'cross_workspace'` — an end-to-end regression test that fails if the ancestor wiring is removed again.
- **`.user-btn`/`.auth-dot` scoped styles lost in merge:** FIXED by `8d72ffec`. `grep -c '\.user-btn {' web/src/App.svelte` → 1; `.auth-dot` rules present (App.svelte:2551, 2563); markup at :1806, Escape-focus handler, and reduced-motion selectors all resolvable again.

## Evidence (commands + outputs under /tmp/stage/review-evidence)

- `cd web && npm ci && npm test` → **2419 passed, 41 skipped, 0 failed** (npm-test.txt; run twice, both green).
- `/tmp/stage/bin/cargo check -p gyre-server` → **Finished dev profile, no errors** (cargo-check-server.txt).
- `/tmp/stage/bin/cargo test -p gyre-adapters list_active_for_user` → **1 passed** (`sqlite::notification::tests::list_active_for_user_excludes_resolved_and_dismissed`, real SQLite storage; cargo-test-adapters-active.txt).
- Mechanical checks pass (run-checks-summary.txt): check-arch, check-abac-route-registry, check-abac-exempt-handlers, check-mem-port-contracts, check-migration-versions, check-migration-sql-portability, check-forged-scope-fields, check-forwarded-header-trust, check-in-memory-state-stores, check-unbounded-external-http, check-mcp-write-tools, check-inert-enforcement, check-scope-literal-defaults, check-fabricated-scope-defaults, check-lossy-secret-conversion, check-fail-open-ref-resolution, check-byte-slice-truncation, check-relative-path-defaults, check-dead-message-kinds, check-noop-callbacks, check-placeholder-stubs, check-stale-stub-markers, check-conditional-test-guards, check-phantom-test-apis-js, check-stale-mechanism-claims-js, check-exhaustive-ui-state.
- i18n integrity probe (i18n-check.js/.txt): every literal `$t('key')` in `web/src` (non-test) resolves in `src/locales/en.json` → **ALL KEYS PRESENT** (guards against the merge-damaged-key class seen in earlier rounds).

## Verified working (no findings)

- **All acceptance-criterion pages ship against real endpoints** (grep-verified call sites):
  - User Settings: `UserProfile.svelte` — profile edit via `PUT /users/me`, preferences, delivery channels via `PUT /notifications/preferences`; Sessions tab via `GET /users/me/sessions`, per-session `DELETE .../sessions/{id}`, sign-out-everywhere `POST .../sessions/revoke-all` (task-111 endpoints).
  - User Profile `/@username`: `PublicUserProfile.svelte` + `parseUrl`/`urlFor`/`pushState`/`popstate` wiring in App.svelte (page-load, in-app navigation, back/forward, URL canonicalization all handled); tenant containment 404 state.
  - Tenant User Management: `TenantSettings.svelte` Users tab — `GET /users` (TenantAdmin-only server-side; UI degrades gracefully via `usersNotAdmin`), deactivate with reason (`PUT /users/{id}/deactivate`, self-deactivation hidden), provisioning (`POST /users`, tenant_id derived from the caller's own `me()`, not caller-supplied).
  - Workspace Members: `WorkspaceSettings.svelte` Members tab — member list, role change via `PUT .../members/{user_id}/role` with an audit-reason prompt on any Owner-role change (ownership transfer), remove, invite-by-username (handle resolved via `GET /users/{username}` then `POST .../invite`), pending invitations list + revoke.
  - Team Management: WorkspaceSettings Teams tab — real team CRUD + membership over the task-122 endpoints (`createTeam`/`updateTeam`/`deleteTeam`/`addTeamMember`/`removeTeamMember`).
  - Invitation Accept page `/invite/{id}`: `InvitationAccept.svelte` — workspace preview, accept (`POST .../invite/accept`), decline (invitee self-decline). Server change in this task: `DELETE /workspaces/{id}/invitations/{invitation_id}` now allows invitee self-decline (§Workspace Invitation Flow step 4), 404-masked for third parties (verified authorization logic: tenant-scoped workspace load → manager-or-invitee gate → 404 for foreign invitations), distinct `workspace_invitation_declined` audit event; server test `invitee_can_decline_own_pending_invitation` covers third-party 404 + invitee decline.
  - Notification Drawer: `NotificationDrawer.svelte` — active-only listing (`?active=true` → new `list_active_for_user` port method, implemented in SQLite/PostgreSQL/mem with query-level resolved/dismissed exclusion; adapter test passes), priority grouping Urgent→Low, unread pill, per-item read/unread, mark-all-read, type/priority/workspace/repo filters, deep links via the app's `navigateToPath` (InvitationReceived → `/invite/{ws}`), no fabricated targets (button disabled when no action_url), preferences link. Bell (`NotificationBell.svelte`) shows unread count from `GET /notifications/unread-count`, kept live by WS `NotificationPush` + 60s resync.
- **Svelte 5 runes** used throughout (`$state`, `$derived`/`$derived.by`, `$effect`, `$props`, `$bindable`).
- **Bell/decisions-badge supersession is spec-amended, not smuggled:** `ui-navigation.md` §1 carries a dated task-127 amendment; `docs/ui.md` updated consistently; AppShell tests re-pinned.
- **Coverage honesty:** row 31 (§UI Pages) stays `task-assigned` — the note explicitly lists what remains open and why (tenant invitation management + `/invite/{token}` magic links + bulk invite + expiry countdown depend on task-110, which is not-started and outside this task's dependency set; public-profile activity feed/stats and inline global-role editing have no backing endpoint). No `implemented`/`verified` claim over unshipped behavior.

## Non-findings (checked, judged acceptable)

- `/invite/{token}` vs `/invite/{workspaceId}`: the task-124 API this task is scoped to (depends_on: task-124, task-125) models a pending workspace invitation as a membership row keyed by workspace — there is no token; the token form belongs to the tenant-invitation flow (task-110, §API `POST /api/v1/invite/{token}/accept`). The page implements the specced surface (route, workspace preview, accept/decline) against the only invitation model its declared dependencies provide, and the deviation is documented in the component, the coverage row, and kept open rather than claimed closed.
- `InvitationAccept.svelte` `loadError` branch is currently unreachable (only 404 is distinguished; other workspace-fetch failures render with a blank name) — cosmetic robustness gap, not a correctness defect; primary flows tested.
- **Environment restrictions (not code defects):** this sandbox has mawk only, so the awk-based mechanical checks (`check-self-confirming-tests`, `check-tautological-assertions-js`, `check-mirrored-logic-tests-js`, `check-assertionless-tests`, `check-dead-test-code`, `check-dead-test-variables-js`, `check-comparative-test-claims-js`, `check-self-contradicting-test-comments-js`, `check-notification-audience`, `check-notification-priority`) exit 2 without evaluating; the verifier must run them under gawk. `check-web-tests.sh` fails only because the sandbox npm wrapper appends `--maxWorkers=1` a second time (duplicate-flag vitest CLI error); the direct `npm test` run is green. `check-dead-components` flags `CanvasProbeStub`/`PipelineOverview` — both untouched by this diff (pre-existing on base `706bc7bd`). `check-dead-parameters` flags postgres/sqlite `repository.rs:17` — also untouched by this diff, pre-existing.

## Verifier must run (4 GiB build budget — deferred server test compilation)

- `cargo test --all` (acceptance criterion).
- Focused on this task's server changes: `cargo test -p gyre-server --lib api::users` (includes `invitee_can_decline_own_pending_invitation`, `get_user_by_username_is_tenant_scoped`, invitation accept/revoke suite) and `spec_path_notifications_active_filter_excludes_resolved_and_dismissed` in `api::users` (the `?active=true` handler test).
- The gawk-based mechanical checks listed above.
