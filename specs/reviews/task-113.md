# Review — task-113 (Implement in-app notification UI & email templates)

Candidate: `46ca0565a1885253eb31602ba9eda34bb037dbeb` (base `9bfab365f2b4740c501ecd4cdf50323b374762d2`)
Spec: `specs/system/user-management.md` §In-App Notification UI (row 19) + §Email Notifications (row 20).
Verdict: **approved** — no blocking findings. Server-test execution deferred to the independent verifier (build wrapper rejects gyre-server test codegen under the 4 GiB budget); everything runnable here was run and passed.

## Probes run (all evidence under /tmp/stage/review-evidence/)

| Probe | Result |
|---|---|
| `bash scripts/check-arch.sh` | pass |
| 16 invariant scripts (ABAC registry/exempt handlers, MCP write tools, migration versions, dead message kinds, byte-slice, relative paths, fail-open refs, fabricated/lossy scope, inert enforcement, mem port contracts, forwarded headers, in-memory state, unbounded HTTP) | all pass |
| `bash scripts/check-task-commit-attribution.sh` | fails **only** against the working-tree staged contract (`specs/tasks/task-113.md` reset to `commits: []` by the controller for this review round). The candidate commit itself carries the full 11-SHA list including all 8 flagged product commits (`git show 46ca0565:specs/tasks/task-113.md`). Not a candidate defect. |
| `npm ci` (web) | ok |
| `npx vitest run` — NotificationBell/NotificationDrawer/NotificationSettings/notifications-store | 20/20 pass |
| `npm test` (full web suite) | 2384 passed, 41 skipped, 0 failed |
| `cargo check -p gyre-server` | pass (8m07s) |
| `cargo check -p gyre-server --tests` | pass (test code type-checks, incl. all new email.rs tests) |
| `cargo test -p gyre-domain` | 535 + 8 pass |
| `cargo test -p gyre-server ...` | **rejected by the build wrapper** ("gyre-server code generation plus the model exceeds 4 GiB") — deferred, see below |
| Type-taxonomy cross-check (node script) | UI `TYPES` (41) exactly equals server `NotificationType` variants (41): no dangling unsubscribe/filter type |

## Verified working

- **Bell + drawer (row 19).** `NotificationBell.svelte` in the App.svelte topbar (distinct from the HSI §1.1 Decisions badge, documented); unread badge from `GET /api/v1/notifications/unread-count`, 99+ cap, `aria-live` count. `NotificationDrawer.svelte`: slide-out panel grouped Urgent→Low by `priority_level`; per-item read/unread (`PUT .../read`, `POST .../unread`), bulk `POST .../mark-all-read`; filter bar (type/priority/workspace/repo) driving the task-125 backend list filters (`?type=&priority=&workspace_id=&repo_id=` — verified in `list_my_notifications_impl`); deep links push the server-emitted `action_url` and replay popstate routing; stale-response guard on filter races; link to preferences.
- **Realtime.** `BroadcastingNotificationRepo::create` fans out to BOTH the specced SSE channel (endpoint `GET /api/v1/notifications/stream` unchanged, per-event user+tenant filtered) and `WsMessage::NotificationPush` on the app's single `/ws` connection, matched by the **server-verified** connection identity (`ws_connection_users`, populated in ws.rs from authenticated caller) — never a client-supplied id. The impersonation-specific duplicate push was removed in favor of the centralized fan-out. In-module test asserts owner-only delivery. Client-side design decision (WS push instead of a second always-open SSE stream per tab) is documented in the coverage row; the specced SSE endpoint still exists and works — user-visible requirement (real-time badge) is met.
- **Preferences page.** `/settings/notifications` mode wired through parseUrl/urlFor/pushState/popstate/user-menu; edits `GET/PUT /api/v1/notifications/preferences` (email on/off, digest, min priority, per-type unsubscribe, webhook, Slack). `?unsubscribe={type}` email-link landing persists the opt-out and shows a confirmation banner. Server validates `type_overrides` keys against `NotificationType::parse`.
- **Email layer (row 20).** `email.rs`: exhaustive per-type template match (compile error on a new variant without copy) with HTML + text fallback; digest aggregation grouped by priority with per-item unsubscribe links; `email_digest` job (registered + spawned, 60 s) drains the durable outbox per user at their frequency (Immediate individual / Hourly-Daily one aggregate per elapsed window, kv `email_digest_state`); priority filter enforced at queue time AND re-checked at drain against current config; queue-and-hold when `GYRE_SMTP_HOST` unset; failed sends retained for retry. Locale rendering from `User::locale` (en default, de catalog, no hardcoded prose outside the catalog). SMTP client is a real RFC 5321 implementation with dot-stuffing, RFC 2047 subject encoding, base64-wrapped multipart/alternative, 10 s per-op timeouts, CRLF/NUL injection gates on subjects and addresses (with hard tests that fail on reintroduction), and byte-safe reply-code parsing. Docs updated (`docs/server-config.md`).
- **API repair.** `api.markNotificationRead` was previously the dismiss endpoint; remapped to the read endpoint, and the three legacy dismiss call-sites moved to the real dismiss endpoint.
- **Fixtures updated** (`type_overrides: Default::default()` in all `EmailConfig` literals) — serde-compatible with pre-task-113 stored rows (`#[serde(default)]`).
- **Test quality.** Frontend tests assert endpoints, payloads, grouping order, deep-link navigation, filter pass-through, unsubscribe-landing persistence, WS unread-only counting — they fail if the mapped behavior breaks. No self-confirming or assertionless tests found.

## Deferred to the independent verifier (build-wrapper restriction, not a code defect)

1. `cargo test --all` (acceptance criterion).
2. Focused server tests: `cargo test -p gyre-server --lib -- email:: notifications::tests::create_notification_pushes_to_owners_live_ws_connections_only notification_dispatcher::tests::email_queue_respects_per_type_unsubscribe impersonation` (includes the end-to-end digest drain against an in-process fake SMTP relay).
3. Web e2e suite after a fresh server + web build.

## Non-blocking observations

- **[generated output only]** The committed `web/dist/` regen commits (`ed9a85cb`, `d5340ae6`) predate the last web-source change (`08c58f63`, WS-realtime UI wiring), so the committed bundle may be stale relative to final source. Per review contract this is verifier-owned: the verifier rebuilds, tests, and commits the assets. Not a source defect.
- Locale catalog covers `en`/`de`; the app UI itself ships only `en.json`, so unknown tags falling back to English is consistent with the app surface.
