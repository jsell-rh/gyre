# Review — task-112 (Implement notification delivery channels & routing)

Spec: `specs/system/user-management.md` §Delivery Channels, §Who Gets Notified,
§Notification Routing for Agent Escalations.
Candidate under review: `01773127b` (merge of assignment base `3d87a1507`,
task-109 spec-lifecycle/impersonation, into the task-112 branch).
Task commits: `606ef649f`, `9558f4091`, `ea8dd97b7`, `8269e9067`, `7601f79ab`
(plus merge/process commits on the branch).
Verdict: **approved** — no findings. All acceptance criteria verified with
independent evidence, including a live HTTP round-trip probe with an
independently computed HMAC cross-check.

## Probes run (candidate `01773127b`, this sandbox)

- `cargo test -p gyre-server --lib` → **1649 passed, 0 failed** (full suite;
  includes notification_dispatcher 16, api::users 22, api::spawn 46,
  api::spec_lifecycle 9, impersonation 15).
- `cargo test -p gyre-adapters --lib` → **414 passed, 0 failed, 6 ignored**
  (includes migration 000058 backfill, 000060 channel-shape convergence,
  no_duplicate_migration_version_prefixes).
- `cargo test -p gyre-domain --lib` → **450 passed**; `cargo test -p
  gyre-common --lib` → **120 passed**.
- Text gates all rc=0: migration-versions, migration-sql-portability, arch,
  abac-route-registry, abac-exempt-handlers, unbounded-external-http,
  in-memory-state-stores, mem-port-contracts, fabricated-scope-defaults,
  scope-literal-defaults, lossy-secret-conversion, inert-enforcement,
  forged-scope-fields, dead-message-kinds, mcp-write-tools,
  byte-slice-truncation, relative-path-defaults, forwarded-header-trust,
  fail-open-ref-resolution.
- **Live webhook round-trip probe** (temporary integration test, deleted
  after run; evidence `/tmp/stage/review-evidence/live-webhook-probe.txt`):
  drove the production `ReqwestSender` through `dispatch_with` against a real
  TCP listener. The raw POST arrived at the configured path with
  `content-type: application/json` and `x-gyre-signature`. The signature was
  cross-checked against an **independently computed** Python
  `hmac.new(secret, body, sha256)` over the exact 207-byte request body —
  **MATCH** (`98373ff5…e3f79d`). This proves the webhook channel performs a
  real HTTP delivery signed with genuine HMAC-SHA256 over the sent bytes,
  not a structural/mirrored check.

## Verified against the task contract

- **Domain types** (`gyre-domain/src/user.rs`): `NotificationChannels`
  (`in_app` always-true default, `email: EmailConfig`,
  `webhook: Option<WebhookConfig>`, `slack: Option<SlackConfig>`),
  `NotificationPriority` (Low..Urgent, ordered),
  `DigestFrequency` — exact match to the spec structs. The custom
  `Deserialize` keeps pre-task-112 flat `users.preferences` rows readable
  (no silent defaults-reset on upgrade); migration 000060 converges stored
  rows to the nested shape (all 8 flat combinations, portable REPLACE/LIKE
  SQL for both SQLite and PG).
- **Dispatcher** (`notification_dispatcher.rs`): per-channel
  `min_priority` threshold filtering with the band derived from the
  notification TYPE via `for_notification_type` (exhaustive match; band
  table amended into user-management.md §Notification Entity, including the
  merge-round ImpersonationRequest/Alert arms). Webhook HMAC-SHA256 verified
  live (above). Slack POST honors channel override. Email queues a durable
  `kv_store` outbox entry (interface contract per the task plan). All
  outbound HTTP latency-bounded (reqwest 10 s timeout — gate green).
- **Routing engine**: `escalation_recipients` implements the full §Agent
  Escalations chain — `spawned_by` primary, offline spawner (presence map,
  60 s janitor spawned in `main.rs`) adds workspace Admins, Urgent always
  adds Owners, dedup'd; full-path regression proves Owners are notified for
  AgentEscalation even with an online spawner (review repair 07fd53c9).
  Routing-table rows: persona approval → owner (`personas.rs` wired), merge
  queue paused → Admins/Owners (`merge_processor.rs` wired), MR reverted →
  author + Admins, spec approval → manifest approvers (existing
  `specs_assist.rs` path), breaking change → interim superset routing with
  the spec amendment recording the missing team-lead concept (task-122/125).
  Budget warning/exhausted and security-finding helpers are routing
  contracts whose emission triggers are owned by task-191/task-137 per
  other specs — decomposition boundary documented in the task record and
  coverage rows; the helpers themselves are real and tested.
- **Preferences API**: `GET/PUT /api/v1/notifications/preferences`
  registered in `api/mod.rs`, ABAC-exempt in the resolver with genuine
  self-scope per-handler auth (reads/writes only the caller's own row via
  `resolve_user_id(&auth)`); PUT rejects `in_app: false` and non-absolute
  webhook/Slack URLs (round-trip + rejection tests green). Storage is
  port-backed (`UserChannelPreferenceRepository` with SQLite, Postgres, and
  mem adapters; migration `000061` renumbered past the base's duplicate
  `000059`, verified by the migration-versions gate and the
  spec_lifecycle e2e test).
- **Wiring**: `notifications::notify`/`notify_rich` fan out through
  `dispatch_to_channels` (fail-closed: no fan-out when the in-app record
  fails to persist); `spawn.rs` `fail_agent` calls `notify_agent_escalation`
  with tenant resolved from the workspace (never fabricated).
- **Coverage matrix**: rows 16–18 marked `implemented` with accurate
  detail; no `not-started` rows introduced; task frontmatter commits cover
  the product commits.

## Notes (non-blocking)

- The unit test's signature assertion recomputes `sign_payload` (mirrored),
  but the live probe above independently confirms genuine HMAC-SHA256, and
  the assertion still binds the signature to the exact sent body/secret.
- reqwest emits the signature header lowercase (`x-gyre-signature`);
  HTTP header names are case-insensitive (RFC 9110 §5.1), so receivers
  must treat it case-insensitively — standard webhook practice.
