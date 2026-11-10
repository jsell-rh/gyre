# Review — task-112 (Implement notification delivery channels & routing)

Spec: `specs/system/user-management.md` §Delivery Channels, §Who Gets Notified,
§Notification Routing for Agent Escalations.
Candidate: `1d5346f58` (base `cd752300c`, merge round `ee154fffb` + process/style
commits `8ac27689c`, `606ef649f`, `1d5346f58`).
Verdict: **approved** — all three coverage sections are implemented with real
production code; every acceptance criterion verified with independent evidence.

## Probes run (this sandbox, candidate HEAD)

- `cargo test -p gyre-server --lib` → 1622 passed / 0 failed (incl.
  notification_dispatcher 16, api::users 22, notifications, spawn).
- `cargo test -p gyre-adapters --lib` → 410 passed / 0 failed / 6 ignored
  (incl. migration 000058 backfill + 000060 channel-shape convergence).
- `cargo test -p gyre-domain --lib` → 445 passed; `cargo test -p gyre-common --lib` → 120 passed.
- `cargo fmt --check` clean; 20 runnable mechanical gates all PASS
  (arch, migration-versions, migration-sql-portability, abac-route-registry,
  abac-exempt-handlers, mcp-write-tools, unbounded-external-http,
  fabricated-scope-defaults, scope-literal-defaults, in-memory-state-stores,
  inert-enforcement, forged-scope-fields, lossy-secret-conversion,
  forwarded-header-trust, fail-open-ref-resolution, byte-slice-truncation,
  mem-port-contracts, dead-message-kinds, relative-path-defaults,
  task-commit-attribution).
- **Live round-trip probe** (temporary integration test, deleted after; tree
  verified clean): real server via `build_router`/`build_state` on a local
  port → `PUT/GET /api/v1/notifications/preferences` over real HTTP (200,
  config round-trips; `in_app: false` → 400) → `notifications::notify_rich`
  for the token's user → in-app record persisted, email queued in the durable
  kv outbox, webhook and slack POSTs received by a local TCP listener.
- **Independent HMAC verification**: Python `hmac`/`sha256` over the exact
  body bytes captured on the wire matches the `x-gyre-signature` header the
  production `ReqwestSender` sent — the signature is a genuine HMAC-SHA256
  over the sent body with the configured secret (not a structural check and
  not the crate's own `sign_payload` re-invoked).
- **Mutation probe**: re-introducing the findings-07fd53c9 band bug
  (AgentEscalation → High instead of Urgent) fails both
  `agent_escalation_notifies_owners_even_when_spawner_online` and
  `priority_bands_match_spec_membership` — the regression tests kill the bug
  they were written for. Source restored, suite re-run green.

Evidence: `/tmp/stage/review-evidence/task-112-review-summary.txt` and siblings.

## Verified working

- **Domain types** match the spec struct shapes exactly
  (`gyre-domain/src/user.rs`): `NotificationChannels` (in_app always true —
  `Default` forces true and PUT rejects false), `EmailConfig`,
  `WebhookConfig` (HMAC secret), `SlackConfig`, ordered `NotificationPriority`,
  `DigestFrequency`.
- **Dispatcher** (`notification_dispatcher.rs`) fans each persisted in-app
  notification out to email/webhook/slack with per-channel `min_priority`
  threshold filtering; band derives from the notification TYPE via
  `for_notification_type` (spec membership table amended into
  user-management.md §Notification Entity). Webhook POST is HMAC-SHA256
  signed; Slack honors channel override; email queues a durable
  `email_outbox` kv entry (interface contract per the task plan). All
  outbound HTTP latency-bounded (reqwest 10 s timeout).
- **Routing engine** implements the §Who Gets Notified table: spec approval →
  manifest approvers (fallback owner, unknown ids collapse to the system
  inbox), agent escalation chain (spawned_by primary; presence-checked
  offline → workspace Admins; Urgent → Owners, dedup'd) wired into
  `fail_agent`, gate failure → MR author's spawner, MR merged/reverted →
  author (+ Admins for revert, replacing a fabricated `"default"` tenant_id
  with workspace-derived tenant resolution), merge queue paused →
  Admins/Owners, spec drift → workspace members + spec owner, persona
  approval → owner (creator-ownership set in `create_persona`), invitations
  ride existing paths. Budget warning/exhausted and security-finding helpers
  are contract-complete and tested but have no production emission triggers
  yet — documented decomposition boundary owned by task-191/task-137
  (verified: no callers outside the dispatcher). The team-lead row is
  amended in the spec with the interim superset routing and the
  task-122/125 dependency.
- **Preferences API** `GET/PUT /api/v1/notifications/preferences`: self-scope
  per-handler auth (ABAC-exempt with registered exemption), rejects
  `in_app: false` and non-absolute webhook/Slack URLs; storage in
  `user_channel_preferences` (portable migration 000059) via
  `UserChannelPreferenceRepository` with SQLite, Postgres, and mem adapters.
- **Stored-preferences compatibility** (findings 2facf1217 repair): custom
  `Deserialize` accepts both the nested and pre-task-112 flat shapes;
  migration 000060 converges stored rows with thorough regression coverage
  (flat-default/flat-custom/flat-in_app-false/nested/NULL rows, byte-equal
  convergence, canonical re-serializability, down.sql reverses the lossless
  subset).
- **Wiring**: `notify`/`notify_rich` fan out through `dispatch_to_channels`,
  and every direct `notifications.create` site (spec patrol, merge queue,
  abandoned branch, trust suggestion, spec-link staleness, git_http, mcp,
  divergence/mirror-sync via the `on_created` callback, constraint check,
  reconciliation) now delivers to configured channels.

## Test-inflation check

None found. The dispatcher tests assert real deliveries (URL, header, body,
kv outbox contents) through the `HttpSender` seam; the escalation tests drive
the full `notify_agent_escalation` → persistence path and assert per-recipient
inboxes; the band-membership table test pins the spec amendment; the
migration tests drive the real embedded up.sql. The mutation probe confirms
the key regression tests fail when the behavior breaks.

## Non-blocking observations (recorded for the follow-up task owners)

- `notify_security_finding` resolves "tenant Admin" as platform-wide
  `UserRole::Admin` holders (users are tenant-bound via `tenant_id`, but the
  filter does not match the workspace's tenant). This reading is consistent
  with the spec's own ABAC expression of TenantAdmin authority
  (§Ownership Transfer: `subject.global_role == TenantAdmin`, no tenant
  condition) and the helper has no production trigger yet — but when
  task-137 wires the security-finding escalation, either tenant-scope the
  recipient filter (`u.tenant_id == workspace's tenant`) or amend
  user-management.md to pin the intended semantics.
- The committed `web/dist` bundle is the assignment-base regeneration and
  does not contain the new notification-type labels added to
  `Inbox.svelte`/`WorkspaceHome.svelte`/`en.json` (cosmetic additions outside
  the task's acceptance criteria; the executor rebuilds web source remotely,
  and the current source builds clean — build.rs `npm run build` succeeded
  during this review's test compile).
- Migration 000059 `down.sql` opens with a stale "Revert migration 000057"
  comment (copy-paste; the DROP TABLE is correct). Cosmetic.
- Channel fan-out runs inline in emitting handlers (bounded at 10 s per
  channel by the reqwest timeout, errors warn-and-continue). The spec does
  not require asynchronous delivery; noted as a latency characteristic, not
  a defect.
- Standing sandbox restriction: the gawk-syntax gates
  (notification-audience, notification-priority, notification-test-coverage,
  tenant-filter, path-scope-binding, cross-surface-parity) cannot run under
  mawk — remote verification should run them.
