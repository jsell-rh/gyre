---
title: "Implement in-app notification UI & email templates"
spec_ref: "user-management.md §In-App Notification UI"
depends_on:
  - task-112
progress: ready-for-review
coverage_sections:
  - "user-management.md §In-App Notification UI"
  - "user-management.md §Email Notifications"
commits: ["25f0bb2d29e70cd30c959a73fb2abd7027513c67", "8cc357498e02489c2ce63b90785fe813e39daaab"]
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

Both coverage sections implemented on the task-112/task-125 backend (endpoints,
dispatcher, outbox already existed from those tasks; this task built the UI
surface and the email template/digest layer).

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
- **Real-time SSE**: `web/src/lib/notifications.svelte.js` subscribes to
  `GET /api/v1/notifications/stream` via fetch-streaming (EventSource
  cannot send the Authorization header), incrementally parsing SSE frames
  (split-frame safe, heartbeats skipped) to bump the bell badge live;
  the drawer reloads on each streamed event.
- **Preferences page** `/settings/notifications`
  (`NotificationSettings.svelte`, `notification_settings` app mode wired
  through parseUrl/urlFor/pushState/popstate/title/user-menu): email
  on/off, digest frequency (Immediate/Hourly/Daily/Off), minimum email
  priority, per-type unsubscribe toggles, webhook (url/secret/min
  priority), Slack (webhook URL/channel/min priority) — persisted via
  `GET/PUT /api/v1/notifications/preferences` with `in_app` forced true.
- **Repair**: `api.markNotificationRead` was mapped to the *dismiss*
  endpoint — remapped to `PUT .../read`; the three dismiss call-sites
  (Inbox / WorkspaceHome / CrossWorkspaceHome) now call the real dismiss
  endpoint.

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
  per-type unsubscribes drop already-queued entries).
- **Unsubscribe per type**: every email links to
  `{base_url}/settings/notifications?unsubscribe={type}`; the preferences
  page applies and persists `EmailConfig::type_overrides` (new
  `#[serde(default)]` field — old rows deserialize unchanged; PUT
  validates keys parse as notification types) and the dispatcher honors
  it before queueing.
- **Transport**: minimal real RFC 5321 client (EHLO/MAIL FROM/RCPT TO/DATA,
  dot-stuffing, CRLF, base64 multipart/alternative, RFC 2047 subject
  encoding, 10 s per-op timeouts) against `GYRE_SMTP_HOST`/`PORT`/`FROM`
  (documented in `docs/server-config.md`). No relay configured =
  queue-and-hold: the durable outbox is left untouched, nothing silently
  dropped.

### Contract-repair round (this checkpoint)

- The executor restored the assigned task document after the protected
  task text drifted; this round re-records the commit attribution
  (`8cc35749` implementation + `25f0bb2d` repair) and these Shipped notes
  on top of the restored contract. The production fix itself was already
  merged with the assignment base (merge `bd4ed7cf`) and is preserved
  byte-for-byte for every task-113 file (verified:
  `git diff e5b8ce55..HEAD -- <task-113 files>` is empty).
- `25f0bb2d`: removed the never-read `details_label` field from the email
  locale `Strings` struct (dead code in the task's own `email.rs` — the
  only compiler warning in the merged tree). No rendered email output
  changes.

### Test evidence

- Web (in-sandbox, vitest — focused run on the task's four suites:
  **20 passed**): `NotificationDrawer.test.js` (priority grouping order,
  read/unread/bulk endpoint calls + state updates, deep-link pushState +
  popstate replay, filter params incl. workspace→repo chain, empty
  state), `NotificationBell.test.js` (badge 99+ cap, drawer open),
  `NotificationSettings.test.js` (config load, save payload with
  `in_app` forced, per-type unsubscribe flip, webhook/slack payload
  structure, `?unsubscribe=` landing applies the opt-out + banner),
  `notifications-store.test.js` (SSE frame parsing incl. split frames,
  unread-only counting, Authorization header, failed-stream fallback).
- Rust (in-sandbox): `cargo test -p gyre-domain` — **510 passed** (incl.
  `EmailConfig::type_overrides` serde compatibility);
  `cargo check -p gyre-server` and `cargo check -p gyre-server --tests`
  — clean, zero warnings on the merged tree.
- Local invariant checks (in-sandbox, exit 0): check-arch,
  check-abac-route-registry, check-fabricated-scope-defaults,
  check-inert-enforcement, check-task-commit-attribution (passes again
  once this round's attribution lands).
- Rust (deferred to the independent verifier per the 4 GiB build
  wrapper — the server test binary must be compiled there):
  - `cargo test -p gyre-server --lib email::` — templates per type,
    locale rendering, digest aggregation/grouping, RFC 2047/dot-stuffing,
    drain gating (type_overrides + min_priority), outbox parsing
    (embedded + flat task-112 shape), end-to-end digest drain against an
    in-process fake SMTP listener (one aggregate email, outbox cleared,
    state stamped, no-relay → queue held).
  - `cargo test -p gyre-server --lib notification_dispatcher::tests::email_queue_respects_per_type_unsubscribe`
    — per-type unsubscribe suppresses the queue entry.
  - Full gates: `cargo test --all`, `npm test` (full web suite), and the
    remaining `scripts/check-*.sh`.
