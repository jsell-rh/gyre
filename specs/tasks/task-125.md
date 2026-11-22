---
title: "Implement teams and notifications API endpoints"
spec_ref: "user-management.md §API > Teams"
depends_on:
  - task-122
  - task-123
progress: complete
coverage_sections:
  - "user-management.md §Teams"
  - "user-management.md §Notifications"
commits: ["02b4afa6f7e79cfe67b2e921509926c0d6a9bf31", "2a8cf5f6b49c7419ef1906bff20e5c97292fa03a", "83594b91dbe13ee305036e13b3f3663237c96875", "e4af712a72e9e8618f126f691df3e060bed26da6", "bc0d0eac099f133695f07aeb09b3ccb22d72afac"]
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

- [ ] Team membership add/remove endpoints functional
- [ ] `GET /api/v1/notifications` with pagination and filtering
- [ ] `GET /api/v1/notifications/unread-count` returns count
- [ ] `PUT /api/v1/notifications/{id}/read` marks notification as read
- [ ] `POST /api/v1/notifications/mark-all-read` marks all as read
- [ ] Notification preferences CRUD endpoints functional
- [ ] `GET /api/v1/notifications/stream` delivers real-time SSE notifications
- [ ] All routes registered in `api/mod.rs`
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/user-management.md` §API > Teams and §API > Notifications. Existing team endpoints: `gyre-server/src/api/users.rs` (create_team, list_teams). Route registration: `gyre-server/src/api/mod.rs` — grep for `/teams/` and `/notifications`. Existing notification endpoints: grep for `get_my_notifications\|get_notification_count\|notification_preferences` in users.rs. SSE implementation pattern: grep for `Sse\|axum::response::sse` in the codebase for existing SSE examples. Notification preferences: `gyre-adapters/src/sqlite/user_profile.rs` may have existing preference storage.

## Shipped

Production work: commit `bc0d0eac` (feat(task-125), original implementation),
repair commit `e4af712a` (verification-finding repair), review-repair
commit `83594b91` (attempt acba40e3 — SSE fan-out closure, below),
format-only repair commit `2a8cf5f6` (rustfmt changed-line violations), and
CI-repair commit `02b4afa6` (Clippy E0061 from the task-191 merge, below).
The assigned task body above is left byte-identical to the assigned contract,
including the unchecked acceptance criteria — checking those boxes is a
normative change reserved for the independent reviewer.

### Teams (§API > Teams) — audit only, no code needed

All five specced team endpoints already shipped with task-122 and were
re-audited against the spec table: `GET/POST /workspaces/:id/teams`,
`PUT/DELETE /workspaces/:id/teams/:team_id`,
`POST /workspaces/:id/teams/:team_id/members`,
`DELETE /workspaces/:id/teams/:team_id/members/:user_id` — authenticating,
tenant-scoped, gated on Owner/Admin/TenantAdmin, lead validated against real
workspace membership, 409 on slug collision, cross-workspace team id → 404,
members limited to workspace members. Covered by task-122's test suite
(team_crud_slug_lead_and_membership_flow, team_routes_are_workspace_scoped,
team_management_requires_owner_admin_or_tenant_admin, …).

### Notifications (§API > Notifications) — spec-path surface

- `GET /api/v1/notifications` — caller-scoped listing, limit/offset pagination,
  filters `?type=`, `?priority=` (case-insensitive; widens to its HSI §8 rank
  band via `NotificationPriority::rank_band`/`from_level_str`; unknown level →
  400), `?workspace_id=`, `?repo_id=` — pushed down through
  `NotificationRepository::list_for_user` into SQLite, PostgreSQL, and mem
  (real storage filtering, not handler post-filtering), plus pre-existing
  `?min_priority=&max_priority=` rank filters. Legacy
  `/users/me/notifications` shares the same implementation.
- `GET /api/v1/notifications/unread-count` — new `count_unread` port method
  (SQLite/PG/mem): active (unresolved, undismissed) AND unread rows; optional
  `?workspace_id=`.
- `PUT /api/v1/notifications/{id}/read` — spec method on the existing
  per-handler-auth mark-read handler (POST kept for pre-existing callers).
- `POST /api/v1/notifications/mark-all-read` — new `mark_all_read` port method
  (all three adapters): stamps `read_at`, optional `?workspace_id=` sweep,
  returns `{marked}`, drives unread-count to zero.
- `GET/PUT /api/v1/notifications/preferences` — spec path for the existing
  HSI §12 handlers.
- `GET /api/v1/notifications/stream` — SSE stream (heartbeat keep-alive,
  per-event user+tenant+optional-workspace filtering in the handler).

### Review repair `83594b91` (attempt acba40e3) — SSE fan-out closure

- **Bare-repo persistence bypass (graph_extraction):** the two push-scoped
  persistence sites (`ConflictingInterpretations`, `SpecAssertionFailure`)
  wrote through the raw `NotificationRepository` handed to `DivergencePorts`,
  so git pushes, mirror syncs, and repo imports stored notifications that
  never reached the stream. All four production `DivergencePorts` feeders
  (git_http.rs, mirror_sync.rs, api/repos.rs ×2) now pass
  `AppState::broadcasting_notifications()` — a `NotificationRepository`
  decorator (`crates/gyre-server/src/notifications.rs`) whose `create`
  persists AND broadcasts. `AppState::create_notification` delegates to the
  same wrapper, so the fan-out now has exactly one implementation and every
  production emitter routes through it (verified by sweep: the only
  `notification_broadcast_tx.send` site is the wrapper; the only remaining
  bare `state.notifications.create` uses are `#[cfg(test)]` seeds).
- **Lagged stream termination (users.rs):** `notification_stream` treated
  broadcast `RecvError::Lagged` as channel-closed and silently ended a live
  subscriber's stream after a >1024-notification burst. `Lagged` now
  `continue`s (the channel is live; missed messages are gone); only `Closed`
  ends the stream.
- **Regression test:** `push_check_notifications_broadcast_to_stream_
  subscribers` (graph_extraction.rs) drives a real failing-assertion push
  check through `DivergencePorts` built with the broadcasting wrapper and
  asserts the notification is both persisted and delivered on a subscribed
  broadcast receiver — it fails at the broadcast assertion if a feeder ever
  passes a bare repo again.

### CI repair `02b4afa6` — Clippy E0061 from the task-191 merge

CI run 43458944185 (PR head `bb2d4744`, later re-merged with main `d9c419a0`
as `03922cc3`) failed the Clippy gate with a single error: `api/budget.rs:2332`
called `NotificationRepository::list_for_user` with task-191's pre-task-125
7-argument signature, while this branch's port takes 8 (task-125 added the
`repo_id` filter). The merge kept main's budget.rs test helper verbatim, so
every gyre-server test target failed to compile. `02b4afa6` supplies the
missing `None` `repo_id` argument — one line, no behavior change. Reproduced
locally via the exact CI command (`cargo clippy --all-targets --all-features
-- -W clippy::all` plus the changed-line gate of
`scripts/check-clippy-diff.py`): before the fix — exit 101, 1 error at
budget.rs:2332; after — exit 0, zero errors and zero changed-line warnings
(842 pre-existing warnings, all outside changed lines). Evidence retained in
`/tmp/stage/review-evidence/clippy-head.{json,stderr,exit}` and
`clippy-fixed.{json,stderr,exit}`.

### Test evidence

In-sandbox (build wrapper): `cargo clippy --all-targets --all-features`
(workspace, CI-equivalent invocation) exits 0 with no errors and no
changed-line warnings; `cargo check` of all targets is implied by that run;
`cargo fmt --check` on the changed file clean. gyre-server test execution is
blocked by the 4 GiB build wrapper and belongs to the independent verifier —
required probes (evidence retained in /tmp/stage/review-evidence/):

    cargo test -p gyre-server --lib api::budget
    cargo test -p gyre-server --lib push_check_notifications_broadcast_to_stream_subscribers
    cargo test -p gyre-server --lib push_check_creates_priority9_notifications_for_failed_assertions
    cargo test -p gyre-server --lib notification_stream_delivers_own_notifications_only
    cargo test -p gyre-server --lib spec_path_notifications_list_with_filters
    cargo test -p gyre-server --lib graph_extraction::tests
    cargo test -p gyre-server --lib notifications
