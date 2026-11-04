# Review — task-112 (Implement notification delivery channels & routing)

Spec: `specs/system/user-management.md` §Delivery Channels, §Who Gets
Notified, §Notification Routing for Agent Escalations.

Candidate: `9f6737047e2894562aa76fc17dfd8c257c42e615` (base
`918f16bfbf7f85cc9d65b1316a3af35a5e6d5ecb`; assigned commits
`a3a3ea0f`, `94cf2f62`, `896c9055`, `ee4e4342`, `5c37f969` all present in
the range and recorded in the task frontmatter).

Verdict: **approved** — real, production delivery-channel and routing
implementation, independently verified end-to-end against a live server.

## Independent evidence (all at candidate HEAD, shared target dir)

- **Live end-to-end probe** (debug build of the actual `gyre-server` binary,
  in-memory storage, local HTTP capture listener on 127.0.0.1:8977 — this
  sandbox *does* permit TCP listeners, unlike the implementer's, so the
  deferred live round-trip was performed here; scripts and captured traffic
  in `/tmp/stage/review-evidence/`):
  - `PUT/GET /api/v1/notifications/preferences` round-trips a full channel
    config (webhook + slack + email) — 200, persisted across a fresh router.
  - Persona creation (`POST /api/v1/personas`, workspace scope) routed a
    `PersonaApprovalRequested` notification to the persona owner (the
    creator): in-app record confirmed via `GET /api/v1/users/me/notifications`
    (same notification id as the fanned-out payload).
  - **Webhook delivery arrived at the live listener** with header
    `X-Gyre-Signature` = `a66dddb7…`, which **matches an independent
    Python `hmac` HMAC-SHA256 over the exact received body bytes keyed with
    the configured secret** — not a mirrored-logic assertion; the signature
    is verified against an implementation that shares no code with the Rust
    path.
  - **Slack delivery arrived** at the configured incoming-webhook URL with
    the `channel` override (`#probe`) and human-readable `text`.
- **Focused suites** (all pass, matching the shipped claims):
  `notification_dispatcher` 15, `users` 17, `spawn::` 29, `notifications` 10,
  `mcp` 71, `merge_processor` 49, adapters `user_profile` 5, domain `user::` 5.
- **Invariant scripts** (14 run, all OK): arch, abac-route-registry,
  abac-exempt-handlers (91 handlers), unbounded-external-http,
  migration-versions, migration-sql-portability, fabricated-scope-defaults,
  in-memory-state-stores, lossy-secret-conversion, dead-message-kinds,
  byte-slice-truncation, relative-path-defaults, mcp-write-tools,
  inert-enforcement, forwarded-header-trust, forged-scope-fields,
  mem-port-contracts, scope-literal-defaults.

## Verified correct against the spec

- **Domain types match §Delivery Channels exactly**: `NotificationChannels`
  (in_app/email/webhook/slack), `EmailConfig` (enabled/digest/min_priority),
  `WebhookConfig` (url/secret/min_priority), `SlackConfig`
  (webhook_url/channel/min_priority), `NotificationPriority`
  (Low<Medium<High<Urgent, ordered derive → threshold `>=` is correct),
  `DigestFrequency` = Immediate/Hourly/Daily/Off (spec-exact; the shipped
  summary's "Weekly" was a summary typo — the code is correct).
  `in_app: false` is rejected by the PUT handler (spec: "can't disable") and
  the `Default` keeps it true.
- **Dispatcher**: `dispatch_to_channels` loads the recipient's persisted
  config (`user_channel_prefs`, falling back to defaults), applies
  per-channel `min_priority` thresholds (positive and negative tests), signs
  webhook bodies with ring HMAC-SHA256, posts Slack payloads with channel
  override, and queues email outbox entries in `kv_store` (durable; the task
  plan explicitly accepts the queue as the interface — SMTP/digest sending is
  task-113's row). All outbound HTTP is latency-bounded (10 s reqwest
  timeout). Every direct-creation path (spec_patrol, trust_suggestion,
  abandoned_branch, mcp, graph_extraction via `on_created`, invite_member)
  now fans out in addition to the central `notify`/`notify_rich` helpers.
- **Escalation routing** follows §Notification Routing for Agent Escalations
  (the operative algorithm, which supersedes the table row's looser
  phrasing): `spawned_by` primary; offline spawner (presence map, backed by
  the 60 s eviction janitor running in production `main`) adds workspace
  Admins; Urgent always adds Owners; dedup'd. Wired into `fail_agent`, which
  also replaces the previous fabricated `"default"` tenant with a real
  workspace→tenant resolution (skip+log on failure).
- **Routing table rows**: persona approval → owner (wired in
  `create_persona`, `user:` prefix stripped, scope never fabricated);
  merge-queue paused → all Admins/Owners (wired in merge_processor);
  MR reverted → author + workspace Admins (new admin fan-out); spec drift →
  spec owner (`notify_spec_owner_of_drift` in spec_link_staleness, wired into
  the staleness run loop); spec approval → manifest approvers with owner
  fallback (existing path, now channel-fanned); gate failure / MR merged /
  invitation / breaking change → dependent-repo workspace members
  (pre-existing paths).
- **Storage**: new `UserChannelPreferenceRepository` port implemented by
  SQLite, Postgres, and mem adapters with consistent replace-semantics
  upsert; migration `2026-10-08-000056` uses the next sequence number and
  portable SQL; AppState wiring via the `store!` pattern (no bare
  Arc<Mutex<HashMap>>). The route is ABAC-exempt with self-scope per-handler
  auth (reads/writes only the caller's own row), matching the existing
  `/api/v1/users/me/notifications` pattern; the exempt-handler check passes.
- The task also fixed a pre-existing fabricated-scope violation
  (git_http.rs `ws_id` "default" fallback) and correctly *reduced* the frozen
  exemption count 7→6 (removal, not addition).

## Notes (non-blocking, recorded for follow-up tasks)

- `notify_agent_budget_warning`, `notify_agent_budget_exhausted`, and
  `notify_security_finding` are real, tested routing helpers with **no
  production caller yet** — their emission triggers are owned by not-started
  tasks (190/191 budget usage recording + enforcement; 137 security-gate
  escalation). Nothing can wire them today: `record_budget_usage` itself has
  no production caller, so the triggering events do not exist anywhere.
  This is a disclosed decomposition boundary, not fakery; when task-191
  lands it should call these helpers (its current plan text says
  `notifications::notify(...)`, which would notify the spawner only and miss
  the spec's Admin/Owner fan-out).
- `notify_security_finding` resolves "tenant Admin" as all platform users
  holding `UserRole::Admin` (global `users.list()`, no tenant filter). This
  matches today's platform model (Admin → global `TenantAdmin` role; locally
  created admins have no `tenant_id`), but before task-137 makes the path
  reachable, tenant scoping should be revisited for multi-tenant deployments
  so a tenant-B admin does not receive tenant-A finding details.
- The email outbox (`kv_store` ns `email_outbox`) has no drain job yet —
  consistent with this task's "interface at minimum" criterion; digest
  sending is §Email Notifications (task-113).

Full `cargo test --all` and all-target Clippy are left to the verification
stage per the review protocol; all focused probes relevant to the three
coverage sections pass.
