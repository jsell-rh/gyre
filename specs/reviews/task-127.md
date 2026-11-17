# Review: task-127 — Implement user management UI pages

**Candidate:** `1489f7405eaaf9f14855c367dc5d5d4ea1ec8c32` (branch `pipeline/task-127/16a0c4147e534509b07ab97bb822cb8e-1`)
**Base:** `312bc926398ab7627e3f41a0cb1d1ad2edb5a1ff`
**Spec:** `specs/system/user-management.md` §UI Pages (coverage row 31)
**Verdict:** ✅ Approved — independent evidence supports the task contract.

## What was reviewed

Full diff base→candidate (34 files, +5264/−232): new `PUT /api/v1/users/{id}/role`
and `GET /api/v1/invite/{token}/preview` endpoints, `?active=true` notification
listing (new `list_active_for_user` port method + SQLite/PostgreSQL/mem/broadcasting
implementations), `InvitationAccept.svelte` (both `/invite/{workspaceId}` and
`/invite/{token}` magic-link flows), TenantSettings Users/Invitations tabs,
WorkspaceSettings Members/Teams tabs, UserProfile sessions/invitations/preferences
tabs, NotificationBell/Drawer, App.svelte routing (`/@username`, `/invite/*`,
`navigateToPath` deep-link context), api.js surface (`skipAuth` for token flows),
i18n keys, tests, and the ui-navigation.md bell amendment + coverage row 31.

## Independent evidence (saved under /tmp/stage/review-evidence/)

- `probe-web-full-test.log` — `cd web && npm ci && npm test` (vitest, full run):
  **126 files, 2546 passed / 41 skipped, exit 0.** This is the task's own
  acceptance command, run from a clean `npm ci`.
- `probe-guard-scripts.log` — repo invariant lints all pass on the candidate:
  `check-abac-route-registry.sh` (new `/api/v1/users/:id/role` resolver entry
  present, no exemption growth), `check-abac-exempt-handlers.sh` (85 handlers),
  `check-mem-port-contracts.sh`, `check-inert-enforcement.sh`,
  `check-in-memory-state-stores.sh`.
- `probe-cargo-ports-check.log` — `cargo check -p gyre-ports` clean (new
  `list_active_for_user` trait method type-checks).
- Static i18n probe: every static `$t('...')` key in the touched Svelte files
  resolves in `src/locales/en.json` (a prior round's missing-key defect class).

## Key findings (all favorable)

1. **Backend is real, not cosmetic.** `update_user_role` performs a genuine
   per-handler TenantAdmin check (`is_tenant_admin` re-reads the stored record),
   same-tenant containment, self-change/deactivated-target 409s, unknown role 400,
   sets GlobalRole and the UserRole vec in lockstep, persists through the real
   user repository (SQLite and PostgreSQL `update` both write `global_role` —
   verified in adapter sources), and records a `user_global_role_changed` audit
   event. Tests assert the stored record and the audit trail, not just the HTTP
   response.
2. **Preview endpoint is read-only by construction.** `GET /invite/{token}/preview`
   reports an expired-but-Pending row as Expired without persisting the transition
   (pinned by test), leaks no token/credential material, and follows the exact
   trust model of the existing accept/decline magic-link routes (token is the
   auth factor; registered in `lib.rs` beside them).
3. **Drawer active-only listing is enforced in the query**, in all four
   implementations (SQLite, PostgreSQL, mem, broadcasting wrapper), with the
   empty-type-set-matches-nothing contract held identically to `list_for_user`.
   The deliberate divergence from the Inbox's disabled-type exclusion is
   documented and consistent with the badge count it pairs with.
4. **Frontend tests are meaningful.** They pin request shapes (endpoint URLs,
   methods, bodies, and the absence of the Bearer header on token flows —
   `api.test.js` skipAuth tests), distinguish tenant vs workspace token
   endpoints, assert negative paths (404/terminal-status/load-error render no
   accept action, wrong endpoints never called), and guard the i18n
   raw-key-leak class. The prior round's 29m/30m flake was fixed by freezing
   `Date.now` in the two whole-minute assertions without loosening them.
5. **Spec conflict handled by amendment, not reinterpreting.** The
   ui-navigation.md §1 decisions-badge behavior is superseded by a documented
   amendment reconciling it with user-management.md §In-App Notification UI /
   §UI Pages (bell = unread count → drawer), with the e2e expectations updated
   to match.
6. **Routing is sound.** 64-hex token vs UUID workspace-id disambiguation is
   shape-disjoint; magic-link URLs round-trip byte-for-byte through
   parseUrl/urlFor (pinned by AppShell test); popstate/history restore covers
   the new modes.

## Non-blocking observations (recorded, not defects)

- `TenantSettings.loadUsers` treats any listing failure (including 5xx) as the
  not-admin note rather than an error state — cosmetic UX nuance on a
  TenantAdmin-gated surface; the server contract is unchanged.
- `web/dist/` was not rebuilt by the agent — generated output is owned by the
  deterministic verifier, which rebuilds and runs all checks remotely.
- Server Rust test execution deferred to the independent verifier per the 4 GiB
  build budget (no cold gyre-server build in review).

## Verifier must run (recorded for remote verification)

- `cargo test --all` (full acceptance criterion).
- Focused: `cargo test -p gyre-server --lib api::users::tests::update_user_role`
  (lockstep + guards), `cargo test -p gyre-server --lib api::users::tests::spec_path_notifications_active_filter`,
  `cargo test -p gyre-server --lib api::invitations::tests::preview` (4 preview
  tests), `cargo test -p gyre-adapters sqlite::notification` (includes
  `list_active_for_user_excludes_resolved_and_dismissed`).
- Web rebuild + `npm test` + e2e on the rebuilt bundle (verifier-owned).
