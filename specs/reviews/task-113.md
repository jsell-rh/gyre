# Review — task-113 (user-management §In-App Notification UI + §Email Notifications)

Task: notification bell + drawer UI, preferences page, email templates/digest/unsubscribe/locale, SSE-driven badge.
Candidate under review: `991cc8b5f117a02206e1830dd805cc1282358919` (implementation `8cc35749`, repair `25f0bb2d`, attribution/process `73ef50a4`/`e5b8ce55`/`3028dcc1`/`991cc8b5`), base `bfc36c9c`.
Verdict: **needs revision** — one code finding (F1). Everything else verified working.

## What was verified (independently, this review)

Probes (evidence in `/tmp/stage/review-evidence/`, index in `00-probe-index.md`):

- `web && npm ci` then focused vitest on the four new suites: **4 files / 20 tests passed** (`01-web-focused.log`).
- Full `npm test`: **117 files, 2328 passed, 41 skipped** (`02-web-full.log`) — acceptance criterion "`npm test` passes" holds, and the pre-existing suites absorbed the `api.markNotificationRead` remap (dismiss call-sites moved to `dismissNotification`) without regressions.
- `cargo test -p gyre-domain`: **510 + 8 passed** (`03-domain-tests.log`) — includes the `EmailConfig::type_overrides` serde-compat tests (flat pre-task-112 shape still deserializes).
- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests`: **clean, no warnings** (`04`/`05`). Server test-binary compilation deferred to the verifier per the 4 GiB budget; required commands recorded in the evidence index.
- Mechanical gates on the tree: check-arch, check-inert-enforcement, check-fabricated-scope-defaults, check-abac-route-registry, check-mem-port-contracts, check-unbounded-external-http, check-lossy-secret-conversion, check-scope-literal-defaults, check-forwarded-header-trust, check-byte-slice-truncation — **all exit 0** (`06-invariants.log`). Note check-byte-slice-truncation passes only because its regexes require a leading `&`; see F1.

Verified working, by reading the diff against the base and cross-checking consumers:

- **Bell + badge** (`NotificationBell.svelte`): unread count from `GET /api/v1/notifications/unread-count`, 99+ cap, `aria-live`, opens the drawer; distinct from the HSI Decisions badge as documented.
- **Drawer** (`NotificationDrawer.svelte`): groups by the server-emitted `priority_level` (verified the field exists in `NotificationResponse` and the `?priority=` filter maps names to HSI rank bands server-side), per-item read/unread via `PUT .../read` + `POST .../unread`, bulk `POST .../mark-all-read` (workspace-scoped when filtered), type/priority/workspace/repo filter bar (repo options load per selected workspace, stale-response guarded by loadGen), deep links push the server `action_url` and replay the popstate router, preferences link via context. The `api.markNotificationRead` → dismiss-endpoint mis-mapping repair is real and its three call-sites (Inbox/WorkspaceHome/CrossWorkspaceHome) correctly moved to `dismissNotification`.
- **SSE client** (`lib/notifications.svelte.js`): fetch-streaming with Authorization header (EventSource can't send it — correct choice), incremental frame parsing that survives split frames and heartbeats (test proves a frame split mid-JSON), unread-only badge bumps. Server side (`notification_stream`, pre-existing task-125) emits `event: notification` with the `NotificationResponse` JSON filtered to the caller's user+tenant — the client parser's expectations match.
- **Preferences page** (`NotificationSettings.svelte`, route `/settings/notifications` wired through parseUrl/urlFor/pushState/popstate/title/user-menu): loads via `GET /notifications/preferences` (verified handler returns `{preferences, channels}`), saves with `in_app` forced, per-type unsubscribe toggles persisted as `EmailConfig::type_overrides` (only `true` entries sent), webhook/Slack structure; `?unsubscribe={type}` landing applies the opt-out and shows the banner.
- **Email templates** (`email.rs`): exhaustive `type_heading` match over all 41 `NotificationType` variants × en/de — a new variant without copy is a compile error; HTML + text fallback; `html_escape` on all interpolated user content; per-type unsubscribe URL; locale from `User::locale` with en fallback (probe: de subject/catalog strings asserted).
- **Digest mode**: `email_digest` job registered (jobs.rs) and spawned (main.rs, after `start_job_registry`, so the 60 s interval is picked up); `run_digest_once` groups the durable outbox per user, re-checks the user's CURRENT gating (enabled/digest≠Off/min_priority/per-type unsubscribe) before sending, Immediate → individual sends, Hourly/Daily → one aggregate once the window since the kv `email_digest_state` stamp elapsed; failed sends keep entries; unparseable entries and gone-user entries are dropped with warnings; no relay configured → queue-and-hold (nothing sent or dropped).
- **Unsubscribe enforcement chain is closed end-to-end**: email link → preferences page → `type_overrides` (validated at PUT: keys must `NotificationType::parse`, in `validate_channel_config`, which both `PUT /notifications/preferences` and `PUT /users/me` call) → dispatcher drops the queue entry (`notification_dispatcher.rs`, with a real test) → drain re-checks.
- **SMTP client**: minimal but real RFC 5321 (EHLO/MAIL FROM/RCPT TO/DATA/QUIT, multi-line reply continuation, dot-stuffing, base64 multipart/alternative, RFC 2047 subjects), every socket op latency-bounded (10 s) — consistent with the unbounded-external-HTTP invariant's spirit; env config documented in `docs/server-config.md`.

## Findings

### F1 (blocking, code): fixed byte-index string slice panics the digest job

`crates/gyre-server/src/email.rs`, `read_reply`, line 592:

```rust
let line = line.trim_end();
if line.len() < 4 { anyhow::bail!(...); }
let code: u16 = line[..3].parse()?;
```

`read_line` yields valid UTF-8, but a relay reply line whose byte 3 falls inside a multi-byte character (e.g. `"üü\r\n"` — 4 bytes after trim, so the length guard passes) makes `line[..3]` panic. Demonstrated with a standalone probe reproducing the exact logic (`07-byte-slice-panic-probe/`): "end byte index 3 is not a char boundary". The panic unwinds through `send_email` → `run_digest_once` → the `tokio::spawn`ed `email_digest` loop in `jobs::spawn_job`, permanently killing email/digest delivery until restart — the same availability class that produced the repo's mechanically-enforced invariant "Never slice strings at fixed byte indexes (`&s[..N]`) — panics on non-char-boundary UTF-8" (task-095 process revision). It evades `check-byte-slice-truncation.sh` only because that gate's regexes require a leading `&` (`&expr[..N]`), while this site is `line[..3].parse()` — the gate should also be widened in a process round, but the code fix is the blocking part. One-line fix: parse the code from bytes (`std::str::from_utf8` on `line.as_bytes().get(..3)`, bailing on short/non-ASCII), or `line.get(..3)` with a bail on `None`.

## Non-blocking observations (recorded, no action required for this task)

- `?unsubscribe={type}` applies the opt-out as a side effect of loading the authenticated preferences page (GET-triggered PUT). It requires the victim's own Bearer token, and the worst effect is unsubscribing oneself from a notification type — the standard email-unsubscribe UX. If a confirmation step is ever wanted, that's a product decision, not a defect.
- `GET /notifications/preferences` returns the stored webhook secret to its owner — pre-existing task-112 surface, unchanged here.
- Working-tree note: `specs/tasks/task-113.md` is dirty at checkout because the executor restored the assigned contract (durable finding 39fe190a); the candidate commit itself carries the attribution and Shipped notes. Not a candidate defect.
- Generated `web/dist/` was not rebuilt by the agent — owned by the deterministic verifier; no source defect implied.

## Verifier must run (server test binary, deferred from this sandbox)

- `cargo test -p gyre-server --lib email::`
- `cargo test -p gyre-server --lib notification_dispatcher::tests::email_queue_respects_per_type_unsubscribe`
- `cargo test --all`; full `scripts/check-*.sh` matrix.
