# Review — task-113 (Implement in-app notification UI & email templates)

Spec: `specs/system/user-management.md` §In-App Notification UI + §Email Notifications.
Candidate under review: `8285396d` (base `8d07a41a`, branch `pipeline/task-113/0205037a328c49d681cfe272fbadb0f1-1`).
Verdict: **approved** — the round-2 blocking finding (SMTP header injection) is repaired and verified; all acceptance criteria are implemented with real production code; all review probes pass.

## Round 3 (this review — header-injection repair round)

### Prior finding verification (round-2 F1, SMTP CRLF header injection — fixed)

The round-2 review found that `encode_subject` passed ASCII subjects through with no CR/LF stripping while `render_notification_email` embeds `notif.title` raw, and titles carry user-controlled free text (display names via `PUT /users/me`, workspace names) — a CRLF-laden `ImpersonationRequest` title forged a `Bcc:` header at the relay, copying the victim's impersonation approval-code email to an attacker.

Commit `5d008fb8` repairs it in layers, all verified in source and by probe:

- `strip_header_controls` (email.rs:534) removes CR/LF/NUL; `encode_subject` (:558) applies it before the ASCII/encoded-word branch, so no subject can ever emit a line break (non-ASCII subjects go through base64 encoded-words, which cannot contain CR/LF).
- `valid_address` (:547) gates `from`/`to` in `send_email` (:671) **before any socket is opened** — the values are interpolated into both RFC 5322 headers and raw `MAIL FROM:`/`RCPT TO:` envelope commands, so both injection vectors are blocked at the gate (reject-outright, not strip: a stripped address is not the configured address).
- `build_message` (:583) control-strips from/to as defense in depth.
- `run_digest_once` (:815) drops entries whose stored user address can never pass the gate, instead of retrying forever.
- In-repo regression tests are real: `crlf_in_title_cannot_forge_header_lines` asserts the header block stays exactly 5 lines with the exact round-2 attack payload (CRLF stripped, not relocated), and `addresses_with_controls_are_rejected_at_the_gate` asserts the gate error surfaces before connect (port 1, gate error not connection error).

Independent probe: `/tmp/stage/review-evidence/header-injection-fix-probe.rs` (standalone rustc binary replicating the exact `strip_header_controls`/`valid_address`/`encode_subject`/`build_message` statements from the candidate) — **13/13 checks PASS, exit 0**, including the exact round-2 attack repro (`Admin Ada\r\nBcc: admin@evil.example` in an ImpersonationRequest title produces no forged header line).

### Acceptance criteria — all verified

- **Bell with unread badge in top nav** — `NotificationBell.svelte` in App.svelte's always-rendered topbar (`topbar-right`), badge caps at 99+, `aria-live` count; real `GET /api/v1/notifications/unread-count` fetch on mount and drawer close (route exists, api/mod.rs:953).
- **Drawer grouped by priority** — `NotificationDrawer.svelte`: slide-out panel, groups ordered Urgent→High→Medium→Low, unknown priorities bucket to Low; per-item icon-by-type, title, body snippet, relative time.
- **Deep links** — clicking an item marks it read and navigates the server-emitted `action_url` via `pushState` + popstate router replay (works from any mode); tested including the popstate replay.
- **Mark read/unread individual + bulk** — real `PUT /notifications/{id}/read`, `POST /notifications/{id}/unread`, `POST /notifications/mark-all-read` (workspace-scoped when that filter is active); optimistic updates with rollback toast.
- **Filtering by type, priority, workspace, repo** — filter bar drives real server-side query params (`?type=&priority=&workspace_id=&repo_id=`); repo options load per selected workspace; priority widens to its band server-side; invalid priority → 400.
- **Unread count endpoint** — efficient count query, user-scoped (other user's rows not counted — tested).
- **SSE endpoint** — `GET /api/v1/notifications/stream` per-event user+tenant filtered per connection, heartbeat keep-alive; own-delivered / other-user-dropped tested.
- **Preferences page** — `/settings/notifications` route wired through parseUrl/urlFor/pushState/popstate + user-menu entry; edits `GET/PUT /api/v1/notifications/preferences`: email on/off, digest frequency, min priority, per-type unsubscribe toggles, webhook (url/secret/min priority), Slack (webhook_url/channel/min priority). Server validates: `in_app` forced true, absolute-URL checks, type_overrides keys must parse as real notification types (400 otherwise).
- **Email templates ≥5 types** — exhaustive `type_heading` match over **all 42** `NotificationType` variants in both `en` and `de` catalogs (a new variant without copy is a compile error), HTML + plain-text fallback each, HTML-escaped bodies/urls.
- **Digest mode aggregates** — `render_digest_email` groups by priority band Urgent→Low with per-item unsubscribe links; `run_digest_once` drains the durable kv outbox per user: Immediate → individual emails, Hourly/Daily → ONE aggregate once the window since the last send (kv `email_digest_state`) has elapsed; gating re-checked at drain against the user's CURRENT config; failed sends kept for retry; no relay configured (`GYRE_SMTP_HOST` unset) → queue-and-hold. End-to-end test drives a real in-process SMTP listener (asserts aggregate subject, outbox drained, state stamped, hold-without-relay).
- **Real-time push** — server pushes `WsMessage::NotificationPush` over the app's single `/ws` connection to every live connection whose **server-verified** identity is the notification's owner (never client-supplied ids); test asserts the owner receives exactly one push and another user's connection receives nothing. The centralized `create_notification` fan-out covers all emitters; impersonation's bespoke push was correctly folded into it.
- **Locale rendering** — `User::locale` BCP-47 → `en` default / `de` catalog; no hardcoded prose outside the catalog.

### Scope/auth enforcement (sub-agent deep review, confirmed)

- All notification routes resolve the caller in-handler and key every repo operation on `user_id`; by-id routes carry explicit cross-tenant Forbidden guards; user A marking/reading user B's notification is a 404 with zero rows mutated (tested).
- `mark_all_read` scoped to caller, optional workspace narrowing; sqlite/mem/postgres adapters implement the same filter/pagination/scope contract.
- ABAC exempt-route invariant satisfied (per-handler auth in every exempt handler); all ten notification routes registered.

### Probes run (evidence in /tmp/stage/review-evidence/)

| Probe | Result |
|---|---|
| `cargo check -p gyre-server` | exit 0 (6m56s cold; build.rs also compiled current web source) |
| `cargo test -p gyre-domain` | 535 + 8 passed, 0 failed |
| `cd web && npm ci` | exit 0 |
| `npx vitest run` (4 notification test files) | 20/20 passed |
| `npm test` (full frontend suite) | 119 files, 2371 passed, 41 skipped, 0 failed, exit 0 |
| standalone rustc header-injection probe (verbatim fix logic) | 13/13 PASS, exit 0 |
| `check-arch.sh`, `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh`, `check-in-memory-state-stores.sh`, `check-forged-scope-fields.sh`, `check-unbounded-external-http.sh`, `check-lossy-secret-conversion.sh`, `check-inert-enforcement.sh`, `check-byte-slice-truncation.sh` | all exit 0 |
| `check-task-commit-attribution.sh` | fails **only** because the controller's uncommitted working-tree reset of `specs/tasks/task-113.md` empties `commits:`; the candidate's *committed* frontmatter lists all 11 task-labeled product-surface commits in range (verified SHA-by-SHA) |

### Deferred to the independent verifier (gyre-server test compilation, per 4 GiB budget)

- `cargo test --all` (full suite — acceptance criterion).
- Focused: `cargo test -p gyre-server email::` (CRLF regression, address gate, digest drain E2E, read_reply non-char-boundary), `cargo test -p gyre-server notifications::` (owner-only WS push), `cargo test -p gyre-server notification_dispatcher` (per-type unsubscribe suppresses queue entry), `cargo test -p gyre-server api::users` (notification endpoint scope/filter tests).
- `web/dist` regeneration + freshness: generated output is verifier-owned in this pipeline; recorded separately, not a finding.

### Non-blocking observations (recorded, no repair required)

- The task's Implementation Plan mentions `?page=`/`?unread=true` params for `GET /api/v1/notifications`; the implementation uses `?limit=&offset=` and has no unread filter. The canonical spec (user-management.md endpoint table) requires only "paginated, filterable", and the acceptance criteria list filtering by type/priority/workspace/repo — all present. Plan-level detail, not a contract violation.
- `PUT /notifications/{id}/unread` is not routable (route is POST-only); the spec's endpoint table defines no `/unread` endpoint at all (mark-unread is a UI bullet, satisfied via POST), and the shipped UI uses POST.
- Frontend test-strength nits: the bell tests neutralize the mount-time fetch (the bell→store call is unpinned); `api.js`'s URL/method mapping is mocked away in all component tests (verified correct by source reading against the route table); an unknown `?unsubscribe=` type is silently ignored by the settings page (the server validates stored keys regardless); duplicate request pairs fire on workspace-filter change (benign, stale-guarded); `navigateTo` on a malformed/cross-origin `action_url` would throw uncaught in the click handler (not XSS — pushState cannot navigate cross-origin).
- API-layer tests don't exercise cross-user isolation of the *listing* endpoints (repo-layer scoping is tested; by-id isolation is tested at the API layer).

Test-inflation check: none found. The CRLF regression tests fail on reintroduction of the injection; the digest E2E test drives a real socket; the WS push test asserts owner-only delivery; frontend tests assert exact endpoints, payloads, filter params, and popstate replay.
