# Review — task-127 (Implement user management UI pages)

Spec: `specs/system/user-management.md` §UI Pages.
Candidate under review: `e71b2e1ab62867bfadeb5aeea74f55e488eaca23` (branch
`pipeline/task-127/431b1a775d4f47a2a2bff46e1bbb0821-1`), diff against assigned
base `b8d63b0d115c1b7623480f69010f43d718e336bc`.
Verdict: **needs-revision** — one material finding (R2-F1, merge damage to the
topbar user-avatar styles); all acceptance-criterion pages verified real.

## Round 2 (this review; round 1 = the goToCrossWorkspace finding, repaired by `4aa5a87a`)

### Probes run (evidence in `/tmp/stage/review-evidence/`)

- `cd web && npm ci` → exit 0 (`npm-ci.log`).
- `cd web && npm test` (full suite) → **exit 0: 123 files, 2419 passed / 41
  skipped** (`npm-test.log`).
- `/tmp/stage/bin/cargo check -p gyre-server` → **exit 0** in 6m54s
  (`cargo-check-gyre-server.txt`); the new `NotificationRepository::
  list_active_for_user` trait method is implemented by all four implementors
  (sqlite, postgres, mem, BroadcastingNotificationRepo).
- `cargo test -p gyre-adapters list_active_for_user` → **1 passed**
  (`list_active_for_user_excludes_resolved_and_dismissed ... ok`;
  `cargo-test-adapters.txt`) — the active-only listing really excludes
  resolved/dismissed rows in the SQLite query, and a read-but-active row is kept.
- Static probe `user-btn-styles-lost.txt`: `.user-btn {` style-rule count is 1
  in base `b8d63b0d`, 1 in branch-side merge parent `62ed1272`, **0 in candidate
  `e71b2e1`** — dropped by merge commit `237f9a5b` — while the candidate
  markup still uses `class="user-btn"` (App.svelte:1793), `.auth-dot`
  (App.svelte:1806), `class:auth-active`, and focuses `.user-btn` on Escape
  (App.svelte:864).
- i18n completeness probe (`check-i18n-keys.py` → `i18n-keys.txt`): every
  `$t('...')` literal in the eight task-127-touched components resolves in
  `web/src/locales/en.json`. (The prior round's missing-keys regression is
  additionally pinned by a render-guard test in TenantSettings.test.js.)
- Rust server tests could not be compiled in this sandbox (executor wrapper:
  "gyre-server code generation plus the model exceeds 4 GiB" — exit 64,
  `cargo-test-server-focused.txt`). **Verifier must run:**
  - `cargo test -p gyre-server spec_path_notifications_active_filter_excludes_resolved_and_dismissed`
  - `cargo test -p gyre-server invitee_can_decline_own_pending_invitation`
  - `cargo test --all` (the task's own AC)
- A runtime probe attempting to assert `.user-btn` CSS injection via jsdom was
  inconclusive (Svelte component styles are not exposed as document
  styleSheets in this vitest environment — the control selector
  `.user-menu-wrap` also returned zero rules), so the finding rests on the
  static git evidence above, which is conclusive.

### Verified working (no findings)

- **All acceptance-criterion pages are real production code against live
  endpoints.** Every `api.*` call added in `api.js` maps to a registered route
  in `crates/gyre-server/src/api/mod.rs` (checked individually:
  `/users/:username`, `/users`, `/users/:id/deactivate`, `/users/me/sessions{,/:id,/revoke-all}`,
  `/workspaces/:id/members/:user_id/role`, `/workspaces/:id/invite`,
  `/workspaces/:id/invitations{,/:invitation_id}`, `/workspaces/:id/invite/accept`,
  `/workspaces/:id/teams*`, `/notifications?active=true`).
- **User Settings** (`/profile`): profile edit + preferences fieldset via
  `PUT /users/me`; notification prefs fixed to the real
  `{preferences: [...], channels: {...}}` contract of
  `PUT /notifications/preferences` (the old flat map mis-parse is gone and is
  pinned by an updated test asserting the exact payload shape); delivery
  channels (in_app locked on, email digest/min-priority, webhook, slack).
- **User Profile `/@username`**: `PublicUserProfile.svelte` renders accepted
  memberships only (pending invitations correctly hidden), team memberships
  derived from per-workspace team listings, 404 state shared for
  nonexistent/cross-tenant handles (matches the server's 404 containment).
  Routing (`/@:username`, `/invite/:id`) wired through parseUrl/urlFor/
  pushState/popstate/mount-time canonicalization, including the round-1 repair
  that preserves deep links on refresh.
- **Session Management**: sessions tab lists `GET /users/me/sessions`,
  per-session revoke, revoke-all; UA-less rows get an explicit
  WebSocket-device label; epoch-second timestamps handled per task-111's
  contract.
- **Tenant User Management**: TenantSettings Users tab — TenantAdmin-gated
  listing (403 renders a not-admin note, not an error), client-side search,
  deactivate with reason + confirm (self-deactivation hidden),
  admin provisioning through `POST /users` with server-side tenant
  containment (the UI sends the caller's own `tenant_id` from `me()`, which
  the handler validates against the caller — no forged scope).
- **Workspace Members**: new Members tab — role change incl. ownership
  transfer with a required audit reason (cancelled prompt aborts), remove
  member, invite-by-username resolved through the tenant-public profile,
  pending invitations with revoke. The member list correctly unwraps the
  `{members}` envelope (previously broken).
- **Team Management**: Teams tab reworked from a duplicate member list to
  real team CRUD + membership over the task-122 endpoints.
- **Invitation Accept page**: workspace preview, accept, decline; decline is
  the invitee self-decline path — the backend change (`revoke_invitation`
  allowing `caller_id == membership.user_id`, 404-masking foreign
  invitations, distinct `workspace_invitation_declined` audit event) is real
  per-handler authorization with a hard test (`invitee_can_decline_own_pending_invitation`,
  deferred to verifier per the build wrapper).
- **Notification Drawer**: bell badge = unread count from
  `GET /notifications/unread-count` (pinned by a fetch-level mock test that
  proves the decisions-count endpoint does NOT drive the badge), active-only
  `?active=true` listing (server-enforced by `list_active_for_user` in all
  adapters), priority grouping, unread pill, deep links via the app's
  `navigateToPath` context, no-fabricated-target disable, and
  InvitationReceived → `/invite/{workspace}` deep link. The old decisions
  badge removal is covered by a dated spec-lifecycle amendment to
  `ui-navigation.md` §1 (spec amended rather than contradicted — goal rule 4
  satisfied) with `docs/ui.md` updated consistently, and the e2e shell test
  re-pinned to the bell.
- **Test quality**: no test inflation found. New tests are behavioral (real
  components, endpoint-call assertions with exact arguments, DOM state
  transitions, exact tab-id lists); the round-1 regression test
  (`InvitationDeclineNavigate.test.js`) renders the real App and was
  verified by its author to fail with the fix stashed.
- **Coverage honesty**: `specs/coverage/system/user-management.md` row 31
  stays `task-assigned` (not falsely `implemented`), documenting exactly what
  remains blocked on missing APIs (tenant invitations/task-110, expiry
  countdown, activity feed, inline global-role editing).
- Generated `web/dist/` observations are out of review scope per the stage
  contract (verifier-owned).

### Findings

- [major] **R2-F1 (code, merge damage): the topbar user-avatar button's scoped
  styles were dropped by merge commit `237f9a5b`, while the markup still uses
  them.** In candidate `e71b2e1:web/src/App.svelte`, the `<style>` rules
  `.user-btn { ... }` (32px circular elevated button), `.user-btn:hover`,
  `.user-btn:focus-visible`, `.auth-dot { ... }` (positioned 8px status dot),
  and `.user-btn.auth-active .auth-dot` (green/red auth indicator) are all
  gone — they exist in base `b8d63b0d` and in both parents of the merge
  (`62ed1272` branch side, `b8d63b0d` main side), so the conflict resolution
  of the topbar region (removing the decisions badge) silently deleted the
  adjacent user-avatar rules too. The candidate markup still renders
  `class="user-btn"` (line 1793) with `class:auth-active` and a child
  `<span class="auth-dot">` (line 1806), the Escape handler focuses
  `.user-btn` (line 864), and the reduced-motion media query still lists
  `.user-btn, .auth-dot` (lines 2945-2946) — dead selectors referencing
  undefined classes. User-visible consequences: the primary user-menu button
  loses its sizing/circular/elevated design and its `:focus-visible` outline
  (an accessibility regression for keyboard users), and the auth-status dot
  becomes an invisible zero-size inline span (no width/height/position/
  background), so the logged-in/logged-out indicator disappears entirely.
  No test catches it (jsdom/vitest does not exercise scoped CSS).
  Reproduce: `git show e71b2e1a:web/src/App.svelte | grep -c '\.user-btn {'`
  → 0 (vs 1 at base); compare markup lines 1793/1806.
  **Repair:** restore the five rule blocks removed in the `237f9a5b` hunk
  (from `git show b8d63b0d:web/src/App.svelte`, lines ~2444-2481) into
  App.svelte's `<style>`, and drop or keep the reduced-motion selectors as
  appropriate once the classes are defined again.

### Verifier must run (recorded)

1. `cd web && npm ci && npm test` (re-run on integrated tree).
2. `cargo test -p gyre-server spec_path_notifications_active_filter_excludes_resolved_and_dismissed`
3. `cargo test -p gyre-server invitee_can_decline_own_pending_invitation`
4. `cargo test --all` (task AC).
5. Web bundle rebuild + commit (verifier-owned stage).
