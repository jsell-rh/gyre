---
title: "Implement in-app notification UI & email templates"
spec_ref: "user-management.md §In-App Notification UI"
depends_on:
  - task-112
progress: ready-for-review
coverage_sections:
  - "user-management.md §In-App Notification UI"
  - "user-management.md §Email Notifications"
commits: ["f25b5d1856a7e39f29cbf63f4927ca7045b3bd15", "08c58f63cfd82da292a778bb208e0f4c801c8c4e", "ed9a85cb1424069b8aecc5ce00dc60a75b0ffbc4", "d5340ae6128a6333bf2f3415ee4afd0c8d62d18e", "5ff4666790c4efa5873493db62566ee6f6b5c669", "5b8d0974a3a2f034e0bca0cd86957d2345f61cfc", "c945df274b9ad764af6651ebce77bf96c1af4cc2", "5eed7b0321d6b22191893f5b2266ec93a634ed1a", "25f0bb2d29e70cd30c959a73fb2abd7027513c67", "8cc357498e02489c2ce63b90785fe813e39daaab", "5d008fb8b7b87b637a09f5ce9f341acbefaf159c"]
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

Both coverage sections implemented on the task-112/task-125 backend
(endpoints, dispatcher, outbox already existed from those tasks; this task
built the UI surface and the email template/digest layer). Coverage rows 19
and 20 (`user-management.md`) are `implemented`.

### In-App Notification UI

- **Bell** (`web/src/components/NotificationBell.svelte`) in the topbar beside
  the HSI §1.1 Decisions badge — intentionally a distinct surface: the
  Decisions badge counts unresolved judgment items and navigates to the
  Decisions section; the bell counts **unread** state across all
  notifications (`GET /api/v1/notifications/unread-count`) and opens the
  drawer. Badge caps at 99+, `aria-live` count for screen readers.
- **Drawer** (`NotificationDrawer.svelte`): slide-out panel grouped by
  `priority_level` (Urgent → Low), per-item title / type label / body
  snippet / relative time, per-item mark read/unread
  (`PUT /api/v1/notifications/{id}/read`, `POST .../unread`), bulk
  `POST /api/v1/notifications/mark-all-read` (workspace-scoped when that
  filter is active), filter bar (type / priority / workspace / repo — repo
  options load for the selected workspace), live-connection dot, and a link
  to the preferences page.
- **Deep links**: clicking an item marks it read and navigates the
  server-emitted `action_url` by pushing it and replaying the app's
  popstate router (works from any mode).
- **Real-time**: `web/src/lib/notifications.svelte.js` subscribes the bell
  to the app's single authenticated `/ws` connection (wired in App.svelte,
  shared with presence/liveness). The centralized
  `AppState::create_notification` fan-out (`BroadcastingNotificationRepo`)
  pushes `WsMessage::NotificationPush` to every live WS connection whose
  server-verified identity is the notification's owner, bumping the badge
  live; the drawer reloads on each pushed event.
  `GET /api/v1/notifications/stream` remains the specced SSE HTTP API
  (unchanged, per-event user+tenant filtered, heartbeat keep-alive).
- **Preferences page** `/settings/notifications`
  (`NotificationSettings.svelte`, `notification_settings` app mode wired
  through parseUrl/urlFor/pushState/popstate/title/user-menu): email
  on/off, digest frequency (Immediate/Hourly/Daily/Off), minimum email
  priority, per-type unsubscribe toggles, webhook (url/secret/min
  priority), Slack (webhook URL/channel/min priority) — persisted via
  `GET/PUT /api/v1/notifications/preferences` with `in_app` forced true.

### Email Notifications (server)

- `crates/gyre-server/src/email.rs`: per-type templates for **every**
  `NotificationType` variant (exhaustive match — a new variant without
  copy fails to compile), HTML + plain-text fallback, per-type
  unsubscribe link, locale catalog (`en` default, `de`; resolved from
  `User::locale`, all prose through the catalog).
- **Digest mode**: the `email_digest` background job (job registry +
  main.rs, 60 s) drains the durable `email_outbox` per user — Immediate
  sends individual emails, Hourly/Daily aggregate every pending entry
  into ONE digest (window tracked in kv `email_digest_state`), failed
  sends keep their entries for retry.
- **Priority filter**: enforced at queue time (task-112) and re-checked at
  drain time against the user's CURRENT config (raised thresholds /
  unsubscribes take effect on already-queued entries).

### Security repair (this round, commit 5d008fb8)

Review finding (round 2): SMTP header injection in the RFC 5322 message
builder — `encode_subject` passed ASCII subjects through with no CR/LF
stripping, and `render_notification_email` embeds `notif.title` raw;
titles carry user-controlled free text (display names via `PUT /users/me`,
workspace names at creation), so a CRLF-laden ImpersonationRequest title
forged a `Bcc:` header at the relay, copying the victim's approval-code
email to the attacker.

- `strip_header_controls` (CR/LF/NUL) applied inside `encode_subject`
  before the ASCII/encoded-word branch.
- `valid_address` gates `from`/`to` in `send_email` before any socket —
  these values are interpolated into RFC 5322 headers AND raw SMTP
  envelope commands (`MAIL FROM`/`RCPT TO`), so both header and command
  injection are blocked; `build_message` control-strips them again as
  defense in depth; `run_digest_once` drops queued entries for a user
  whose stored address can never pass the gate (warn log) instead of
  retrying forever.
- Regression tests: `crlf_in_title_cannot_forge_header_lines` (renders
  the exact attack title, asserts the header block stays exactly 5 lines
  with no forged header and CRLF stripped from the subject) and
  `addresses_with_controls_are_rejected_at_the_gate` (unit assertions +
  `send_email` failing with the gate error before any connection).

Validation this round (evidence under `/tmp/stage/review-evidence/`):

- Standalone probe `header-injection-repair-probe.rs` (functions copied
  verbatim from the repaired source; same methodology as the reviewer's
  repro): exit 0, all checks pass — including the NEGATIVE CONTROL where
  the pre-fix chain still exhibits the injected `Bcc:` header, proving
  the new assertions kill the real bug.
- `cargo check -p gyre-server` and `cargo check -p gyre-server --lib
  --tests`: exit 0 (gyre-server test builds are deferred to the verifier
  per the 4 GiB build budget).
- Mechanical checks: `check-arch.sh`, `check-byte-slice-truncation.sh`,
  `check-inert-enforcement.sh` all exit 0.

### Realtime/CI repair (commit 08c58f63)

The first published candidate failed CI with all 64 e2e tests timing out
at `waitForLoadState('networkidle')`: the client held a per-tab
fetch-streamed SSE connection to `/api/v1/notifications/stream`, and a
never-ending HTTP response permanently blocks networkidle. Fix: the
bell's realtime channel rides the app's existing single authenticated
`/ws` WebSocket (`BroadcastingNotificationRepo::create` pushes
`WsMessage::NotificationPush` to the owner's live connections); the
specced SSE endpoint is unchanged. Verified empirically with headless
Chromium (networkidle fires in ~1.5 s, zero SSE requests); `npm test`
119 files, 2371 passed / 41 pre-existing skips.

Server tests the independent verifier should run (deferred per the 4 GiB
build budget):

- `cargo test -p gyre-server --lib email::` (includes this round's two
  header-injection regression tests plus the in-process relay digest
  drain test)
- `cargo test -p gyre-server notifications` (includes
  `create_notification_pushes_to_owners_live_ws_connections_only`)
- `cargo test -p gyre-server impersonation`
- `cargo test -p gyre-server --test spec_enforcement`
- `cargo test --all`
- web e2e: `cd web && npx playwright test` (against a release server
  build, GYRE_AUTH_TOKEN=e2e-test-token)
