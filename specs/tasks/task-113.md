---
title: "Implement in-app notification UI & email templates"
spec_ref: "user-management.md §In-App Notification UI"
depends_on:
  - task-112
progress: ready-for-review
coverage_sections:
  - "user-management.md §In-App Notification UI"
  - "user-management.md §Email Notifications"
commits:
  - COMMIT_SHA_PLACEHOLDER
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
  (split-frame safe, heartbeates skipped) to bump the bell badge live;
  the drawer reloads on each streamed event.
- **Preferences page** `/settings/notifications`
  (`NotificationSettings.svelte`, new `notification_settings` app mode wired
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

### Test evidence

- Web (run in-sandbox, `npm test` — full suite **2328 passed**, 41
  skipped, 117 files): new `NotificationDrawer.test.js` (priority
  grouping order, read/unread/bulk endpoint calls + state updates,
  deep-link pushState + popstate replay, filter params incl.
  workspace→repo chain, empty state), `NotificationBell.test.js` (badge
  99+ cap, drawer open), `NotificationSettings.test.js` (config load,
  save payload with `in_app` forced, per-type unsubscribe flip,
  webhook/slack payload structure, `?unsubscribe=` landing applies the
  opt-out + banner), `notifications-store.test.js` (SSE frame parsing
  incl. split frames, unread-only counting, Authorization header,
  failed-stream fallback).
- Rust (in-sandbox): `cargo test -p gyre-domain` — **509 passed**
  (incl. `EmailConfig::type_overrides` serde compatibility);
  `cargo check -p gyre-server --tests` — clean (all test code compiles).
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
  - Full gates: `cargo test --all`, `scripts/check-*.sh` (all local
    invariant checks already pass in-sandbox: arch, ABAC registry,
    ABAC-exempt handlers, byte-slice, relative paths, fail-open refs, mem
    contracts, scope defaults ×2, lossy secrets, inert enforcement,
    migrations ×2, message kinds, unbounded HTTP, in-memory stores,
    forwarded headers, forged scope fields, MCP write tools).
