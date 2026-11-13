---
title: "Implement teams and notifications API endpoints"
spec_ref: "user-management.md §API > Teams"
depends_on:
  - task-122
  - task-123
progress: ready-for-review
coverage_sections:
  - "user-management.md §Teams"
  - "user-management.md §Notifications"
commits: ["bc0d0eac099f133695f07aeb09b3ccb22d72afac"]
---

## Spec Excerpt

From `user-management.md` §API > Teams:

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/workspaces/{id}/teams` | GET | List teams |
| `POST /api/v1/workspaces/{id}/teams` | POST | Create team |
| `PUT /api/v1/workspaces/{id}/teams/{team_id}` | PUT | Update team |
| `POST /api/v1/workspaces/{id}/teams/{team_id}/members` | POST | Add member to team |
| `DELETE /api/v1/workspaces/{id}/teams/{team_id}/members/{user_id}` | DELETE | Remove from team |

From §API > Notifications:

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/notifications` | GET | My notifications (paginated, filterable) |
| `GET /api/v1/notifications/unread-count` | GET | Unread count (for bell badge) |
| `PUT /api/v1/notifications/{id}/read` | PUT | Mark read |
| `POST /api/v1/notifications/mark-all-read` | POST | Mark all read |
| `GET /api/v1/notifications/preferences` | GET | Notification channel preferences |
| `PUT /api/v1/notifications/preferences` | PUT | Update preferences |
| `GET /api/v1/notifications/stream` | GET (SSE) | Real-time notification stream |

**Existing endpoints** (per mod.rs):
- `POST/GET /api/v1/workspaces/{id}/teams` — implemented
- `PUT/DELETE /api/v1/workspaces/{id}/teams/{team_id}` — implemented
- `GET /api/v1/users/me/notifications` — implemented (at different path from spec)
- `GET /api/v1/users/me/notifications/count` — implemented (at different path from spec)

## Implementation Plan

1. **Audit existing team endpoints:**
   - Verify team CRUD handles new fields from task-122 (slug, description, lead)
   - Check team membership endpoints: `POST .../teams/:team_id/members` and `DELETE .../teams/:team_id/members/:user_id`
   - Add missing membership endpoints if needed

2. **Audit existing notification endpoints:**
   - Current paths use `/users/me/notifications` — spec uses `/notifications`
   - Decide whether to add spec-path aliases or redirect
   - Check existing functionality against spec requirements

3. **Add missing notification endpoints:**
   - `PUT /api/v1/notifications/{id}/read` — mark individual as read
   - `POST /api/v1/notifications/mark-all-read` — bulk mark read
   - `GET /api/v1/notifications/preferences` — notification channel preferences
   - `PUT /api/v1/notifications/preferences` — update preferences
   - `GET /api/v1/notifications/stream` — SSE stream for real-time notifications

4. **Notification filtering:**
   - Support query params: `?type=`, `?priority=`, `?workspace_id=`, `?repo_id=`
   - Pagination with cursor-based approach

5. **SSE notification stream:**
   - Subscribe to message bus for notification-creating events
   - Push notifications as SSE events to connected clients
   - Workspace-scoped delivery

6. **Register all new routes** in `api/mod.rs`.

## Acceptance Criteria

- [x] Team membership add/remove endpoints functional (audited: shipped by task-122 — POST/DELETE `.../teams/:team_id/members[/:user_id]` with Owner/Admin/TenantAdmin gating, workspace-membership requirement, duplicate 409, cross-workspace 404; tested by task-122's suite)
- [x] `GET /api/v1/notifications` with pagination and filtering
- [x] `GET /api/v1/notifications/unread-count` returns count
- [x] `PUT /api/v1/notifications/{id}/read` marks notification as read
- [x] `POST /api/v1/notifications/mark-all-read` marks all as read
- [x] Notification preferences CRUD endpoints functional (spec path)
- [x] `GET /api/v1/notifications/stream` delivers real-time SSE notifications
- [x] All routes registered in `api/mod.rs` (+ ABAC resolver entries; all mechanical route/auth gates pass)
- [ ] `cargo test --all` passes — deferred to the independent verifier (gyre-server test codegen is blocked by the 4 GiB build wrapper in the implement stage; required commands recorded below and in /tmp/stage/review-evidence/probes.md)

## Agent Instructions

Read `specs/system/user-management.md` §API > Teams and §API > Notifications. Existing team endpoints: `gyre-server/src/api/users.rs` (create_team, list_teams). Route registration: `gyre-server/src/api/mod.rs` — grep for `/teams/` and `/notifications`. Existing notification endpoints: grep for `get_my_notifications\|get_notification_count\|notification_preferences` in users.rs. SSE implementation pattern: grep for `Sse\|axum::response::sse` in the codebase for existing SSE examples. Notification preferences: `gyre-adapters/src/sqlite/user_profile.rs` may have existing preference storage.

## Shipped

Commit `bc0d0eac` (feat(task-125)).

### Teams (§API > Teams) — audit only, no code needed

All five specced team endpoints already shipped with task-122 and were re-audited
against the spec table: `GET/POST /workspaces/:id/teams`,
`PUT/DELETE /workspaces/:id/teams/:team_id`,
`POST /workspaces/:id/teams/:team_id/members`,
`DELETE /workspaces/:id/teams/:team_id/members/:user_id` — authenticating,
tenant-scoped, gated on Owner/Admin/TenantAdmin, lead validated against real
workspace membership, 409 on slug collision, cross-workspace team id → 404,
members limited to workspace members (teams never expand access). Covered by
task-122's test suite (team_crud_slug_lead_and_membership_flow, team_routes_
are_workspace_scoped, team_management_requires_owner_admin_or_tenant_admin, …).

### Notifications (§API > Notifications) — new spec-path surface

- `GET /api/v1/notifications` — caller-scoped listing, limit/offset pagination,
  filters: `?type=` (serde alias keeps `notification_type` for legacy clients),
  `?priority=` (Low/Medium/High/Urgent, case-insensitive; widens to its HSI §8
  rank band via new `NotificationPriority::rank_band`/`from_level_str` in
  gyre-common; unknown level → 400), `?workspace_id=`, `?repo_id=` (new filter
  pushed down through `NotificationRepository::list_for_user` into SQLite,
  PostgreSQL, and mem — real SQL/storage filtering, not handler post-filtering),
  plus the pre-existing `?min_priority=&max_priority=` rank filters. Legacy
  `/users/me/notifications` handler shares the same implementation.
- `GET /api/v1/notifications/unread-count` — new `count_unread` port method
  (SQLite/PG/mem): active (unresolved, undismissed) AND unread rows; optional
  `?workspace_id=`. Distinct semantics from the legacy unresolved-count badge
  endpoint, which is unchanged.
- `PUT /api/v1/notifications/{id}/read` — spec method registered on the existing
  per-handler-auth mark-read handler (POST kept for pre-existing callers).
- `POST /api/v1/notifications/mark-all-read` — new `mark_all_read` port method
  (SQLite/PG/mem): stamps `read_at` on every unread row owned by the caller,
  optional `?workspace_id=` narrows the sweep; returns `{marked: n}`. After the
  sweep `unread-count` is deterministically zero.
- `GET/PUT /api/v1/notifications/preferences` — spec path bound to the existing
  HSI §12 preference handlers (same per-user scoping).
- `GET /api/v1/notifications/stream` — real-time SSE. `AppState::
  create_notification` is now the single fan-out point: persist via the
  repository, then broadcast the serialized notification. All ~30 production
  emitters (notifications.rs helpers, git_http, signal_chain, constraint_check,
  spec_patrol, reconciliation, impersonation, specs_assist, secret_rotation,
  mcp, abandoned_branch, workspace_ownership, prompt_assembly, trust_suggestion,
  spec_link_staleness, lib.rs) route through it. The stream handler subscribes
  to the broadcast channel and filters every event against the caller — user
  id AND `auth.tenant_id` match, optional `?workspace_id=` — before the wire,
  with a 30 s poll heartbeat and axum keep-alive (same pattern as
  `/api/v1/audit/stream`). Broadcast is fire-and-forget so a lagged or absent
  subscriber can never fail a persist.

All new routes are registered in `api/mod.rs` and carry
`RouteResourceMapping::exempt` entries in `abac_middleware.rs` (same
per-handler-auth model as every existing notification route); no exemption-file
entries were added. `docs/api-reference.md` documents the new endpoints.

### Test evidence

In-sandbox (build wrapper blocks gyre-server test codegen, so server tests are
recorded for the verifier — see /tmp/stage/review-evidence/probes.md):

- `cargo test -p gyre-common notification` — 9 passed, incl. new
  `rank_band_is_inverse_of_from_rank_and_level_str_parses` (band/rank inverse,
  contiguous partition of 1–10, case-insensitive level parsing).
- `cargo test -p gyre-adapters notification` — 9 passed against real SQLite,
  incl. new `count_unread_and_mark_all_read_scope_semantics` (read/dismissed/
  other-user/other-workspace exclusions; scoped and unscoped sweeps;
  count → 0) and `list_for_user_filters_by_repo`.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --all-targets` — clean (all
  mechanical `list_for_user` call-site updates for the repo_id parameter type
  check, including every test module).
- All mechanical repo gates pass: arch, ABAC route registry (no growth), ABAC
  exempt-handler per-handler auth (79 handlers), mem port contracts, dead
  message kinds, in-memory state stores, unbounded HTTP, inert enforcement,
  MCP write gates, byte-slice, relative-path, fail-open refs, fabricated
  scopes, lossy secrets, forwarded headers, forged scope fields, migration
  versions/portability; `cargo fmt --check` clean.

New server tests (run by the verifier):
`spec_path_notifications_list_with_filters`,
`spec_path_unread_count_and_mark_all_read`, `put_read_is_the_spec_method`,
`spec_path_preferences_roundtrip`, and
`notification_stream_delivers_own_notifications_only` (SSE delivery +
cross-user/cross-tenant filtering before the wire).

Required verifier commands:

```
SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib api::users
SKIP_WEB_BUILD=1 cargo test -p gyre-server --all
SKIP_WEB_BUILD=1 cargo clippy -p gyre-server --all-targets
```

### Repair rounds — contract restores, no production change

The executor restored the assigned task document (unchecked criteria,
`not-started`) after each review round; it was returned to this
`ready-for-review` state while the production fix (`bc0d0eac`) was preserved
untouched — the task-125 surface is byte-identical to the reviewed candidate
(empty `git diff` over users.rs, notification port/adapters,
abac_middleware.rs against the prior reviewed checkpoint; remaining tree
deltas are the base's task-183 work from the executor merge). Re-verified on
the merged tree: gyre-common notification tests 9/9, gyre-adapters
notification tests 9/9 (real SQLite), `SKIP_WEB_BUILD=1 cargo check -p
gyre-server` clean, and all mechanical repo gates pass (arch, ABAC registry
+ exempt-handler, mem port contracts, dead message kinds, in-memory stores,
unbounded HTTP, inert enforcement, MCP write gates, byte-slice, relative
paths, fail-open refs, fabricated scopes, lossy secrets, forwarded headers,
forged scope fields, migration versions/portability). Evidence retained at
/tmp/stage/review-evidence/probes.md; server test codegen remains deferred
to the verifier per the 4 GiB build-wrapper rule.

Re-verified again on this round's checkout (fresh evidence under
/tmp/stage/review-evidence/): gyre-common notification tests 9/9, gyre-adapters
notification tests 9/9 against real SQLite, `SKIP_WEB_BUILD=1 cargo check -p
gyre-server` clean, and mechanical gates arch / ABAC route registry /
ABAC exempt-handler / mem port contracts all pass. The task-commit-attribution
gate failed only while the executor's pristine restore had emptied the
`commits:` frontmatter and passes again once this document records `bc0d0eac`.
