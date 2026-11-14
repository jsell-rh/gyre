---
title: "Implement in-app notification UI & email templates"
spec_ref: "user-management.md §In-App Notification UI"
depends_on:
  - task-112
progress: ready-for-review
coverage_sections:
  - "user-management.md §In-App Notification UI"
  - "user-management.md §Email Notifications"
commits: ["5d008fb8b7b87b637a09f5ce9f341acbefaf159c", "f25b5d1856a7e39f29cbf63f4927ca7045b3bd15", "08c58f63cfd82da292a778bb208e0f4c801c8c4e", "ed9a85cb1424069b8aecc5ce00dc60a75b0ffbc4", "d5340ae6128a6333bf2f3415ee4afd0c8d62d18e", "5ff4666790c4efa5873493db62566ee6f6b5c669", "5b8d0974a3a2f034e0bca0cd86957d2345f61cfc", "c945df274b9ad764af6651ebce77bf96c1af4cc2", "5eed7b0321d6b22191893f5b2266ec93a634ed1a", "25f0bb2d29e70cd30c959a73fb2abd7027513c67", "8cc357498e02489c2ce63b90785fe813e39daaab", "b85772c5fed727886f9ce32a6e9c349585acfc8a"]
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

## Shipped

Both coverage sections (`user-management.md` §In-App Notification UI, §Email
Notifications) are implemented on the task-112/task-125 backend (endpoints,
dispatcher, durable outbox from those tasks; this task built the UI surface
and the email template/digest layer). Coverage rows 19 and 20 are
`implemented`.

### In-App Notification UI

- **Bell** (`web/src/components/NotificationBell.svelte`) in the topbar —
  unread count from `GET /api/v1/notifications/unread-count`, badge capped
  at 99+, `aria-live` count. Realtime badge bumps ride the app's single
  authenticated `/ws` connection (`BroadcastingNotificationRepo` pushes
  `WsMessage::NotificationPush` to the owner's live connections); the
  specced SSE endpoint `GET /api/v1/notifications/stream` remains the HTTP
  API (per-event user+tenant filtered, heartbeat keep-alive).
- **Drawer** (`NotificationDrawer.svelte`): slide-out panel grouped by
  `priority_level` (Urgent → Low); per-item title / type label / body
  snippet / relative time; per-item mark read/unread
  (`PUT /api/v1/notifications/{id}/read`, `POST .../unread`); bulk
  `POST /api/v1/notifications/mark-all-read`; filter bar (type / priority /
  workspace / repo); link to preferences.
- **Deep links**: clicking an item marks it read and navigates the
  server-emitted `action_url` through the app's popstate router.
- **Preferences page** `/settings/notifications`
  (`NotificationSettings.svelte`, `notification_settings` app mode wired
  through parseUrl/urlFor/pushState/popstate/user-menu): email on/off,
  digest frequency, minimum email priority, per-type unsubscribe toggles,
  webhook (url/secret/min priority), Slack (webhook URL/channel/min
  priority) — persisted via `GET/PUT /api/v1/notifications/preferences`.

### Email Notifications (server)

- `crates/gyre-server/src/email.rs`: per-type templates for every
  `NotificationType` variant (exhaustive match — a new variant without
  copy fails to compile), HTML + plain-text fallback, per-type unsubscribe
  link, locale catalog (`en` default, `de`) resolved from `User::locale`.
- **Digest mode**: `email_digest` background job drains the durable
  `email_outbox` per user — Immediate sends individually, Hourly/Daily
  aggregate into one digest email; failed sends keep entries for retry.
- **Priority filter**: enforced at queue time (task-112) and re-checked at
  drain time against the user's current preferences.
- **Security hardening** (commits 5d008fb8, 5ff46667, 5eed7b03, 5b8d0974,
  c945df27, 25f0bb2d from prior review rounds): SMTP header/command
  injection blocked at the RFC 5322 builder and envelope gate
  (`strip_header_controls`, `valid_address`) with regression tests
  (`crlf_in_title_cannot_forge_header_lines`,
  `addresses_with_controls_are_rejected_at_the_gate`); dot-stuffing and
  reply-code parsing repairs.

### Verification repair (this round, commit b85772c5)

Independent verification of the merged tree (base 2824d9b1 via merge
7a742af6, which brought task-136 gate evaluation in) failed on exactly
one gate: rustfmt reported changed lines needing formatting in
`email.rs` (1095–1099, 1145–1151) and `notifications.rs` (1272) — test
code added by earlier rounds of this task. Clippy, architecture, ABAC,
and the web build were all green on the same tree. Fixed by running
rustfmt over both files; `git diff --word-diff` confirms the change is
line-wrapping only (identical tokens, no semantic change). Evidence
under `/tmp/stage/review-evidence/task-113-verification-repair/`.

### Test evidence

- `rustfmt --edition 2021 --check`: exit 0 on both repaired files and
  on every Rust file changed vs base 2824d9b1 (16 files).
- The failing verification already compiled and linted this exact tree
  (clippy changed-lines clean) and built the web bundle; the repair is
  formatting-only, so no recompilation was needed inside the 4 GiB
  assignment budget.
- Server tests the verifier must execute:
  `cargo test -p gyre-server --lib email::`,
  `cargo test -p gyre-server notifications`,
  `cargo test -p gyre-server impersonation`,
  `cargo test -p gyre-server --test spec_enforcement`,
  `cargo test --all`, and web e2e (`npx playwright test` against a release
  server build, GYRE_AUTH_TOKEN=e2e-test-token).
