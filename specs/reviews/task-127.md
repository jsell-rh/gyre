# Review — task-127 (Implement user management UI pages)

Spec: `specs/system/user-management.md` §UI Pages (row 31).
Candidate: `a61528a8` (base `9fb371ac`). Commits under review (task frontmatter):
`1a32b4e6`, `4de47315`, `0f127bc0`, `fe84485b`, `b6966093`, `1deff0b5` — this
round is the repair `1deff0b5` closing the two findings of the prior review.
Verdict: **approved** — both findings verified closed with independent
evidence; no new defects found.

## Prior findings and their closure

### F1 — Bell badge showed decisionsCount, not the unread count; `api.unreadNotificationCount` consumed by nothing; unamended spec contradiction

**Closed.** `web/src/App.svelte` now keys the bell badge, aria-label, and
sr-only live region on `unreadCount`, loaded via `api.unreadNotificationCount`
(`GET /api/v1/notifications/unread-count` → `get_unread_count` → real
`count_unread` port: active ∧ unread). Refreshes on init, workspace change,
the 60s interval, and on drawer close (`$effect` cleanup on the open→false
transition). `decisionsCount` survives only as the workspace-home Decisions
section prop. The route is registered (api/mod.rs:975) with a
`RouteResourceMapping::exempt` entry (abac_middleware.rs:711) — no
exemption-file growth.

The spec contradiction is reconciled via a **dated spec-lifecycle amendment**
in `specs/system/ui-navigation.md` (§1 principle 4, Top Bar bullet, amendment
note, repo-mode bullet, Daily/Exception flows); `docs/ui.md` matches;
`user-management.md` §In-App Notification UI / §UI Pages is the superseding
spec. No unamended "decisions badge" wording remains.

**Mutation probe:** reverting the badge to `decisionsCount` in App.svelte
(temporarily, restored) makes the new AppShell test fail —
`× badge shows the unread count from GET /notifications/unread-count, not
the decisions count` — because the test mocks `unreadNotificationCount` → 3
vs `notificationCount` → 7 and asserts the rendered badge text is '3'. The
test genuinely discriminates the wired behavior.

### F2 — Drawer listed dismissed/resolved notifications (no active filter)

**Closed.** New `NotificationRepository::list_active_for_user` port method,
implemented by all four implementors: SQLite and PostgreSQL adapters
(`resolved_at IS NULL AND dismissed_at IS NULL` in the query, same
filters/pagination/ordering as `list_for_user`), the mem adapter (code
guard), and `BroadcastingNotificationRepo` (delegation). Handler dispatch:
`GET /api/v1/notifications?active=true` routes to it; the default listing is
unchanged, preserving the Inbox's show-dismissed toggle contract. The drawer
requests `{ limit: 50, active: true }` (drawer request-shape test pins it).

**Adapter probe:** `cargo test -p gyre-adapters
list_active_for_user_excludes_resolved_and_dismissed` → ok (real tempdir
SQLite: resolved and dismissed rows excluded, read-but-active kept,
`list_for_user` still returns all rows).

## Probes (evidence: /tmp/stage/review-evidence/task-127-review-round-probes.md)

- `cd web && npm ci`; `npx vitest run` on AppShell + NotificationDrawer +
  Inbox test files → 160/160 passed.
- Mutation probe of the bell badge test → test fails under the reverted
  wiring (discriminating); source restored, tree clean at candidate.
- `cargo test -p gyre-adapters list_active_for_user` → 1 passed.
- `cargo check -p gyre-server --tests` → clean (covers port, adapters, mem,
  broadcasting wrapper, `NotificationParams.active` dispatch, and the new
  handler test's compilation).
- Full `cd web && npm test` → 119 files / 2403 passed, 41 pre-existing skips
  (matches the implementation claim exactly).
- Grep sweep: no stale `topbar.decisions_pending`/`no_decisions` keys or
  `decisions-badge` testid references remain (only cosmetic CSS class names
  `.decisions-badge-btn`).

## Acceptance criteria (verified at candidate)

All pages present as real Svelte 5 runes components against task-124/125
endpoints: User Settings + Sessions tab (GET/DELETE `/users/me/sessions`,
revoke-all), `/@username` public profile, Tenant User Management
(TenantAdmin-gated Users tab), Workspace Members (invite, role change,
ownership-transfer reason prompt, pending invitations + revoke, invitee
self-decline), Team Management (Teams tab, real CRUD + membership),
Invitation Accept `/invite/:id` (workspace-keyed per task-124's membership
model; token form is task-110), Notification Drawer (unread badge, priority
grouping, deep links, mark read/unread, active-only listing), Notification
Preferences (per-type toggles + delivery channels).

Coverage honesty: row 31 stays `task-assigned` with the repair recorded and
the open rows (tenant invitations / `/invite/{token}` token form / expiry
countdown — task-110 backend; public-profile activity feed; inline
global-role editing — no endpoints) documented with their missing-backend
reasons. No fake completion.

## Deferred to the independent verifier

- `cargo test -p gyre-server spec_path_notifications_active_filter_excludes_resolved_and_dismissed`
  (gyre-server test binary exceeds the in-sandbox 4 GiB wrapper budget;
  type-checked clean here)
- Full `cargo test --all` and web e2e
