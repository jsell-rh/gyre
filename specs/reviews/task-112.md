# Review: TASK-112 — Notification Delivery Channels & Routing

**Reviewer:** Independent review (pi runtime)
**Date:** 2026-11 (post repair-round 8269e906)
**Comparison base:** c5deca801a331e049f35b181f42d898df24e7e2b
**HEAD:** d21a4ced6fe445287fc94de38774c2a7b59f7be2
**Verdict:** approved

---

## Scope

user-management.md §Delivery Channels, §Who Gets Notified, §Notification
Routing for Agent Escalations — dispatcher (email/webhook/slack), routing
table, agent-escalation chain, preferences API, wiring into creation paths.

## Findings

No blocking findings. Two cosmetic observations recorded in the evidence file
(unused `Notification::with_priority` builder; stale "Migration 000057"
comment inside migration 000059's up.sql).

## Behavior verification

- [x] **Domain types** match the spec structs exactly (`NotificationChannels`
  with `email: EmailConfig`, `webhook: Option<WebhookConfig>`,
  `slack: Option<SlackConfig>`; `NotificationPriority` Low..Urgent ordered;
  `DigestFrequency`). `Default` keeps `in_app: true`.
- [x] **Band mapping.** `NotificationPriority::for_notification_type` matches
  the amended type→band table; verified by
  `priority_bands_match_spec_membership` and by mutation: moving
  AgentEscalation to the High band fails both that test and the full-path
  regression `agent_escalation_notifies_owners_even_when_spawner_online`
  (2 failed under mutation; source restored, tree clean).
- [x] **Dispatcher.** `dispatch_with` applies per-channel `min_priority` in
  the correct direction (Low < Medium < High < Urgent, `>=` inclusive);
  webhook suppressed below threshold; email suppressed when disabled/Off;
  slack channel override honored. All verified by the 16 dispatcher tests
  (independently re-run: 16 passed).
- [x] **Webhook HMAC.** Signature is HMAC-SHA256 (ring) over the exact sent
  body bytes, hex-encoded in `X-Gyre-Signature`. Mutation probe removing the
  header fails `webhook_delivery_is_signed_and_threshold_filtered` — the test
  is real, not structural.
- [x] **Slack delivery.** POSTs `{"text": …}` (+ `channel` override) to the
  configured incoming-webhook URL.
- [x] **Email delivery.** Durable outbox entries in `kv_store`
  (`email_outbox` ns) keyed `user:notif`, threshold- and enable-filtered.
  Queue-only interface is the task plan's stated contract ("actual SMTP can
  be stubbed").
- [x] **Escalation chain** (§Notification Routing for Agent Escalations):
  spawner primary; offline spawner (presence map, janitor-evicted, keyed by
  verified user id) adds workspace Admins; Urgent always adds Owners,
  deduplicated. Production path `fail_agent → notify_agent_escalation` wired
  (spawn.rs), tenant resolved from the workspace record with log-and-skip on
  failure (no fabricated "default").
- [x] **Routing table.** Persona approval → owner (create_persona now sets
  the creator as owner; `user:` prefix stripped; scope never fabricated);
  merge-queue paused → all Admins/Owners (wired in `pause_merge_queue`);
  budget warning → spawner+Admins, exhausted → spawner+Owner, security
  finding → workspace Owners + Admin-role users (helpers tested; emission
  triggers owned by task-191/137, documented decomposition boundary); MR
  reverted → author + workspace Admins (new); spec approval → manifest
  approvers with unknown-recipient collapse to system (improved from
  hardcoded "system"); spec drift → spec owner; breaking changes → documented
  interim superset (spec-amended). The security-finding tenant-admin
  selection (all `UserRole::Admin` users) is consistent with the platform's
  Admin-role semantics (`workspaces.rs` grants Admin-role users cross-tenant
  authority), so the recipient set exposes nothing the recipients cannot
  already access.
- [x] **Preferences API.** `GET/PUT /api/v1/notifications/preferences`
  self-scope per-handler auth (route is resolver-exempt with a mapping entry,
  not a frozen-file exemption); PUT rejects `in_app: false` and non-absolute
  webhook/Slack URLs; storage durable via `user_channel_preferences`
  (portable SQL, SQLite/PG/mem adapters all enforce the upsert contract —
  mem-port-contracts gate passes). 18 `api::users` tests pass, including the
  four channel-prefs tests.
- [x] **Wiring.** `notify`/`notify_rich` fan out through
  `dispatch_to_channels`; raw-repo creation paths (graph_extraction
  divergence/spec-assertions via `on_created` callback, abandoned_branch,
  trust_suggestion, spec_patrol, constraint_check, mcp,
  emit_reconciliation_completed, git_http cross-workspace) all dispatch.

## Gates

All 23 runnable text gates pass (arch, abac-route-registry,
abac-exempt-handlers, dead-message-kinds, unbounded-external-http,
byte-slice-truncation, inert-enforcement, migration-versions,
mem-port-contracts, relative-path-defaults, fabricated-scope-defaults,
lossy-secret-conversion, scope-literal-defaults, forwarded-header-trust,
in-memory-state-stores, forged-scope-fields, migration-sql-portability,
task-commit-attribution, unwired-port-methods, api-auth, message-delivery,
sandbox-sweep-artifacts, mcp-write-tools). gawk-syntax gates cannot run in
this mawk-only sandbox (infrastructure, not a code defect) — remote verifier
should run notification-audience / notification-priority /
notification-test-coverage / tenant-filter / path-scope-binding /
cross-surface-parity.

## Test evidence

Independent re-runs (all pass): notification_dispatcher 16, api::users 18,
notifications 12, gyre-domain user 11, gyre-adapters user_profile 5,
graph_integration divergence 1; web vitest (Inbox, WorkspaceHome, last-seen)
48. Two mutation probes (band regression, HMAC removal) both caught by the
relevant tests; source restored clean afterward. Full logs:
`/tmp/stage/review-evidence/task-112-review.txt`.
