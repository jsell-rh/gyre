# Review — task-127 (Implement user management UI pages)

Spec: `specs/system/user-management.md` §UI Pages.
Assigned base `c9cc1536`, candidate `8a546fec` (rebase-repair round after durable
merge-conflict finding 19be1c33; merge of main's task-209/task-113 heads resolved
in `25ca4f72`/`96ee3ff0`).
Verdict: **needs-revision** — one blocking finding (F1, candidate does not compile
its own adapter tests); the frontend implementation itself is real, well-tested,
and passes the full web suite on this tree.

## Round N (2026-11-15) — rebase-resolution review

### Probes run (checkout at candidate `8a546fec`)

- `cd web && npm ci && npm test` → **123 test files, 2426 passed / 41 skipped,
  exit 0** (evidence: `/tmp/stage/review-evidence/web-npm-test.txt`).
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo test -p gyre-adapters --lib --no-run` →
  **error[E0061], exit 101** — the candidate's own test code does not compile
  (evidence: `/tmp/stage/review-evidence/cargo-adapters-test-norun.txt`).
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server` → **clean, exit 0**
  (evidence: `/tmp/stage/review-evidence/cargo-check-server.txt`).
- i18n key audit: extracted every `$t('...')` literal from the eight
  new/changed frontend files and resolved each against `src/locales/en.json` →
  **469 keys checked, 0 missing** (script `/tmp/i18n-check.js`).
- Diff inspection base→candidate for all 31 changed paths; git blame on the
  failing test lines (introduced by candidate commit `1deff0b5`).

### Verified working (no findings)

- **The UI pages are real implementations against live endpoints**, not mock
  theater: User Settings + Session Management + Notification Preferences
  (`UserProfile.svelte` — `PUT /users/me`, `GET/PUT /notifications/preferences`,
  `GET /users/me/sessions`, `DELETE /users/me/sessions/{id}`,
  `POST /users/me/sessions/revoke-all`); public profile `/@username`
  (`PublicUserProfile.svelte` via `GET /users/{username}`, accepted-memberships
  filter, per-workspace team resolution); Tenant User Management
  (`TenantSettings.svelte` Users tab — `GET /users`, `PUT /users/{id}/deactivate`,
  `POST /users` provisioning, TenantAdmin-gated listing with honest non-admin
  state); Workspace Members/Invitations + Team Management
  (`WorkspaceSettings.svelte` members & teams tabs — role change with
  ownership-transfer reason prompt, remove, invite-by-username resolved through
  the tenant-public profile, pending invitations + revoke, team CRUD + membership
  over the task-122 endpoints); Invitation Accept page `/invite/{workspaceId}`
  (`InvitationAccept.svelte` — preview + accept + invitee self-decline);
  Notification Drawer (active-only `?active=true` listing, priority grouping,
  unread pill, per-item and bulk read/unread, filters, no-fabricated-target
  disable, InvitationReceived → `/invite/{ws}` deep link).
- **Routing is complete and deep-link-safe**: `/@username` and `/invite/:id`
  wired through parseUrl/urlFor/pushState/popstate/applyParsed with
  popstate-state and URL-recovery paths for both new modes; drawer deep links
  route through a new `navigateToPath` context. Title handling and
  workspace-recovery exclusions updated consistently.
- **The bell supersession is a proper spec amendment, not a contradiction**:
  `ui-navigation.md` §1 gains a dated "Bell amendment (2026-11, task-127)" note
  recording that `user-management.md` §In-App Notification UI / §UI Pages
  supersede the old decisions-count badge click; `docs/ui.md` updated to match.
  The old Decisions badge, its styles, and its i18n usage are removed cleanly.
- **Backend additions are real and correctly scoped**: the invitee self-decline
  on `DELETE /workspaces/{id}/invitations/{invitation_id}` keeps manager
  revocation, masks foreign invitations as 404 for non-manager non-invitees
  (no probe surface), and emits a distinct audit event; `list_active_for_user`
  is a new port method implemented by SQLite/PostgreSQL/mem with the
  resolved/dismissed guard in the query itself (mem guards in code per the
  port-contract rule), plus an `?active=true` parameter on
  `GET /api/v1/notifications` that deliberately bypasses the HSI §12
  disabled-type exclusion so the drawer and its unread badge agree.
- **Test quality is genuine**: drawer tests pin the `active: true` request
  shape, grouping order, endpoint methods, and disabled no-link items;
  InvitationAccept tests assert the exact accept/decline endpoints and outcome
  states; WorkspaceSettings tests assert `updateMemberRole`/invite-resolution/
  team-CRD endpoint payloads; UserProfile sessions tests assert revoke and
  revoke-all calls. No assertionless or self-confirming tests found in the
  new suites.
- **Open sub-gaps are honestly recorded, not papered over**: coverage row 31
  stays `task-assigned` and names what has no backing API yet (tenant
  invitation management / `/invite/{token}` magic links / bulk invite →
  task-110; expiry countdown — no expiry field on WorkspaceMembership;
  public-profile activity feed/stats — no per-user endpoints; inline global
  role editing). The `/invite/{workspaceId}` (not token) route is a documented
  consequence of task-124's membership-row invitation model, with the token
  form correctly attributed to task-110.

### Findings

- [F1, blocking, code] **`crates/gyre-adapters/src/sqlite/notification.rs:1016`
  — the candidate's own test does not compile against the merged trait
  signature.** The new test `list_active_for_user_excludes_resolved_and_dismissed`
  (candidate commit `1deff0b5`) calls
  `NotificationRepository::list_for_user(&s, &user1, None, None, None, None, None, 50, 0)`
  — 9 arguments — but the trait (already at the assigned base via task-209)
  requires 10, with `exclude_types: &[&str]` as argument #8. The main-merge
  resolution `25ca4f72` updated every other call site with `&[]` but missed
  this one. `cargo test -p gyre-adapters --lib --no-run` fails (E0061, exit
  101), which blocks the whole workspace test build — the task's
  "`cargo test --all` passes" acceptance criterion cannot hold. This is
  precisely the merge-resolution defect class the rebase-repair round was
  opened for. **Fix:** insert `&[]` before `50` at line 1016 (one line), then
  the repair round must re-run the adapter test and the deferred gyre-server
  handler tests (`spec_path_notifications_active_filter_excludes_resolved_and_dismissed`,
  `invitee_can_decline_own_pending_invitation`) in the independent verifier —
  neither has ever executed against this merged tree.

### Verifier instructions (post-repair)

1. `cd web && npm ci && npm test` (expect ≥ 2426 passing, as on this tree).
2. `cargo test -p gyre-adapters --lib notification` — must compile and pass,
   including `list_active_for_user_excludes_resolved_and_dismissed`.
3. `cargo test -p gyre-server --lib users::` (or the full suite) — must pass,
   including the two new handler tests named above.
4. Standard gate scripts (`check-arch.sh`, `check-abac-route-registry.sh`,
   `check-mem-port-contracts.sh`, `check-inert-enforcement.sh`) — no new
   exemptions expected.

Generated-output note (non-blocking, per review policy): `web/dist/` state in
this checkpoint is the verifier's concern; no source-vs-bundle finding is
raised here.
