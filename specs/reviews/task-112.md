# Review — task-112 (notification delivery channels & routing)

Spec: `specs/system/user-management.md` §Delivery Channels, §Who Gets Notified, §Notification Routing for Agent Escalations (coverage rows 16–18).
Candidate under review: `abbbb1d3` on `pipeline/task-112/780235fa6abe43f68425f947c440fc6e-1`, diff scoped from base `fefde294` (the branch's merge base with main at assignment time). Commits attributed per the task's `commits:` frontmatter plus the recorded merge/process commits.
Verdict: **complete** (approved). Evidence: `/tmp/stage/review-evidence/task-112-review-round.txt`.

## Round 1 (this review)

Test runs (this checkout at `abbbb1d3`, `/tmp/stage/bin/cargo`, `SKIP_WEB_BUILD=1`):

- `cargo test -p gyre-server --lib notification_dispatcher` → **16 passed, 0 failed**.
- `cargo test -p gyre-server --lib "api::users"` → **18 passed, 0 failed** (incl. the four channel-prefs API tests).
- `cargo test -p gyre-server --lib` (full crate lib suite) → **1505 passed, 0 failed**.
- `cargo test -p gyre-domain --lib user` → **11 passed**; `cargo test -p gyre-common --lib notification` → **4 passed**; `cargo test -p gyre-adapters --lib channel_prefs` → **1 passed** (SQLite round-trip + upsert-replaces).
- `cd web && npm ci && npm test` → **89 files, 1936 passed, 41 skipped** (web/src label additions for the new notification types).
- Mechanical gates: arch, abac-route-registry, abac-exempt-handlers, mem-port-contracts, migration-versions, migration-sql-portability, in-memory-state-stores, unbounded-external-http, inert-enforcement, fabricated-scope-defaults, task-commit-attribution, dead-message-kinds → **all OK**.
- Mutation probes (each restored and md5-verified; `git status --short` clean after):
  - Drop the `X-Gyre-Signature` header from webhook delivery → `webhook_delivery_is_signed_and_threshold_filtered` **FAILS**.
  - Disable the Urgent→Owners escalation branch (`if false && priority == Urgent`) → `agent_escalation_notifies_owners_even_when_spawner_online` and `urgent_priority_always_notifies_owners` **FAIL**.
  - Disable per-channel threshold filtering (`if true || priority >= min_priority`) → `webhook_below_threshold_is_not_sent` **FAILS**.
  - Reintroduce the historical band bug (AgentEscalation → High instead of Urgent in `for_notification_type`) → the full-path regression and `priority_bands_match_spec_membership` **FAIL**. The prior finding-07fd53c9 repair is genuinely anchored.
- Independent HMAC cross-check: a standalone ring probe mirroring `sign_payload` over the exact serialized webhook body produces `341c1b5e…fea3`, byte-identical to Python's `hmac.new(secret, body, sha256)` — real, standards-compliant HMAC-SHA256, and the dispatcher test pins the header↔body↔secret pairing.

Verified working (no findings):

- **Domain types** match the spec's §Delivery Channels structs exactly: `NotificationChannels { in_app, email: EmailConfig, webhook: Option<WebhookConfig>, slack: Option<SlackConfig> }` with ordered `NotificationPriority` (Low<Medium<High<Urgent) and per-channel `min_priority` compared with `>=` on the derived ordinal.
- **Dispatcher** (`notification_dispatcher.rs`) fans each persisted in-app notification out to email/webhook/slack with per-channel `min_priority` filtering; the band derives from the notification type via `for_notification_type` (the full type→band table is amended into user-management.md §Notification Entity, with a sound rationale for not threshold-mapping the HSI §8 numeric scale — verified: SecurityFinding and GateFailure share numeric 3 yet sit in different bands). Webhook POSTs the JSON payload signed with real HMAC-SHA256 (ring) as `X-Gyre-Signature`; Slack POSTs to the incoming webhook URL honoring the channel override; email queues a durable outbox entry in `kv_store` (`email_outbox` ns) — the queue interface the task plan authorizes. All outbound HTTP is latency-bounded (reqwest client with 10 s timeout; `check-unbounded-external-http.sh` passes).
- **Routing table** (§Who Gets Notified) is covered row by row: spec approval → manifest approvers (fallback owner; unknown ids collapse to the "system" inbox), persona approval → owner (owner derived from the authenticated user at create, not caller-supplied), agent escalation → the §Agent Escalations chain, gate failure / MR merged → author's spawning user, MR reverted → author + workspace Admins (test asserts the Admin fan-out through the real `apply_revert_side_effects` path), merge queue paused → all Admins/Owners, spec drift → spec owner (`notify_spec_owner_of_drift`, owner from the spec ledger entry), breaking change → interim superset routing recorded as a spec amendment pending a Teams lead concept (task-122/125), invitation → invited user. Budget warning/exhausted and security-finding helpers are implemented and tested but intentionally unwired pending their emission triggers in task-191/task-137 — disclosed in the task notes, consistent with the decomposition.
- **Agent escalation chain** (§Notification Routing for Agent Escalations) implemented literally: `spawned_by` primary; offline spawner (presence map, 60 s eviction janitor) adds workspace Admins; Urgent always adds workspace Owners; dedup'd. The full-path regression proves Owners are notified even with an online spawner — the exact bug class the prior review round found.
- **Preferences API**: `GET/PUT /api/v1/notifications/preferences` registered with ABAC exemption + per-handler self-scope auth (reads/writes only the authenticated user's row via `resolve_user_id`); PUT rejects `in_app: false` per the spec's "can't disable" and non-absolute webhook/Slack URLs. Storage is port-backed (`UserChannelPreferenceRepository`: SQLite, Postgres, mem — all enforce upsert-replaces; SQLite test proves no stale-webhook merge), migration `2026-10-08-000059_user_channel_preferences` uses the next unused sequence and portable SQL.
- **Wiring** is comprehensive, not spot-wired: `notify`/`notify_rich` fan out through `dispatch_to_channels`, so gate failure, spec patrol, merge queue, abandoned branch, trust suggestion, spec-link staleness, git_http divergence/spec assertions (via the `on_created` callback), mirror sync/clone, constraint violations, MCP uncertainty, reconciliation, and secret expiry all deliver to configured channels; `fail_agent` routes through the escalation engine.
- **No fakery found**: real crypto (cross-checked against an independent implementation), real storage (durable table + adapters), real HTTP with error propagation into logs, real online-status evaluation, real threshold enforcement — each proven by a mutation probe that flips the behavior and fails a test.

Infrastructure note (not a code defect): this sandbox has only mawk, so the gawk-syntax gates (`check-notification-audience.sh`, `check-notification-priority.sh`, `check-notification-test-coverage.sh`) exit 2 on awk syntax errors — matching the task's standing disclosure. I emulated the audience gate's Check 1 in Python (0 violations) and verified the priority gate's table entry (`ConstraintViolation => 2`) by grep. **Verifier must run those three scripts under gawk on the host.**

Findings:

- (none)

Minor (not blocking, recorded for completeness):

- `docs/api-reference.md` has no rows for the new `GET/PUT /api/v1/notifications/preferences` endpoints. Peer route-adding tasks (089/098/103/157) update the reference; the task's acceptance criteria do not include docs and no gate enforces it. A two-row docs addition would close this.
- The migration's `up.sql` comment says "Migration 000057" while the directory is `2026-10-08-000059_user_channel_preferences` — cosmetic comment mismatch only (the version gate keys on the full directory prefix).
- `PUT .../preferences` URL validation accepts any absolute URL scheme (e.g. `file://`); reqwest rejects non-http(s) schemes at delivery time with a logged warn (best-effort channel), so there is no silent misbehavior — but restricting to http(s) at validation time would fail faster.
