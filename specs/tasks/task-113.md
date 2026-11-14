---
title: "Implement in-app notification UI & email templates"
spec_ref: "user-management.md §In-App Notification UI"
depends_on:
  - task-112
progress: not-started
coverage_sections:
  - "user-management.md §In-App Notification UI"
  - "user-management.md §Email Notifications"
commits: []
---

## Spec Excerpt

From `user-management.md` §In-App Notification UI:

- **Notification bell** in top nav with unread count badge
- **Notification drawer** slides out showing recent notifications grouped by priority
- **Deep links** — clicking a notification navigates to the relevant entity (spec, MR, agent, etc.)
- **Mark read/unread** individually or bulk
- **Filter** by type, priority, workspace, repo
- **Notification preferences** page in user settings

From §Email Notifications:

- Email templates per notification type
- Digest mode aggregates notifications into a single email at configured frequency
- Priority filter prevents low-priority email noise
- Unsubscribe link per notification type
- Rendered in the user's configured locale

**API endpoints:**

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/notifications` | GET | My notifications (paginated, filterable) |
| `GET /api/v1/notifications/unread-count` | GET | Unread count (for bell badge) |
| `PUT /api/v1/notifications/{id}/read` | PUT | Mark read |
| `POST /api/v1/notifications/mark-all-read` | POST | Mark all read |
| `GET /api/v1/notifications/stream` | GET (SSE) | Real-time notification stream |

## Implementation Plan

1. **Backend API enhancements:**
   - `GET /api/v1/notifications` — add pagination (`?page=&limit=`), filtering (`?type=&priority=&workspace_id=&repo_id=&unread=true`)
   - `GET /api/v1/notifications/unread-count` — efficient count query
   - `POST /api/v1/notifications/mark-all-read` — bulk mark read
   - `GET /api/v1/notifications/stream` — SSE endpoint that pushes new notifications in real-time

2. **Svelte UI components:**
   - `NotificationBell.svelte` — bell icon in top nav with unread count badge (red dot with number)
   - `NotificationDrawer.svelte` — slide-out panel from bell click, shows recent notifications grouped by priority (Urgent first)
   - Each notification item: icon by type, title, body snippet, timestamp, action button (deep link)
   - Mark read/unread toggle on each item
   - Filter bar: type dropdown, priority dropdown, workspace dropdown
   - "Mark all read" button
   - Link to notification preferences page

3. **Notification preferences page:**
   - Route: `/settings/notifications`
   - Channel toggles (email on/off, digest frequency, min priority)
   - Webhook configuration (URL, secret)
   - Slack configuration (webhook URL, channel)
   - Per-type overrides (optional, stretch goal)

4. **Email templates:**
   - HTML email template per notification type (gate failure, budget warning, spec approval, etc.)
   - Text fallback for each
   - Digest template that aggregates multiple notifications
   - Include unsubscribe link per type
   - Use user's configured locale for rendering

5. **SSE integration:**
   - Server pushes notifications via SSE when created
   - Client subscribes on page load, updates bell badge in real-time

## Acceptance Criteria

- [ ] Notification bell with unread count badge in top nav
- [ ] Notification drawer slides out with grouped notifications
- [ ] Deep links navigate to relevant entity
- [ ] Mark read/unread (individual and bulk)
- [ ] Filtering by type, priority, workspace, repo
- [ ] Unread count API endpoint
- [ ] SSE endpoint for real-time notification push
- [ ] Notification preferences page with channel configuration
- [ ] Email templates for at least 5 notification types
- [ ] Digest mode aggregates notifications
- [ ] `cargo test --all` and `npm test` pass

## Agent Instructions

Read `specs/system/user-management.md` §In-App Notification UI and §Email Notifications. Existing notification endpoints: grep for `notifications` in `gyre-server/src/api/mod.rs`. The top nav is in `web/src/components/` — look for the app shell or layout component. Svelte 5 is used — use `$state` runes, not stores. SSE can use axum's `Sse` extractor. For email templates, check if any email infrastructure exists (grep for `lettre` or `smtp` in Cargo.toml).
