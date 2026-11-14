# Review — task-113 (Implement in-app notification UI & email templates)

Spec: `specs/system/user-management.md` §In-App Notification UI + §Email Notifications.
Commits under review: `8cc35749` (product), `25f0bb2d` (product fix), `5eed7b03` (review-repair) — recorded in the task frontmatter.
Candidate: `8f98d5f4` on `pipeline/task-113/b613a0c4455c4e3696b346c022d0e77e-1` (base `cdd1dd02`).
Verdict: **approved** — the single prior-round finding (SMTP reply byte-slice panic) is fixed and independently probed; no new defects found.

## Round 1 (prior review, candidate `991cc8b5`)

One material finding (durable, carried in the pipeline record):

- **F1 (major): `read_reply()` sliced the SMTP reply at a fixed byte index (`line[..3]`)** — panicking on a valid-UTF-8 reply line whose byte 3 falls inside a multibyte char (e.g. `"üü\r\n"`, 4 bytes after trim_end, passing the length guard). The panic unwound through `send_email` → `run_digest_once` → the `tokio::spawn`'d `email_digest` job loop, permanently killing the §Email Notifications digest drain until restart — the exact availability class behind the repo's mechanically-enforced byte-slice-truncation invariant (task-095); the site evaded the gate only because its regexes require a leading `&` on `&expr[..N]`.

Everything else in the round-1 diff was verified clean (endpoint surface pre-existed from task-112/task-125 and was only consumed, not changed).

## Round 2 — F1 repair verified, full re-review

### F1 repair (commit `5eed7b03`)

`read_reply` now parses the 3-digit code from raw bytes — `std::str::from_utf8(&bytes[..3])` with an `Err` on non-ASCII, `.parse()` with an `Err` on non-numeric — never a char-boundary slice; `bytes[3]` (byte indexing, panic-free after the length guard) drives the continuation/final discrimination. The function is generalized over `AsyncRead + Unpin` so tests drive it with `tokio::io::duplex`. Two regression tests added: `read_reply_non_char_boundary_line_errors_not_panics` (the exact failure line → `Err("malformed SMTP reply")`, drain loop survives) and `read_reply_continuation_and_short_line` (positive control: `250-.../250 ...` continuation parses; 3-byte line errors).

**Independent probe** (`/tmp/stage/review-evidence/06-read-reply-fix-probe/`, standalone rustc replica of the exact committed logic): the round-1 failure line yields `Err(malformed SMTP reply code (non-ASCII): "üü")` with no panic; the old logic still panics (`end byte index 3 is not a char boundary`); `"250 8BITMIME"` → `Ok(250)`; `250-relay.local` continuation → `Ok(250)`; `"250\r\n"` → `Err(short)`. Confirmed the job loop's error path (`jobs.rs` `spawn_job`): handler `Err` is logged and the loop continues — only a panic killed it, and the panic is gone.

### Probes run (all evidence under `/tmp/stage/review-evidence/`)

- `cd web && npm ci` → exit 0; `npm test` → **2328 passed / 41 skipped, 117 files, exit 0** (full suite, includes the four new files).
- `npx vitest run` on the four new test files → **20/20 passed**.
- `/tmp/stage/bin/cargo test -p gyre-domain` → all green (incl. `EmailConfig::type_overrides` serde compat).
- `/tmp/stage/bin/cargo check -p gyre-server` → exit 0; `cargo check -p gyre-server --tests` → exit 0 (the email test module, incl. the in-process SMTP relay e2e, type-checks).
- Mechanical gates on the candidate tree: `check-arch.sh`, `check-byte-slice-truncation.sh`, `check-relative-path-defaults.sh`, `check-inert-enforcement.sh` → all OK.
- Full diff inspection `cdd1dd02..8f98d5f4` (~3000 lines): every touched file reviewed.

### Verified working (acceptance criteria)

- **Bell + unread badge in top nav**: `NotificationBell.svelte` mounted in the App.svelte topbar beside the HSI §1.1 Decisions badge — intentionally a distinct surface (Decisions = unresolved judgment items; bell = unread state across all notifications via `GET /api/v1/notifications/unread-count`). Badge caps at 99+, `aria-live` count. Tests assert badge values (3, 99+) and absence at zero.
- **Drawer slides out, grouped by priority**: `NotificationDrawer.svelte` — fixed right panel with slide-in animation, items grouped Urgent→Low by `priority_level` (fallback band Low). Test asserts the exact group order `['Urgent','High','Medium','Low']` and per-group membership — it fails if grouping breaks.
- **Deep links navigate to the entity**: item click marks read and pushes the server-emitted `action_url` + replays the app's popstate router. Test spies `history.pushState` and counts popstate events.
- **Mark read/unread individually and bulk**: `PUT .../{id}/read` + `POST .../{id}/unread` per item (busy-guarded, optimistic local update + server refresh of the badge), `POST .../mark-all-read` bulk (workspace-scoped when that filter is active). Tests assert each endpoint is called with the right id and that the UI state flips. Repair verified: `api.markNotificationRead` previously hit the *dismiss* endpoint — remapped to the read endpoint, and the three dismiss call-sites (Inbox / WorkspaceHome / CrossWorkspaceHome) now call the real dismiss endpoint.
- **Filtering by type, priority, workspace, repo**: filter bar with all four selects (repo options load per selected workspace, race-safe); tests assert the query params reach `api.notifications` (`{priority}`, `{workspace_id, repo_id}`); the backend `list_for_user` filter plumbing pre-exists from task-125 and is unchanged.
- **Unread count API + SSE real-time push**: endpoints pre-exist (verified at base `cdd1dd02`, no route/ABAC changes in this diff); the new client (`lib/notifications.svelte.js`) consumes `GET /api/v1/notifications/stream` via fetch-streaming (EventSource cannot send the Authorization header), incrementally parsing SSE frames — split-frame-safe, heartbeats skipped, malformed payloads never kill the stream — and bumps the badge live; the drawer reloads on each streamed event and shows a live-connection dot. Store tests drive a real ReadableStream with chunks split mid-frame and assert unread-only counting.
- **Preferences page** at `/settings/notifications`: full router wiring (parseUrl/urlFor/pushState/popstate/title/user-menu entry, `mode = 'notification_settings'`); email on/off, digest frequency, min priority, per-type unsubscribe toggles, webhook (url/secret/min priority), Slack (webhook/channel/min priority); `in_app` forced true server-side (`validate_channel_config`) and client-side; `type_overrides` keys validated as real notification types server-side. Tests cover load/render, save payload shape, per-type flip, webhook/slack structure, and the `?unsubscribe=TYPE` email landing applying the opt-out.
- **Email templates ≥ 5 types**: `email.rs` templates *every* `NotificationType` variant (~41) via an exhaustive `type_heading` match — a new variant without copy is a compile error — with HTML + plain-text fallback, per-type unsubscribe link, `en`/`de` locale catalogs resolved from `User::locale` (unknown tags → English; no hardcoded prose outside the catalog).
- **Digest mode aggregates**: `render_digest_email` produces ONE email grouped by priority with per-item unsubscribe links; the `email_digest` job (registry + main.rs spawn, 60 s) drains the durable kv outbox per user — Immediate sends individually, Hourly/Daily aggregate once per window (kv `email_digest_state`), failed sends keep entries for retry, tightened configs drop already-queued noise at drain (priority filter re-checked against the *current* config), no relay configured = queue-and-hold (nothing sent or dropped). Transport is a real minimal RFC 5321 client (EHLO/MAIL FROM/RCPT TO/DATA, dot-stuffing, base64 multipart/alternative, RFC 2047 subjects, 10 s per-op timeouts) — no `lettre` dependency needed.
- **`npm test` passes** (my run: 2328 passed). **`cargo test --all`** deferred to the independent verifier per the 4 GiB build-budget rule; exact commands recorded in the evidence summary (`cargo test -p gyre-server email::` includes the end-to-end digest drain against an in-process TCP SMTP listener — listener support confirmed in this sandbox's capabilities — and both read_reply regression tests; the full test module type-checks via `cargo check -p gyre-server --tests`).

### Quality checks

- **Test inflation**: none found. The frontend tests assert exact endpoint calls, payload shapes, group order, and navigation side effects — each fails when the corresponding behavior breaks. The SSE store test drives a real ReadableStream with split frames. The backend dispatcher test asserts the outbox is empty for an unsubscribed type and non-empty (with the embedded notification) for a subscribed one.
- **Hexagonal boundary**: `email.rs` lives in gyre-server (infrastructure layer); domain change is the `EmailConfig::type_overrides` field with serde default for stored-row compat. `check-arch.sh` OK.
- **No scope creep**: no routes, ABAC registry entries, migrations, scripts, or unrelated tasks touched; the one-line diffs outside the notification surface are `type_overrides: Default::default()` additions to existing test fixtures (compile-completeness for the new field).
- **Coverage matrix**: rows 19/20 marked implemented with landed evidence; counts consistent (16→14 task-assigned, 9→11 implemented); the email row explicitly records the deferred-to-verifier server tests rather than claiming a local run.
- **Generated `web/dist/`**: intentionally unchanged in this checkpoint — owned by the deterministic verifier, which rebuilds after integration. Pure generated-output observation, non-blocking; the authored frontend source is what was reviewed and tested.

### Deferred to verifier

`cargo test -p gyre-server email::`, `cargo test -p gyre-server notification_dispatcher::`, `cargo test --all`, and `scripts/check-task-commit-attribution.sh` (the attribution gate timed out in this sandbox's history walk; the three frontmatter commits were verified on the branch via git log). None is expected to fail: the test code type-checks, the digest e2e's dependencies (TCP listener) are supported, and the prior round ran the suite with only F1 outstanding.
