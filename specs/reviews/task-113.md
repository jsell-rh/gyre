# Review — task-113 (Implement in-app notification UI & email templates)

Spec: `user-management.md` §In-App Notification UI + §Email Notifications.
Candidate under review: `29f4cb82c9991d79e707e0e2d9aed10f5b0d2bdb` (base `3f7871b6`).
Commits: `8cc35749` (implementation) plus review/verification repairs `25f0bb2d`,
`5eed7b03`, `c945df27`, `5b8d0974`, `5ff46667` (all in the task frontmatter `commits:`).
Verdict: **approved**.

## Scope note

The backend notification endpoints (spec-path listing with `?type=&priority=&workspace_id=&repo_id=`
filters, `unread-count`, `mark-all-read`, `read`/`unread`, dismiss/resolve, and the SSE
`/notifications/stream` with per-handler user+tenant filtering and heartbeats) pre-exist this
task on the merged task-112/task-125 base — verified present at the base commit. Task-113's
contract is the UI surface, the preferences page, and the email template/digest/transport
layer; that is what the diff delivers.

## Probes run (evidence in `/tmp/stage/review-evidence/`)

- `npm ci` (web) → exit 0.
- `npx vitest run` (full frontend suite) → **117 files, 2328 passed, 41 skipped, 0 failed**
  (includes the 4 new files: NotificationBell, NotificationDrawer, NotificationSettings,
  notifications-store — 20 tests).
- `cargo check -p gyre-server` → clean (8m57s).
- `cargo check -p gyre-server --tests` → clean (type-checks the `#[cfg(test)]` email/
  dispatcher/users test code; 4m01s).
- `cargo test -p gyre-domain` → **535 passed, 0 failed**.
- `scripts/check-arch.sh`, `scripts/check-byte-slice-truncation.sh`,
  `scripts/check-unbounded-external-http.sh` → all exit 0.
- Mechanically diffed the 41 `NotificationType` variants against the frontend `TYPES` /
  `TYPE_OPTIONS` lists — exact match, no missing/extra entries (the `?unsubscribe=TYPE`
  landing and filter options cover the whole taxonomy).
- `dot_stuff("line one\r\n.hidden\r\n")` hand-traced against the join-based implementation:
  split → `["line one", ".hidden", ""]` → `["line one", "..hidden", ""]` → join →
  `"line one\r\n..hidden\r\n"` — the durable verification failure from the prior round
  (attempt 72fbcc80, doubled trailing CRLF) is genuinely repaired by `5ff46667`, and the
  gained no-trailing-CRLF assertion pins the contract.

Per the review runtime constraints (4 GiB budget; large gyre-server test compilation belongs
to the independent verifier), the gyre-server test binary was not executed here. The verifier
must run:

- `cargo test -p gyre-server --lib email::` (templates, locale, digest grouping, subject
  encoding/dot-stuffing, gating, outbox parsing, non-char-boundary SMTP reply, and the
  end-to-end digest drain against an in-process fake SMTP listener — needs a loopback
  TCP listener, which this sandbox supports).
- `cargo test -p gyre-server --lib notification_dispatcher::` (per-type unsubscribe
  suppresses the queue entry; embedded notification in the queued JSON).
- `cargo test -p gyre-server --lib api::users` (notification endpoint handlers incl. SSE
  own-notifications-only test).
- Full `cargo test --all` per the acceptance criterion.

## Verified working (no findings)

- **Bell + badge**: `NotificationBell.svelte` in the App topbar beside the Decisions badge
  (deliberately distinct surface — Decisions counts unresolved judgment items; the bell
  counts unread state). Count from `GET /notifications/unread-count`, 99+ cap, `aria-live`
  announcement, `aria-expanded` on the trigger. Tests cover badge rendering and drawer open.
- **Drawer**: slide-out panel grouped Urgent→Low by `priority_level`; per-item title/type
  label/body snippet/relative time; per-item mark read/unread hitting the real
  `PUT .../read` / `POST .../unread` endpoints; bulk `mark-all-read` (workspace-scoped when
  that filter is active); stale-response guard (`loadGen`) against filter races; filter bar
  (type/priority/workspace/repo — repo options load per selected workspace); link to
  preferences. Tests assert grouping order, endpoint calls, param pass-through
  (`{priority:'Urgent'}`, `{workspace_id, repo_id}`), and the empty state.
- **Deep links**: click marks read and pushes the server-emitted `action_url`, then replays
  the popstate router — works from any mode. Test asserts the pushState target and the
  popstate replay. `parseUrl` gains `/settings/notifications` ahead of the workspace routes —
  no shadowing (workspaces live under `/workspaces/...`).
- **SSE client**: `notifications.svelte.js` uses fetch + ReadableStream (EventSource cannot
  send the Authorization header — correct call), incremental frame parsing that survives
  chunk splits, heartbeat comments skipped, unread-only badge bumps, failed stream degrades
  to closed status without throwing. Tests feed real chunked streams including a frame split
  mid-JSON.
- **Preferences page**: `/settings/notifications` mode wired through parseUrl/urlFor/
  pushState/popstate/title/user-menu. Edits `GET/PUT /notifications/preferences`: email
  on/off, digest frequency, min priority, per-type unsubscribe toggles, webhook
  (url/secret/min priority), Slack (webhook_url/channel/min priority). Save payload filters
  `type_overrides` to true-only values and forces `in_app: true`. The `?unsubscribe=TYPE`
  landing (the target of every email's unsubscribe link) auto-applies and persists the
  opt-out, then shows a confirmation banner — tested.
- **Real bug fixed en route**: `api.markNotificationRead` previously hit the dismiss
  endpoint; remapped to the read endpoint and the three dismiss call-sites
  (Inbox/WorkspaceHome/CrossWorkspaceHome) moved to `dismissNotification`.
- **Email templates**: exhaustive `type_heading` match over all 41 `NotificationType`
  variants in both `en` and `de` catalogs (a new variant without copy is a compile error);
  HTML body + plain-text fallback via `render_notification_email`; HTML-escaped throughout;
  per-type unsubscribe link; RFC 2047 subject encoding for non-ASCII.
- **Digest mode**: `render_digest_email` aggregates into one email grouped by priority with
  per-item unsubscribe links; `run_digest_once` drains the durable kv outbox per user —
  Immediate sends individually, Hourly/Daily send one aggregate only once the window since
  the last send (kv `email_digest_state`) has elapsed; failed sends keep entries for retry;
  gating is re-checked at drain against the user's CURRENT config (enabled, digest,
  min_priority, per-type unsubscribe) so tightened configs drop already-queued noise.
  The e2e test runs a real in-process SMTP listener and a real RFC 5321 client session.
- **Transport**: minimal real SMTP client (EHLO/MAIL FROM/RCPT TO/DATA with dot-stuffing,
  base64-wrapped multipart/alternative), every socket op latency-bounded (10 s
  `tokio::time::timeout`), reply codes parsed from raw bytes without char-boundary slices
  (regression-tested against a multibyte reply line). Unconfigured relay = queue-and-hold
  (nothing sent or dropped) — documented in `docs/server-config.md` with the three env vars.
- **Domain/storage**: `EmailConfig::type_overrides` (`true` = unsubscribed) with
  `serde(default, skip_serializing_if empty)` so pre-task-113 rows deserialize unchanged;
  custom `NotificationChannels` Deserialize keeps the flat legacy shape readable with empty
  overrides; PUT validates override keys against `NotificationType::parse`; channels persist
  to both `user_channel_prefs` and the user profile (single effective config). Dispatcher
  honors the override at queue time (tested), drain re-checks it.
- **Wiring**: `email_digest` job registered in the registry and spawned in `main.rs` (60 s);
  `email.rs` re-exports used by nothing outside the crate; hexagonal boundaries intact
  (`check-arch.sh` clean); all mechanical checks listed above exit 0.
- Frontend i18n keys for bell/drawer/settings all present in `en.json`; auth token handling
  in the store matches `api.js` (`gyre_auth_token` with dev-token fallback).

## Non-findings (observations, not blocking)

- `web/dist/` is not rebuilt in this candidate — owned by the deterministic verifier, which
  rebuilds and runs checks remotely. Authored frontend source and tests reviewed here.
- The working tree carries an executor-restored assigned task document
  (`specs/tasks/task-113.md` at `progress: not-started`, `commits: []`); the committed
  candidate carries `ready-for-review` with all six commits attributed. Expected per the
  checkpoint process, not a code change.
- SMTP client has no STARTTLS — a plaintext relay-hop client on port 25 pointed at a
  deployment mail hub, matching the documented `GYRE_SMTP_HOST` contract. The spec does not
  require TLS; noting for future hardening only.
- The per-type unsubscribe link lands on the authenticated preferences page (which then
  persists the opt-out) rather than a tokenized one-click unsubscribe. The task plan
  specifies exactly this landing behavior; spec text ("Unsubscribe link per notification
  type") is satisfied.
