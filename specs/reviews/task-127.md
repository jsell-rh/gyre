# Review — task-127 (Implement user management UI pages) — rebase-repair round 2

Spec: `specs/system/user-management.md` §UI Pages.
Candidate: `23d8389f` (base `c9cc1536`). Round delta over the previously
reviewed candidate `8a546fec`: `c5902874` (product fix), `234937fe`/`23d8389f`
(process bookkeeping). The candidate carries the full task-127 UI work
through the main-merge `25ca4f72` (task-209 profile/notification work).

Verdict: **approved** — the sole blocking finding from the prior round
(durable finding 759a9183 / 19be1c33 rebase repair) is fixed and verified;
all focused probes pass. Full-workspace `cargo test --all` and the two
gyre-server integration tests named below are deferred to the independent
verifier per the hard 4 GiB budget rule.

## Prior-round finding and its fix

- **Finding (fixed):** the task-127 adapter test
  `list_active_for_user_excludes_resolved_and_dismissed`
  (`crates/gyre-adapters/src/sqlite/notification.rs:1015`) called
  `NotificationRepository::list_for_user` with the pre-merge 9-argument
  shape; task-209 (merged via `25ca4f72`) had added `exclude_types: &[&str]`
  as argument #8, so the crate's test build failed with error[E0061] and
  blocked the whole workspace test build.
- **Fix:** commit `c5902874` inserts `&[]` at the call site (now lines
  1015-1018) and rustfmts the call. The trait signature at
  `crates/gyre-ports/src/notification.rs:30` confirms the 10-argument shape;
  every other call site in the tree already passed `&[]`.

## Probes run (all at candidate `23d8389f`)

- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo test -p gyre-adapters --lib --no-run`
  → exit 0, finished clean (evidence:
  `cargo-adapters-test-norun-round2.txt`). The prior round's E0061 is gone.
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo test -p gyre-adapters --lib list_active_for_user_excludes_resolved_and_dismissed`
  → **1 passed** (evidence: `cargo-adapters-active-filter-test.txt`). The
  test seeds four real rows (active, resolved, dismissed, read-active) via
  the real SQLite store and asserts the exact id list
  `["n-read", "n-active"]` for the active filter and all four rows for the
  full listing — real assertions against real storage, not mirrored logic.
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server` → exit 0
  (evidence: `cargo-check-server.txt`); the merge resolution is
  type-sound on the server side.
- `cd web && npm ci` → exit 0; `npm test` (vitest run) → **123 test files,
  2426 passed / 41 skipped, 0 failed** (evidence: `npm-test.txt`).
- **Mutation probe (test-has-teeth):** replaced the drawer's priority-band
  assignment in `NotificationDrawer.svelte` `grouped` with a constant
  `'High'` → `NotificationDrawer.test.js` "groups notifications by
  Urgent first" **fails** (1 failed / 11 passed); component restored from
  backup, `git status` clean, test re-passes 12/12 (evidence:
  `mutation-drawer-grouping.txt`). The grouping test kills a real
  grouping bug, so it is not self-confirming.

## Verified working (no findings)

- **All §UI Pages surfaces present in authored source:** User Settings +
  Session Management + Notification Preferences (`UserProfile.svelte`,
  `NotificationSettings.svelte`), User Profile at `/@username`
  (`PublicUserProfile.svelte`; route parsing + URL build in `App.svelte`
  lines 32-38, 254-288, 357-360), Tenant User/Invitation Management
  (`TenantSettings.svelte`), Workspace Members/Invitations/Teams/ownership
  transfer (`WorkspaceSettings.svelte`), Invitation Accept at
  `/invite/:id` (`InvitationAccept.svelte`), Notification Drawer + bell
  unread badge (`NotificationDrawer.svelte`, `NotificationBell.svelte`,
  `notifications.svelte.js`). (My Dashboard is superseded per the spec
  note — out of scope.)
- **Svelte 5 runes** confirmed in all task components (`$state`, `$derived`,
  `$effect`), matching the acceptance criterion.
- **API wiring:** `web/src/lib/api.js` exposes the task-124/125 endpoints —
  `/users/me` (GET/PUT), `/users/{username}`, `/users` list/create,
  `/users/{id}/deactivate`, `/users/me/sessions` (+revoke, revoke-all),
  `/users/me/notifications` (+count), `/notifications` list/unread-count/
  read/unread/dismiss, `/notifications/preferences` (GET/PUT),
  `/workspaces/{id}/members|teams|invite` (+role, accept, team member
  add/remove), and the components call them.
- **Deferred-verifier tests are real integration tests:**
  `spec_path_notifications_active_filter_excludes_resolved_and_dismissed`
  (`api/users.rs:3024`) drives the real router via `oneshot` and asserts
  status codes and body contents across default/active listings;
  `invitee_can_decline_own_pending_invitation` (`api/users.rs:6669`)
  seeds real users/keys and asserts owner-invite, third-party-forbidden,
  and invitee-decline paths.

## Verifier must run (deferred from this sandbox)

Large gyre-server test compilation exceeds the review budget; the
independent verifier must execute, after the model exits:

1. `SKIP_WEB_BUILD=1 cargo test --all` (full workspace; the acceptance
   criterion "`cargo test --all` passes").
2. `cargo test -p gyre-server --lib spec_path_notifications_active_filter_excludes_resolved_and_dismissed`
3. `cargo test -p gyre-server --lib invitee_can_decline_own_pending_invitation`
4. `cd web && npm ci && npm test` (re-run in the verifier's environment).

Evidence for every probe is under `/tmp/stage/review-evidence/`.
