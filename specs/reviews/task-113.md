# Review — task-113 (Implement in-app notification UI & email templates)

Spec: `specs/system/user-management.md` §In-App Notification UI (row 19) + §Email Notifications (row 20).
Candidate under review: `6a84d00c` (branch `pipeline/task-113/bf956c316cf04fecbc70dae487e3f2ad-1`, merge-repair round against current main), base `44cf25bf`.
Product commits recorded in the task frontmatter at HEAD: `8cc35749` (feat), `25f0bb2d`, `5eed7b03`, `c945df27`, `5b8d0974`, `5ff46667` (review/verification repairs), `d5340ae6` (verified asset build).
Verdict: **approved** — no blocking findings. Full diff `44cf25bf..6a84d00c` inspected (all 33 files); every acceptance criterion is backed by real production code and real tests.

## Probes run (evidence: /tmp/stage/review-evidence/)

- `cd web && npm ci` — OK (locked versions).
- `cd web && npx vitest run src/__tests__/{NotificationBell,NotificationDrawer,NotificationSettings,notifications-store}.test.js` → 4 files / 20 tests passed, exit 0.
- `cd web && npm test` (full suite, acceptance criterion) → 119 files, **2371 passed / 41 skipped**, exit 0.
- `/tmp/stage/bin/cargo test -p gyre-domain` → **535 + 8 passed**, exit 0 (includes the `EmailConfig::type_overrides` serde-compat coverage).
- `/tmp/stage/bin/cargo check -p gyre-server` → clean, exit 0 (type check of `email.rs`, dispatcher change, job registration, main spawn).
- `bash scripts/check-arch.sh`, `check-byte-slice-truncation.sh`, `check-mem-port-contracts.sh`, `check-unbounded-external-http.sh` → all OK.
- Merge-repair sanity: `git grep -c '<<<<<<<' <candidate> -- '*.rs' '*.svelte' '*.js' '*.json'` → no conflict markers in source; candidate HEAD frontmatter lists all 7 task-labeled product-surface commits (`check-task-commit-attribution.sh` fails only against the stage's dirty working-tree copy of task-113.md, not the committed candidate).

## Deferred to the independent verifier (4 GiB budget — gyre-server test compilation)

- `cargo test --all` (acceptance criterion).
- `cargo test -p gyre-server email::` — per-type template rendering, locale switching, digest grouping/aggregation, RFC 2047 subject encoding + dot-stuffing, gating (type_overrides + min_priority), outbox parsing (embedded + flat), `read_reply` non-char-boundary regression, and the end-to-end drain against an in-process fake SMTP listener (`digest_drain_sends_aggregate_and_clears_outbox`: Daily window elapsed → ONE aggregate with both entries, outbox cleared, state stamped; no relay → queue-and-hold).
- `cargo test -p gyre-server notification_dispatcher` — per-type unsubscribe suppresses the email queue entry while a subscribed type queues with the embedded notification.

## What was verified (no findings)

**In-App Notification UI (row 19)**
- **Bell + badge**: `NotificationBell.svelte` in the App.svelte topbar beside the HSI Decisions badge (correctly a distinct surface — Decisions counts unresolved judgment items; the bell counts unread state). Count from `GET /api/v1/notifications/unread-count` (`get_unread_count` → `notifications.count_unread` port call), 99+ cap, `aria-live` count, live SSE badge bumps.
- **Drawer**: `NotificationDrawer.svelte` — slide-out panel (fixed right, slide-in animation, backdrop, Esc-to-close), grouped Urgent→Low by `priority_level`, per-item title/type label/body snippet/relative time, mark read/unread toggle, bulk mark-all-read (workspace-scoped when that filter is active), filter bar (type/priority/workspace/repo; repo options load for the selected workspace with a stale-response guard), live-connection dot, preferences link.
- **Deep links**: `openNotification` marks read then pushes the server-emitted `action_url` and replays the app's popstate router — tested end-to-end (`deep-links to the entity on click` asserts both pushState args and popstate dispatch).
- **Endpoints** (task-125 surface, verified present and scoped at candidate): `GET /notifications` (paginated + `?type=&priority=&workspace_id=&repo_id=` via `NotificationParams`, priority name widens to the HSI rank band), `GET /unread-count`, `PUT/POST /notifications/{id}/read`, `POST /{id}/unread`, `POST /mark-all-read`, `GET /stream` (SSE with per-event user+tenant filtering, workspace filter, heartbeat + keep-alive, lag-tolerant). Per-handler auth on every id-scoped handler (ownership + tenant match → 403).
- **Preferences page**: `/settings/notifications` route wired through parseUrl/urlFor/pushState/popstate/title/user-menu; `NotificationSettings.svelte` edits `GET/PUT /notifications/preferences` — email on/off, digest frequency, min priority, per-type unsubscribe toggles, webhook (url/secret/min priority), Slack (url/channel/min priority); `in_app` forced true server-side (`validate_channel_config` also rejects unknown `type_overrides` keys and non-absolute webhook/Slack URLs).
- **SSE client**: `lib/notifications.svelte.js` uses fetch + ReadableStream (EventSource cannot send the Authorization header), split-frame-safe incremental SSE parsing, heartbeats skipped, unread-only badge counting, idempotent start, aborted/failed stream degrades to closed status without throwing. Test suite covers split frames, auth header, unread-only counting, failed-stream fallback.
- **API client repair**: `api.markNotificationRead` previously hit the dismiss endpoint — remapped to the read endpoint; the three dismiss call-sites (Inbox/WorkspaceHome/CrossWorkspaceHome) moved to the real `dismissNotification`. This is a genuine bug fix on pre-existing surfaces, not scope creep.

**Email Notifications (row 20)**
- **Templates per type**: `email.rs` `type_heading` is an exhaustive match over every `NotificationType` variant (a new variant without copy is a compile error) × two locales; `render_notification_email` produces HTML + plain-text fallback with deep-link button, priority, timestamp.
- **Digest mode**: `render_digest_email` aggregates all pending entries into ONE email grouped by priority with per-item unsubscribe links; `run_digest_once` (registered `email_digest` job, 60 s, spawned in main.rs) drains the durable kv outbox per user — Immediate sends individually, Hourly/Daily send once per window since the last `email_digest_state` stamp; failed sends keep entries (durable retry).
- **Priority filter**: enforced at queue time (dispatcher) AND re-checked at drain against the user's CURRENT `min_priority`/enabled/per-type-unsubscribe state.
- **Unsubscribe per type**: every email links `{base_url}/settings/notifications?unsubscribe={type}`; the preferences page applies and persists `EmailConfig::type_overrides` on landing (tested), the dispatcher honors it at queue time (tested), and `validate_channel_config` rejects unknown type keys.
- **Locale**: rendered from `User::locale` (BCP-47 primary subtag → `en`/`de` catalog; unknown → English); no hardcoded prose outside the catalog; RFC 2047 encoded-word for non-ASCII subjects (tested round-trip).
- **Transport**: minimal real RFC 5321 client (EHLO/MAIL FROM/RCPT TO/DATA, dot-stuffing with exact line-structure preservation, base64-wrapped multipart/alternative, 10 s per-op timeouts) against `GYRE_SMTP_HOST`/`PORT`/`FROM`, documented in server-config.md. Unconfigured relay = queue-and-hold (tested: entries held untouched). Prior review-round repairs verified in the code: SMTP reply code parsed from bytes with a non-char-boundary guard (regression test included), trailing-CRLF dot-stuffing duplication fixed.

**Backend wiring**
- `EmailConfig::type_overrides` added with `#[serde(default, skip_serializing_if = ...)]` — pre-task-113 stored rows deserialize unchanged (flat-shape compat path updated); outbox entries embed the full notification while the flat task-112 shape still parses (tested both).
- Dispatcher test `email_queue_respects_per_type_unsubscribe` kills a real regression: an unsubscribed type suppresses the queue entry even when every global gate passes.

**Coverage matrix**: rows 19/20 → `implemented` with detailed landed-evidence notes; `grep -c '| not-started |' specs/coverage/system/user-management.md` = 0; SUMMARY.md totals recount consistently.

## Test-inflation check

None found. Frontend tests assert real endpoint calls (exact method + args), DOM state transitions, grouping order, deep-link navigation including popstate replay, filter pass-through, and the unsubscribe-link landing flow — each fails if the corresponding production behavior breaks. The backend email tests (deferred to verifier per the build wrapper) include an end-to-end drain against a real in-process TCP SMTP listener and a panic-regression test for the byte-boundary fix.

## Non-blocking observations (recorded, not findings)

- **Generated `web/dist/`**: owned by the deterministic verifier; this checkpoint's committed bundle may not reflect the current web sources. Not a source defect; the verifier rebuilds, tests, and commits the assets.
- The stage's working tree carries a dirty `specs/tasks/task-113.md` (progress/commits reverted to the pre-implementation contract) — stage setup for this review round, not part of the candidate; the committed candidate has correct attribution.
- Email locale catalog covers `en`/`de` only; unknown tags fall back to English. Spec says "rendered in the user's configured locale" without enumerating locales; the catalog is a real implementation with a clear extension point, and no locale list exists in the spec to contradict.
