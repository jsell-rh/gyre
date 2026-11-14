# Review — task-113 (Implement in-app notification UI & email templates)

Spec: `specs/system/user-management.md` §In-App Notification UI + §Email Notifications.
Candidate: `5d24fc695e10981be582075bd500204bf015d6cf` (base `4b9220bca6ccaf8f3a250adc41c40f6bba6727f1`).
Product commits under review: `8cc35749` (implementation), `25f0bb2d`, `5eed7b03`, `c945df27`, `5b8d0974` (repair rounds); all five recorded in the task frontmatter (verified manually — the remaining task-113-labeled commits are `specs/tasks/task-113.md`-only).
Verdict: **approved**.

## What was reviewed

The diff base→candidate (32 files, +3045/−17): `crates/gyre-server/src/email.rs` (new, 1290 lines incl. tests), notification_dispatcher per-type-unsubscribe gating + embedded-notification outbox entries, `EmailConfig::type_overrides` (gyre-domain + serde compat + PUT-validation), the `email_digest` job (registry + main.rs spawn), `web/src/components/{NotificationBell,NotificationDrawer,NotificationSettings}.svelte`, `web/src/lib/notifications.svelte.js` (SSE store), App.svelte `notification_settings` mode wiring, `api.js` spec-path notification client (incl. the dismiss→read remap repair and the three dismiss call-site fixes), `en.json` locale keys, coverage rows 19–20, and four new frontend test files (20 tests).

## Probes run (evidence in /tmp/stage/review-evidence/)

- `cd web && npm ci` → exit 0.
- `cd web && npm test` → **exit 0: 117 files, 2328 passed / 41 skipped** — the full acceptance suite, including the 20 new notification tests.
- `cd web && ./node_modules/.bin/vitest run` on the four new test files → **exit 0: 4 files, 20/20 passed** (`notification-tests.log`). (A first attempt from the repo root pulled an unpinned npx vitest without the Svelte plugin and failed to parse `.svelte` — probe error, not a code defect; rerun correctly from `web/`.)
- **Mutation probe** (`mutation-drawer-order.log`): temporarily reversed `PRIORITIES` in `NotificationDrawer.svelte` → `groups notifications by priority, Urgent first` **failed (1/8)**, confirming the grouping test is not self-confirming. File restored (`git checkout --`), tree clean apart from the pre-existing stage-setup modification of `specs/tasks/task-113.md`.
- `/tmp/stage/bin/cargo check -p gyre-server` → exit 0 (7m53s cold).
- `/tmp/stage/bin/cargo check -p gyre-server --tests` → exit 0 — the email/dispatcher test code type-checks.
- `/tmp/stage/bin/cargo test -p gyre-domain` → **534 + 8 + 0 passed** (covers the `EmailConfig::type_overrides` serde compat and the priority-band mapping the fixed email fixtures rely on).
- Mechanical gates: `check-arch.sh`, `check-abac-route-registry.sh`, `check-byte-slice-truncation.sh`, `check-unbounded-external-http.sh`, `check-mem-port-contracts.sh`, `check-scope-literal-defaults.sh` → all exit 0. (`check-task-commit-attribution.sh` needs a full-history walk and timed out in-sandbox; attribution verified manually as above.)

## Verified against the task contract

- **Bell + badge**: `NotificationBell.svelte` in the topbar beside the HSI §1.1 Decisions badge (intentionally distinct surface — unread state vs. unresolved judgment items); count from `GET /notifications/unread-count`, 99+ cap, `aria-live`. Tests cover badge render and drawer open.
- **Drawer**: slide-out panel grouped Urgent→Low by `priority_level`, per-item title/type/snippet/relative time, read/unread toggle (`PUT .../read`, `POST .../unread`), bulk `POST .../mark-all-read`, live-connection dot, preferences link. Stale-response guard on filter races; repo options load per selected workspace.
- **Deep links**: item click marks read and navigates the server-emitted `action_url` by pushState + popstate replay; App.svelte's popstate fallback re-parses the URL (empty-state branch), so navigation works from any mode. Test asserts the push and the popstate replay.
- **Filtering**: type/priority/workspace/repo drive the task-125 server filters (`NotificationParams` has `#[serde(rename="type")]`, `priority` band-widening, `repo_id`); tests assert param pass-through incl. the repo-race.
- **Unread-count / SSE / mark-all-read endpoints**: pre-existing from task-112/125 and present in this tree with per-handler auth (self-scope + cross-tenant Forbidden guards; SSE events filtered per caller user+tenant before the wire). The SSE client uses fetch-streaming with incremental split-frame-safe parsing (EventSource cannot send the Authorization header) — store tests cover frame parsing, unread-only counting, auth header, failed-stream fallback.
- **Preferences page**: `/settings/notifications` mode wired through parseUrl/urlFor/pushState/popstate/title/user-menu; edits `GET/PUT /notifications/preferences` (email on/off, digest frequency, min priority, per-type unsubscribe, webhook url/secret/min-priority, slack webhook/channel/min-priority); `in_app` forced true; `?unsubscribe={type}` email-link landing persists the opt-out and shows a confirmation banner — tested end-to-end at the component level. Server side validates `type_overrides` keys against the real notification-type taxonomy.
- **Email templates**: exhaustive `type_heading` match over all 41 `NotificationType` variants (a new variant without copy fails to compile), HTML + text fallback, per-type unsubscribe link, locale catalog (`en` default, `de`) resolved from `User::locale` with no hardcoded prose outside the catalog.
- **Digest mode**: `render_digest_email` aggregates into ONE email grouped by priority with per-item unsubscribe links; `run_digest_once` drains the durable kv outbox per user — Immediate sends individually, Hourly/Daily send once per window (kv `email_digest_state`), failed sends keep entries for retry, no-relay = queue-and-hold. Priority filter enforced at queue time AND re-checked at drain against the user's current config (raised thresholds / per-type unsubscribes drop already-queued entries). Transport is a real minimal RFC 5321 client (EHLO/MAIL FROM/RCPT TO/DATA, dot-stuffing, CRLF, base64 multipart/alternative, RFC 2047 subjects) with 10 s per-op timeouts; `read_reply` parses the code from raw bytes (no char-boundary slice — the `5eed7b03` repair, with a dedicated non-panic regression test).
- **Job wiring**: `email_digest` registered in the job registry and spawned in main.rs after `start_job_registry`; env vars documented in `docs/server-config.md`.
- **Prior verification failure repaired**: the durable finding from candidate `8999ad16` (two failing email tests under `cargo test --all`) was fixed by `5b8d0974` as fixture-only repairs. Verified independently: the digest fixture now includes `MetaSpecDrift` (Medium per `for_notification_type`) alongside MergeQueuePaused/AgentFailed (High) and SuggestedSpecLink (Low), so the High<Medium<Low ordering assertions are satisfiable; the subject-encoding fixture is now genuinely non-ASCII ("Warteschlange pausiert – Übersicht") so `encode_subject` takes the RFC 2047 branch with a base64 decode round-trip assertion. Both repairs strengthen, not weaken, the tests.

## Deferred to the independent verifier (build-budget restriction, not a code defect)

The sandbox cgroup is exactly 4 GiB; per the review runtime rules the gyre-server test-binary compilation belongs in the verifier. Recorded checks the verifier must execute on the integrated tree:

- `cargo test -p gyre-server --lib email::` — 10 tests incl. the two repaired fixtures and the end-to-end digest drain against an in-process fake SMTP relay (TCP listener probe: supported).
- `cargo test -p gyre-server --lib notification_dispatcher` — incl. `email_queue_respects_per_type_unsubscribe`.
- `cargo test --all` (acceptance criterion; prior round showed 2218 passed / the 2 now-fixed fixture failures only).
- `cd web && npm test` (already green in this review: 2328 passed).

## Non-blocking observations (recorded separately)

- **Generated output**: `web/dist/` was not regenerated on the branch — owned by the deterministic verifier per the checkpoint contract; not reviewed, not blocking. Incidental evidence: `cargo check -p gyre-server` triggers `build.rs`, which auto-builds the Svelte UI — the resulting dist regeneration **succeeded** and the fresh bundle contains the notification surface (`notification-bell`/`notification-badge` testids, `settings/notifications` route, `unsubscribe=` links), confirming the authored web source builds. The regenerated dist was then restored to the committed state (`git checkout -- web/dist && git clean -fd web/dist`) so the verifier owns the tested-tree assets.
- `run_digest_once` *drops* (rather than holds) outbox entries the user has since unsubscribed from / deprioritized below threshold. Intentional and documented in-code (tightened configs drop already-queued noise); defensible reading of "priority filter prevents low-priority email noise".
- The bell's SSE stream has no automatic reconnect (status falls back to `closed`; the count refreshes on drawer open/close). Minor UX resilience, not a spec requirement.

No fake implementations, missing enforcement, unsafe scope handling, or test inflation found. All acceptance criteria are met by real production code with meaningful tests.
