# Review — task-113: Implement in-app notification UI & email templates

Candidate: `8999ad164c1f061330316f2c75eb9bf4a59bd49e`
Base: `32c4faefdcd01744f5d2e3015a1a615b2cf83b6e`
Reviewer scope: `git diff 32c4fae..8999ad16` (32 files, ~3045 insertions).
Evidence: `/tmp/stage/review-evidence/review-summary.md` (+ full logs).
This is the first review of this candidate; it incorporates the repairs from
the branch's earlier review/verification rounds (5eed7b03 SMTP reply-code
byte-slice fix, 25f0bb2d unused locale field, c945df27 EmailConfig test
initializer + rustfmt repairs), all re-verified here.

## Verdict

Approved. Both coverage sections (user-management.md §In-App Notification UI,
§Email Notifications) are implemented by real production code, the frontend
test suite passes in full, the server code type-checks including all test
targets, and no structural defect, missing enforcement, unsafe scope handling,
or fake implementation was found. Server-side test *execution* is deferred to
the independent verifier per the 4 GiB build-budget rule (exact commands
recorded below and in review-evidence).

## What was verified

### §In-App Notification UI
- **Bell + badge**: `NotificationBell.svelte` in the topbar (App.svelte),
  beside and deliberately distinct from the HSI §1.1 Decisions badge (unread
  state across all notifications vs unresolved judgment items). Badge caps at
  99+, `aria-live` count, `aria-expanded` on the trigger.
- **Drawer**: `NotificationDrawer.svelte` slide-out grouped by
  `priority_level` Urgent→Low, per-item title/type label/body snippet/relative
  time, mark read/unread individually (`PUT .../read`, `POST .../unread` —
  both verified present server-side with per-handler user+tenant guards) and
  in bulk (`POST .../mark-all-read?workspace_id=`), filter bar
  (type/priority/workspace/repo — repo options loaded per selected
  workspace), live-connection indicator, preferences link.
- **Deep links**: clicking an item marks it read and navigates the
  server-emitted `action_url` by pushState + popstate replay (works from any
  app mode); covered by a test asserting both the push and the replayed
  popstate event.
- **Real-time SSE**: `lib/notifications.svelte.js` consumes
  `GET /api/v1/notifications/stream` with fetch-streaming (EventSource cannot
  send the Authorization header). The server handler filters every event to
  the caller's user_id + tenant_id (+ optional workspace) before it reaches
  the wire. Client parsing is split-frame safe, skips heartbeat comments,
  bumps the badge only for unread notifications — each property pinned by a
  test.
- **Preferences page**: `/settings/notifications` route wired through
  parseUrl/urlFor/pushState/popstate/title/user-menu;
  `NotificationSettings.svelte` edits email (on/off, digest frequency, min
  priority, per-type unsubscribe), webhook (url/secret/min priority), Slack
  (webhook/channel/min priority) via GET/PUT `/notifications/preferences`
  with `in_app` forced. Server-side `validate_channel_config` (extended in
  this diff) rejects `in_app: false`, non-absolute hook URLs, and
  `type_overrides` keys that don't parse as a notification type.
- **Real bug fixed en route**: `api.markNotificationRead` previously hit the
  *dismiss* endpoint; remapped to the read endpoint and the three dismiss
  call-sites (Inbox/WorkspaceHome/CrossWorkspaceHome) moved to the real
  dismiss endpoint.

### §Email Notifications
- **Templates per type**: `gyre-server/src/email.rs`
  `render_notification_email` — HTML + plain-text fallback for every one of
  the 41 `NotificationType` variants via an exhaustive `type_heading` match
  (a new variant without copy is a compile error), HTML-escaped content,
  RFC 2047 subject encoding for non-ASCII.
- **Digest mode**: `render_digest_email` aggregates all pending entries into
  one email grouped by priority band with per-item unsubscribe links; the
  `email_digest` background job (registered in the job registry, spawned in
  main.rs, 60 s, awaited serially by the job loop) drains the durable kv
  outbox per user: Immediate → individual emails; Hourly/Daily → one
  aggregate once the window since the last send (kv `email_digest_state`)
  has elapsed. Failed sends keep their entries (durable retry); no relay
  configured → queue-and-hold, nothing silently dropped.
- **Priority filter**: enforced at queue time (dispatcher) and re-checked at
  drain time against the user's *current* config — tightened settings drop
  already-queued noise.
- **Unsubscribe per type**: every email links to
  `{base_url}/settings/notifications?unsubscribe={type}`; the authenticated
  preferences page applies and persists the per-type opt-out
  (`EmailConfig::type_overrides`), which the dispatcher honors at queue time
  and the drain re-checks. Round-trip covered by frontend tests
  (payload shape, landing application) and a dispatcher test (queue-entry
  suppression).
- **Locale**: rendering driven by `User::locale` through a catalog (en
  default, de supported); no hardcoded prose outside the catalog. Serde
  back-compat for stored rows (`type_overrides` absent-by-default) verified by
  gyre-domain tests executed here (510 passed).
- **Transport**: minimal real RFC 5321 client (EHLO/MAIL FROM/RCPT TO/DATA,
  dot-stuffing, CRLF, base64 multipart/alternative, 10 s per-op timeouts) —
  not a structural stub; documented in docs/server-config.md
  (GYRE_SMTP_HOST/PORT/FROM).

### Acceptance criteria status
All eleven criteria implemented; `npm test` executed here in full
(117 files / 2328 passed). `cargo test --all` belongs to the verifier.

## Probes executed

- `npm ci` (exit 0); focused new test files (20 tests, exit 0); full
  `npm test` (exit 0, 2328 passed | 41 skipped).
- `cargo test -p gyre-domain` (exit 0, 518 tests).
- `cargo check -p gyre-server --tests` (exit 0 — confirms the prior
  verification round's "EmailConfig initializer" compile failure is repaired
  across all seven test-fixture files).
- `cargo fmt --check` on the three touched Rust packages (exit 0).
- `scripts/check-arch.sh`, `check-abac-route-registry.sh`,
  `check-byte-slice-truncation.sh`, `check-unbounded-external-http.sh`,
  `check-lossy-secret-conversion.sh` — all exit 0.
- Taxonomy parity probe: the 41 frontend TYPES entries match the 41 Rust
  `NotificationType` variants exactly; all 55 new en.json i18n keys present.

## Deferred to the independent verifier (required probes)

1. `cargo test -p gyre-server email::` — includes the end-to-end digest drain
   against an in-process fake SMTP listener and the non-char-boundary
   SMTP-reply regression test.
2. `cargo test -p gyre-server notification_dispatcher` — includes
   `email_queue_respects_per_type_unsubscribe`.
3. `cargo test --all` (task acceptance criterion).
4. web/dist regeneration + full check suite (generated output is the
   verifier's asset; the committed candidate touches no web/dist files).

## Non-blocking observations

- `scripts/check-dead-components.sh` reports 4 violations
  (CanvasProbeStub, PipelineOverview, `__accordion_*` helpers) — all
  pre-existing at the base commit, untouched by this diff.
- NotificationDrawer's workspace-filter change triggers both an onchange
  handler and the filter-tracking `$effect`, duplicating load()/loadRepos()
  calls; harmless (stale-response generation guard) but redundant.
- The SMTP client omits Date/Message-ID headers; it is a documented minimal
  client for a deployment relay, and neither header is specced.
