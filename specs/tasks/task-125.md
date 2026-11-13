---
title: "Implement teams and notifications API endpoints"
spec_ref: "user-management.md §API > Teams"
depends_on:
  - task-122
  - task-123
progress: not-started
coverage_sections:
  - "user-management.md §Teams"
  - "user-management.md §Notifications"
commits: []
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
