# Review — task-127 (Implement user management UI pages)

Spec: `specs/system/user-management.md` §UI Pages.
Candidate: `d1d72687` (base `9bfab36`), product commits `1a32b4e6` (pages),
`4de47315` (asset build), `0f127bc0` (review repair: i18n keys + render guard).
Verdict: **approved** — all reviewable acceptance criteria hold on independent
evidence; the two criteria that are blocked on missing backends (task-110
tenant-invitation token flow incl. `/invite/{token}` magic links and bulk
invite; task-111 session endpoints) are honestly recorded as open in the
coverage row (stays `task-assigned`) and the task file, with no stub pages —
exactly what the no-fakery rules require.

## Round 1 (2026-11-14)

Probes (evidence with full output: `/tmp/stage/review-evidence/probes.md`,
`i18n-key-sweep.txt`, `i18n-kill-test.md`):

- `cd web && npm ci && npm test` → **119 files, 2396 passed, 41 skipped, 0
  failed** (matches the implementer's recorded run plus the new guard).
- Focused vitest on all eight touched test files → 321 tests passed.
- i18n sweep of every `$t()` literal in the eight touched components against
  `en.json` → ALL KEYS PRESENT (prior finding repaired).
- Kill test: deleting `tenant_settings.users.col_role`/`.count_note` from
  `en.json` makes exactly the new render-guard test fail (1/46); file
  restored byte-identical (sha256 match against the candidate).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` and `--tests` → clean
  (server test-link/run deferred to the independent verifier: run
  `cargo test -p gyre-server invitee_can_decline_own_pending_invitation`,
  `workspace_invitation_lifecycle`, and full `cargo test --all`).
- `scripts/check-arch.sh`, `scripts/check-abac-route-registry.sh` → clean.
- API cross-check: every endpoint the new UI calls exists at the requested
  path with the parsed response shape (route table + handler bodies).

Verified:

- **User Settings** (`/profile`): profile edit + preferences fieldset via
  `PUT /users/me`; notification-preferences tab now speaks the real
  `{preferences, channels}` contract of `GET/PUT /notifications/preferences`
  (the old flat map would have been silently ignored — this round fixed it);
  Invitations tab with accept/decline.
- **User Profile** `/@username` (`PublicUserProfile.svelte`): profile,
  accepted memberships with role badges, team memberships, honest 404 state;
  routing (parseUrl/urlFor/popstate/history) handled in `App.svelte` with
  tests.
- **Tenant User Management** (TenantSettings Users tab): real TenantAdmin
  listing (403 renders a gated note, no fake list), search, deactivate with
  reason, provisioning modal; self-deactivation hidden; rows link to
  `/@username`.
- **Workspace Members** (WorkspaceSettings Members tab): role badges + inline
  role change with ownership-transfer reason prompt, remove, invite by
  @username (resolved via the public profile), pending invitations with
  revoke.
- **Team Management** (Teams tab): real CRUD + membership over task-122
  endpoints.
- **Invitation Accept** `/invite/{workspaceId}`: workspace preview, accept,
  decline; the segment is the workspace id because task-124 models pending
  invitations as membership rows — `/invite/{token}` is the tenant-invitation
  magic-link flow (spec lines 100/123-124/564) whose backend is task-110,
  not-started. Correctly left open, documented, not faked.
- **Notification Drawer**: slide-out from the bell, priority-band grouping,
  per-item read/unread + mark-all-read over the task-125 spec paths, deep
  links via `action_url` (InvitationReceived → `/invite/{ws}`), unread pill.
- **Backend addition** (invitee self-decline, §Workspace Invitation Flow
  step 4): authorization is real — manager gate OR own-membership-row check,
  404-masked for foreign pending invitations, workspace-id binding, distinct
  audit event. The new router-level test kills both failure modes (third
  party must 404; invitee decline must 204 and remove the row).
- **Svelte 5 runes** throughout ($state/$derived/$props/$bindable/$effect),
  no legacy stores in the new code.
- Test quality: no self-confirming tests found; the i18n render guard, the
  exact-tab-list guards, and the API-shape assertions all fail on the
  corresponding regression (kill test run for the i18n guard).

Non-blocking observations (recorded, no repair required):

- `revoke_invitation` checks `accepted → 409` before the manager/self-decline
  gate, so a same-tenant non-manager probing an accepted membership id gets
  409 vs 404 for pending/absent ids — technically contrary to the code
  comment's "indistinguishable" claim. The disclosed fact (membership id
  exists and is accepted) is already public to every same-tenant caller via
  `GET /users/{username}` (memberships include id + accepted), so no new
  information is exposed; ids are unguessable randoms. If touched again,
  move the authorization check above the accepted check for strictness.
- The bell's numeric badge remains the active-decisions count
  (ui-navigation.md principle 4 "the notification bell in the top bar shows
  the count globally"); the drawer itself carries the unread badge. The
  bell's `aria-label` still reads "N decisions pending" while its `title`
  now says Notifications — cosmetic inconsistency only.
- `api.unreadNotificationCount` is bound but unused (the drawer derives its
  unread count from the loaded page).
- Executor-side note: the working tree's `specs/tasks/task-127.md` was reset
  to assignment-start (progress not-started, commits []) by the checkpoint
  process — the candidate commit carries the full attribution; the
  end-of-round commit-recording step must restore it or
  `check-task-commit-attribution.sh` fails on the tree.
