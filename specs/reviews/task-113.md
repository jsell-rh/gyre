# Review: task-113 — In-App Notification UI & Email Templates

**Candidate:** `d3d5145b31a418493375352eb3de59c657896ab2` (branch `pipeline/task-113/…-1`, base `9fb371ac1b527256496cc05c740ea5bf85ae435c` = main's task-111 head)
**Round:** merge-conflict resolution re-review (fresh, post-resolution; no prior approval carried forward)
**Verdict:** **APPROVED** — independent evidence supports the task contract.

## Scope reviewed

Diff `9fb371ac..d3d5145b` (58 commits; product delta = task-113 UI/email work + mechanical merge of
main's task-111 sessions). Key surfaces:

- `web/src/components/NotificationBell.svelte`, `NotificationDrawer.svelte`, `NotificationSettings.svelte`
- `web/src/lib/notifications.svelte.js`, `api.js` additions, `App.svelte` wiring (`notification_settings`
  mode, route parse/urlFor/pushState/popstate, user-menu entry, bell in top nav beside the HSI Decisions badge)
- `crates/gyre-server/src/email.rs` (new, 1462 lines: per-type templates, locale catalog, digest renderer,
  RFC 5321 SMTP client, digest drain)
- `notification_dispatcher.rs` (per-type unsubscribe gate at queue time; outbox embeds full notification),
  `notifications.rs` (WS `NotificationPush` fan-out via `BroadcastingNotificationRepo`, verified-identity targeting),
  `impersonation.rs` (its private push folded into the centralized fan-out), `jobs.rs`/`main.rs`
  (`email_digest` job), `gyre-domain` `EmailConfig::type_overrides` (serde-default compat), `api/users.rs`
  (type_overrides key validation), docs, coverage matrix rows 19/20.

## Findings — none blocking

Acceptance criteria all satisfied by real code: bell + unread badge (99+ cap, aria-live) in the top nav;
slide-out drawer grouped Urgent→Low with per-item read/unread (`PUT …/read`, `POST …/unread`), bulk
mark-all-read, type/priority/workspace/repo filter bar; deep links replay popstate routing against the
server-emitted `action_url`; preferences page at `/settings/notifications` (email/digest/min-priority,
per-type unsubscribe, webhook, Slack) persisted via `GET/PUT /api/v1/notifications/preferences`; unread-count
endpoint; specced SSE `/notifications/stream` unchanged (per-event user+tenant+workspace filtered, heartbeat,
Lagged-keeps-streaming) with the SPA's realtime badge riding the single authenticated `/ws` connection
(`NotificationPush` targeted by server-verified user identity recorded at WS auth time — never client-supplied);
email templates for **all 41** `NotificationType` variants (exhaustive match = compile-time enforcement, well
past the "≥5 types" bar) in HTML + text fallback with per-type unsubscribe links and `User::locale` rendering;
digest aggregation with Immediate/Hourly/Daily windows against durable kv state; priority filter enforced at
queue time AND re-checked at drain against current prefs; real RFC 5321 transport with dot-stuffing, RFC 2047
subjects, 10 s per-op timeouts, CRLF header/command-injection gates, and queue-and-hold when no relay is
configured. Prior review-round hardening (CRLF injection, non-char-boundary reply slicing) is present with
killing regression tests.

Merge resolution is sound: the only both-sides-changed code file was `sqlite/user_profile.rs` (disjoint
hunks — full adapters suite passes); `impersonation.rs` deduplicated its push into the centralized fan-out;
`api.markNotificationRead` endpoint-mapping bug fixed (dismiss call-sites remapped to `dismissNotification`).

## Independent probes (evidence: `/tmp/stage/review-evidence/`)

| Probe | Result |
|---|---|
| `npm ci` (web) | exit 0 |
| Focused vitest: NotificationBell/Drawer/Settings + notifications-store (20 tests) | 20 passed, exit 0 |
| **Mutation probe**: removed the `!notif.read` guard on the WS badge bump in `notifications.svelte.js` | `notifications-store.test.js` FAILS (unread-only count) — test kills the regression; file restored, tree verified clean |
| Full `npm test` (120 files) | 2384 passed, 41 skipped, exit 0 |
| `SKIP_WEB_BUILD=1 cargo check -p gyre-server` | exit 0 (6 m 39 s) — merged server type-checks |
| `cargo test -p gyre-domain` | 535 + 8 passed (incl. `EmailConfig::type_overrides` serde compat) |
| `cargo test -p gyre-adapters --lib` (the one both-sides-changed file) | 537 passed, 0 failed |
| `scripts/check-arch.sh`, `check-unbounded-external-http.sh`, `check-lossy-secret-conversion.sh` | all pass |
| Frontend type-option list vs server `NotificationType` enum | exact match, 41/41 — no drift |
| Task commit attribution | all 13 `commits:` SHAs are ancestors of the candidate |
| i18n keys used by new components | all present in `en.json` |

## Verifier-required commands (gyre-server test compilation deferred per the 4 GiB build wrapper)

```
cargo test -p gyre-server --lib email::
cargo test -p gyre-server notifications
cargo test -p gyre-server --test spec_enforcement
cargo test --all
```
plus web e2e (`npx playwright test` against a release server build). The email.rs suite includes an
end-to-end digest drain against an in-process fake SMTP relay, the CRLF-injection regression tests, and the
non-char-boundary reply test; the dispatcher suite covers per-type unsubscribe queue suppression.

## Non-blocking observations

- The bell's WS push increments `unread` optimistically; a concurrent `refreshUnreadCount` reconcile happens
  on drawer open/close and on read actions, so drift is bounded — cosmetic at worst.
- Generated `web/dist/` not assessed (verifier-owned; the diff contains no dist changes — the verifier
  rebuilds from source after integration).
