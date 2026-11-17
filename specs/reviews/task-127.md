# Review: task-127 — Implement user management UI pages

**Candidate:** `2e6ae8b885001dab938709150f3ccb0b8baef1f2` (branch `pipeline/task-127/e44b48c3f32245baaebc435c45076c37-1`, base `e8f73c061f1eaad0b38024a60de8599432b7f2c8`)
**Reviewer scope:** diff `e8f73c06..2e6ae8b8` against `specs/tasks/task-127.md` and `specs/system/user-management.md` §UI Pages.
**Verdict:** APPROVED.

## What was reviewed

### Backend additions riding the UI task (all real, all needed by the UI surfaces)

- `GET /api/v1/invite/:token/preview` (`api/invitations.rs::preview_invitation`, registered in `lib.rs`): unauthenticated magic-link preview. Read-only in every dimension — an expired-but-Pending row is *reported* Expired without persisting the transition (`effective_status`), no single-use claim, no credential material in the response. Trust model matches the existing accept/decline token routes (256-bit CSPRNG token is the auth factor). Tenant/workspace invitation resolution degrades per-row on deleted workspaces instead of 404ing the link.
- `PUT /api/v1/users/:id/role` (`api/users.rs::update_user_role`, route + ABAC `RouteResourceMapping::api(...)` entry added): TenantAdmin-only per-handler check with RBAC-denied recording, same-tenant containment (system principal exempt for bootstrap), self-change 409, deactivated-target 409, unknown role 400, idempotent no-op on same role, GlobalRole↔UserRole lockstep matching the invitation/provisioning mapping, `user_global_role_changed` audit event.
- `?active=true` on `GET /api/v1/notifications`: new `list_active_for_user` port method with the active guard (resolved/dismissed excluded) enforced **in the query** in all four implementations (SQLite, PostgreSQL, mem — guarded in code per the mem port-contract invariant — and the `BroadcastingNotificationRepo` delegation). The default listing keeps the full row set the Inbox's show-dismissed toggle depends on; the drawer branch deliberately skips the disabled-types exclusion so it agrees with the unread-count badge (documented in-code).

### Frontend

- `InvitationAccept.svelte` + `App.svelte` routing: `/invite/{token}` (64-hex magic link) and `/invite/{workspaceId}` (UUID) are disambiguated by segment shape in `parseUrl`, round-trip through `urlFor`/canonical-URL rewrite (deep links survive refresh), and are handled in popstate/applyParsed/navigateToPath. Token-flow calls go through `api.js` `skipAuth` — no Bearer header on preview/accept/decline (a logged-in different identity must not burn someone else's invitation; asserted by dedicated api.test.js tests on real fetch calls). Local-mode tenant accept displays the minted first API key exactly once with a copy action; terminal-status links render a none state; 404 and non-404 load failures are distinct states.
- TenantSettings: Users tab (search, deactivate, provision, inline global-role select hidden for self/deactivated rows) and Invitations tab (status filter, single + bulk invite, revoke, one-time invite_url with copy) — all consuming the task-110 endpoints.
- WorkspaceSettings: member role badges, role-change dropdown with mandatory reason on Owner transitions (ownership transfer), invite by username resolution, pending invitations with status badges + expiry countdown + revoke gated to Pending rows, team create/edit/membership.
- UserProfile: session management (list/revoke/revoke-all) over the task-111 endpoints; drawer gains active-only listing (`?active=true`), unread pill, `InvitationReceived` deep link to `/invite/{workspaceId}`, and no fabricated targets (button disabled when a notification has no action_url and no invitation workspace).
- `ui-navigation.md` §1 bell amendment (bell supersedes the decisions-count badge click) is recorded as a spec amendment with decision reference, and `docs/ui.md`/e2e were updated consistently.

### Test quality (read, not just run)

Component tests assert exact endpoint calls, arguments, rendered states, and terminal-status handling; api.test.js asserts exact URLs, methods, and header *presence/absence* on real fetch spies; AppShell tests cover both `/invite/` shapes (UUID id, 64-hex token, non-hex 64-char input) plus round-tripping. No mirrored-logic, assertionless, or conditional-guard patterns (all repo anti-fake-test scripts pass). Expiry assertions freeze `Date.now()` to avoid the 29m/30m race.

## Probe evidence (`/tmp/stage/review-evidence/`)

1. **All 96 `scripts/check-*.sh` invariants: exit 0** (per-script logs saved). Includes arch, ABAC route registry + exempt handlers, mem port contracts, migration versions/SQL portability, forged scope fields, in-memory state stores, task-commit attribution.
2. **`cargo check -p gyre-server --lib --tests` (SKIP_WEB_BUILD=1, /tmp/stage/bin/cargo): exit 0** — 7m42s cold check; log saved as `cargo-check-gyre-server.log`.
3. **Focused vitest (`npm ci` first, locked deps), 9 files: 391/391 passed** — InvitationAccept, InvitationDeclineNavigate, NotificationDrawer, TenantSettings, UserProfile, WorkspaceSettings, api, AppShell, Inbox. Command and tail output in `focused-vitest.log` (this file records the run summarized above; full stdout was shown in the review session).

## Deferred to the independent verifier (4 GiB build budget)

- `cd web && npm test` (full suite; implementation reports 2494 passed, not re-run here in full).
- `cargo test --all` — at minimum the new server tests: `preview_resolves_tenant_invitation_with_preassigned_workspaces` + the other three preview tests in `api/invitations.rs`, `update_user_role_changes_global_and_user_roles_in_lockstep` + the containment test in `api/users.rs`, and `spec_path_notifications_active_filter_excludes_resolved_and_dismissed`. Server test compilation was type-checked here (probe 2).

## Non-findings (recorded, not blocking)

- `web/dist/` was not regenerated in this candidate — owned by the deterministic verifier per checkpoint policy.
- The working tree's `specs/tasks/task-127.md` is staged by the executor at the pristine assignment frontmatter (`not-started`, empty `commits`) while the candidate commit carries the real attribution + Shipped section; this is review-environment staging, not a candidate defect.
