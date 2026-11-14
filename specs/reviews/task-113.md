# Review — task-113 (Implement in-app notification UI & email templates)

Spec: `specs/system/user-management.md` §In-App Notification UI, §Email Notifications.
Candidate: `8aee60078b1a7fce5957d63ee2da4ecb02291f03` (base `2824d9b186d5da08ae043772762246ee5b620e89`).
Verdict: **approved** — no blocking findings.

## Probes

- `cd web && npm ci` → OK.
- `npx vitest run` (from `web/`) of the four new test files (NotificationBell,
  NotificationDrawer, NotificationSettings, notifications-store) → **20/20
  passed**; seven further suites covering the touched components
  (Inbox, CrossWorkspaceHome, WorkspaceHomeSections, WorkspaceCards, AppShell,
  BidirectionalNav, UserProfile) → **300 passed, 41 skipped**. (Vitest must be
  invoked from `web/` — from the repo root a config-less vitest resolves and
  all suites fail with `$state is not defined`; invocation artifact only.)
- `/tmp/stage/bin/cargo check -p gyre-server` → OK (6m 57s).
- `/tmp/stage/bin/cargo test -p gyre-domain` → 543 passed.
- Mechanical invariant scripts all green: check-arch, check-abac-route-registry,
  check-mcp-write-tools, check-dead-message-kinds, check-byte-slice-truncation,
  check-mem-port-contracts, check-in-memory-state-stores,
  check-unbounded-external-http, check-lossy-secret-conversion,
  check-fabricated-scope-defaults, check-inert-enforcement.
- Full evidence: `/tmp/stage/review-evidence/` (logs + README).

## Verified

- **Bell + drawer**: `NotificationBell.svelte` in the App topbar (distinct from
  the HSI §1.1 Decisions badge), unread badge capped 99+ with `aria-live`
  count, backed by `GET /notifications/unread-count`; `NotificationDrawer`
  slides out, groups by `priority_level` Urgent→Low, per-item mark read/unread
  (`PUT .../read`, `POST .../unread`), bulk `POST .../mark-all-read`, filter
  bar (type/priority/workspace/repo) driving the task-125 list filters, deep
  links replay the popstate router against the server `action_url`, and a
  preferences link. Frontend tests assert the exact endpoint calls, filter
  param pass-through, grouping order, and deep-link navigation — they fail if
  the behavior is unhooked.
- **Realtime**: `NotificationPush` rides the app's single authenticated `/ws`
  connection; the centralized `BroadcastingNotificationRepo::create` pushes to
  live connections matched by the **server-verified** identity recorded in
  `handle_socket` (never a client-supplied id), fire-and-forget after the
  persist. The specced SSE endpoint `GET /notifications/stream` remains, with
  per-event user+tenant filtering and heartbeat. Owner-only delivery has a
  real test (`create_notification_pushes_to_owners_live_ws_connections_only`).
  The old duplicated `push_notification_to_user` in impersonation.rs was
  removed in favor of the centralized fan-out (no double-push).
- **Preferences page**: `/settings/notifications` mode wired through
  parseUrl/urlFor/pushState/popstate and the user menu; edits email
  on/off + digest frequency + min priority + per-type unsubscribe, webhook
  (url/secret/min priority), Slack (webhook/channel/min priority) via
  `GET/PUT /notifications/preferences`; `in_app` forced true; type-override
  keys validated server-side against `NotificationType::parse`.
- **Email templates**: exhaustive per-type copy for every `NotificationType`
  variant (compile-time enforcement), HTML + text fallback, digest template
  aggregating into one email grouped by priority, per-type unsubscribe link
  (`?unsubscribe={type}`) landing on the preferences page which persists the
  opt-out, locale rendering from the user-settable `User::locale` (en/de).
- **Digest delivery**: `email.rs` `run_digest_once` — real minimal RFC 5321
  client (EHLO/MAIL FROM/RCPT TO/DATA, dot-stuffing, RFC 2047 subjects,
  base64 multipart/alternative, 10 s per-op timeouts), `email_digest` job
  registered in the registry and spawned in main.rs; Immediate sends
  individually, Hourly/Daily aggregate once per window (kv state); priority
  filter and per-type unsubscribe enforced at queue time AND re-checked at
  drain against the user's current config; queue-and-hold without
  `GYRE_SMTP_HOST` (nothing silently dropped), failed sends retried.
  Hardening from earlier review rounds is present and tested: CRLF header
  injection gates on subjects and addresses, non-char-boundary-safe SMTP
  reply parsing.
- **Repair verified**: `api.markNotificationRead` previously hit the dismiss
  endpoint; now correctly `PUT /notifications/{id}/read`, and the three
  dismiss call-sites (Inbox/WorkspaceHome/CrossWorkspaceHome) use the real
  dismiss endpoint.

## Deferred to the independent verifier (gyre-server test compilation, 4 GiB rule)

- `cargo test -p gyre-server --lib email::`
- `cargo test -p gyre-server --lib notifications::`
- `cargo test -p gyre-server --lib notification_dispatcher::`
- `cargo test -p gyre-server --lib impersonation::`
- `cargo test --all`; `cd web && npm test`; `cd web && npm run build`

## Generated-output observation (non-blocking)

`web/dist` is identical between base and candidate despite web source changes.
Per checkpoint policy the deterministic verifier rebuilds, tests, and commits
the assets — recorded separately, not a review finding.

Test-inflation check: none found. The frontend tests assert concrete endpoint
calls/args and rendered structure; the backend tests include an end-to-end
digest drain against an in-process SMTP listener asserting the wire payload,
outbox clearing, and state stamping, plus negative injection tests that expect
gate errors before any socket is opened.
