# Review — task-127 (Implement user management UI pages)

Spec: `specs/system/user-management.md` §UI Pages (rows: User Settings, Session
Management, User Profile `/@username`, Tenant User Management, Tenant Invitation
Management, Workspace Members, Workspace Invitations, Pending Invitation Accept
Page `/invite/{token}`, Team Management, Notification Drawer, Notification
Preferences).

Candidate under review: `3f44be69` (base `9f446d0e`). Verdict: **needs-revision**
— one confirmed code defect (F1); everything else probed clean.

## Round 3 (this review)

Probes run (branch `pipeline/task-127/fca63afe7f74495fb145fc2d54575b15-1`, HEAD =
candidate; the working tree's `specs/tasks/task-127.md` diff is the stage
controller's reset, not candidate content):

- `cd web && npm ci && npm test` → **124 files, 2495 passed / 41 skipped / 0 failed**.
- Focused vitest render of `InvitationAccept.svelte` with a 30-minute pending
  invitation (temporary test file, deleted after the run) → **FAILED**:
  rendered `'expires in 1800m'` (F1 below).
- `/tmp/stage/bin/cargo check -p gyre-server --lib --tests` → clean (9m18s).
- `cargo test -p gyre-adapters --lib sqlite::notification` → 10 passed, incl.
  `list_active_for_user_excludes_resolved_and_dismissed`.
- Invariant scripts (run individually; the attribution check's failure is purely
  the stage-reset working-tree task file — all 15 product-surface task-127
  commits it lists are present in the candidate's committed frontmatter):
  abac-route-registry, arch, abac-exempt-handlers, mem-port-contracts,
  forged-scope-fields, inert-enforcement, scope-literal-defaults,
  in-memory-state-stores, unbounded-external-http — all OK.
- i18n: every `$t(...)` key referenced by the diff's components resolves in
  `web/src/locales/en.json`.

Evidence: `/tmp/stage/review-evidence/` (probe-summary.md,
probe-invitation-accept-expiry.md).

### Verified working (no findings)

- **Backend additions are real and guarded.** `PUT /api/v1/users/{id}/role`
  (users.rs): TenantAdmin per-handler check with RBAC-denied recording, unknown
  role → 400, same-tenant containment (system principal exempt), self-change →
  409, deactivated target → 409, GlobalRole↔UserRole set in lockstep, audit event
  `user_global_role_changed`, idempotent no-op without audit. ABAC resolver entry
  added (`abac_middleware.rs`); registry script passes. `GET /invite/{token}/preview`
  (invitations.rs): follows the exact pre-existing magic-link pattern (registered
  in `build_router` beside token accept/decline, token = auth factor), strictly
  read-only — an expired-but-Pending row reports Expired WITHOUT persisting the
  transition (probe-tested in the candidate's own test, which asserts the stored
  row stays Pending), no credential/token material in the response, 404 on
  unknown token. `list_active_for_user` port method implemented in SQLite,
  PostgreSQL, and mem adapters with the port's empty-type-set contract, and the
  `?active=true` handler branch keeps the Inbox's full-listing contract
  unchanged. Tests are behavioral (real router via oneshot, real API-created
  invitation tokens, stored-record assertions) — not self-confirming.
- **Frontend pages satisfy the spec rows.** User Settings (`/profile`: profile
  fields + preferences + delivery channels), Session Management tab (revoke /
  revoke-all over task-111 endpoints), Tenant User Management (TenantAdmin-gated
  list/search/deactivate/provision + inline global-role select hidden for self
  and deactivated rows), Tenant Invitation Management (status filter, single +
  bulk invite, revoke, one-time invite_url with copy), Workspace Members
  (invite-by-username modal resolving via the tenant-public profile, role-select
  with ownership-transfer reason prompt, remove), Workspace Invitations (status
  badges, correct expiry countdown, revoke on Pending only), Team Management
  (create/edit/delete + membership modal), Notification Drawer (active-only
  listing, priority grouping, unread pill, deep links via `navigateToPath` with
  standalone fallback, disabled buttons for informational-only notifications),
  Notification Preferences (channel config + priority filters), and the
  `/invite/{token}` accept page (preview → accept/decline with `skipAuth` so a
  logged-in mismatched identity cannot burn someone else's invitation; local-mode
  first API key displayed once with copy). Route disambiguation of 64-hex tokens
  vs UUID workspace ids is round-trip tested in AppShell tests.
- **The bell supersession is a recorded spec amendment, not a contradiction.**
  `ui-navigation.md` §1 carries the amendment note (bell = unread count +
  drawer, superseding the decisions-count badge click); docs/ui.md and the e2e
  spec were updated consistently; the badge count source (`/notifications/unread-count`)
  is unchanged.
- **Tests are meaningful.** New api.js tests assert exact URLs, methods, payloads,
  and the absence of the Authorization header on token flows; AppShell tests
  assert parseUrl/urlFor round-trips; WorkspaceSettings/UserProfile tests kill
  the two 6aa0d0fc-round bugs (sub-hour minutes, double PUT on Save).

### Findings

- [-] **F1 (code): `InvitationAccept.svelte:73` — sub-hour expiry countdown
  renders raw seconds with a minutes unit.** `expiryLabel()`'s final branch
  returns `` `expires in ${secs}m` `` instead of `Math.floor(secs / 60)`, so a
  pending invitation expiring in 30 minutes shows "expires in 1800m" on BOTH
  accept-page flows (authenticated `/invite/{workspaceId}` and magic-link
  `/invite/{token}`). This is the same defect class as review-repair `770378cc`,
  which fixed only the `WorkspaceSettings.svelte` copy; `TenantSettings.svelte`'s
  copy is correct. Existing tests assert only element truthiness
  (InvitationAccept.test.js:85,190), so the suite stays green with the bug.
  Reproduced: focused vitest render, fixture `expires_at = now + 30*60` →
  `'expires in 1800m'` (evidence in `/tmp/stage/review-evidence/`).
  **Repair:** use `Math.floor(secs / 60)` (one line), and add the sub-hour
  value assertion to InvitationAccept tests mirroring the WorkspaceSettings
  test added in 770378cc, so the class cannot recur silently.

### Deferred to the independent verifier (4 GiB build budget)

- `cargo test --all` (task acceptance criterion).
- `cargo test -p gyre-server --lib invitations::` (preview ×4),
  `update_user_role` (×2), `spec_path_notifications_active_filter_excludes_resolved_and_dismissed`.
- `web/dist/` regeneration is verifier-owned (per checkpoint policy) — not
  reviewed, not a finding.
