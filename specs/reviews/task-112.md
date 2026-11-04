# Review — task-112 (Notification Delivery Channels & Routing)

Spec: `specs/system/user-management.md` §Delivery Channels, §Who Gets Notified,
§Notification Routing for Agent Escalations.
Candidate under review: `1dc5b7489b1aefc2e5a534e0b8f14cf898532604` (assigned base
`110180fbb248d6fa408492c0f9a3ec4a5a724372`, merged in via `b647c183`; rebase-repair
merges `d8e12aa4`, `0413ddc7` recorded in the task file).
Verdict: **approved** (round 1). Evidence under `/tmp/stage/review-evidence/`.

## What ran (independent probes, this sandbox)

- All 20 mechanical invariant scripts (`scripts/check-*.sh`): **all OK**
  (`scripts.log`) — including arch, ABAC route registry + exempt-handler,
  migration versions + SQL portability, mem port contracts, dead message
  kinds, unbounded external HTTP, fabricated-scope defaults (count frozen at
  4 with the merged-tree fix), scope-literal defaults, inert enforcement,
  in-memory state stores, task-commit attribution, forwarded-header trust,
  lossy secret conversion, byte-slice truncation, relative path defaults,
  fail-open ref resolution, forged scope fields, MCP write tools.
- `cargo test -p gyre-server --lib notification_dispatcher` → **15 passed**
  (first invocation failed on a transient crates.io download of `encoding_rs`
  — infrastructure, not code; immediate rerun green).
- `cargo test -p gyre-server --lib api::users` → **13 passed**;
  `spawn::` → **34 passed**; `mcp::` → **78 passed**; `merge_processor::` →
  **45 passed**; `gyre-adapters --lib user_profile` → **5 passed**;
  `gyre-domain --lib user::` → **5 passed**; `gyre-common --lib notification`
  → **4 passed**; `graph_integration test_divergence_detection_creates_notifications`
  → **1 passed** (the round-296d542 regression probe deferred by the repair
  round — now verified on this HEAD in this sandbox).
- Frontend: `npm ci` then `npm test` → **65 files / 1602 tests passed**
  (41 skipped), covering the touched `Inbox.svelte` / `WorkspaceHome.svelte` /
  `en.json` label-map additions.
- **Independent HMAC cross-check**: replicated `sign_payload` (ring
  HMAC-SHA256 + hex) in a standalone probe for two fixed inputs; outputs
  match `openssl dgst -sha256 -hmac` byte-for-byte (`hmac-probe-build.log`).
  This closes the mirrored-logic weakness of the in-repo test (which computes
  its expected value via the same helper): the scheme is standard HMAC-SHA256
  over the exact sent body with the configured secret.
- **Mutation probes** (temporary, auto-restored; tree verified clean after):
  dropping the `X-Gyre-Signature` header, dropping the offline-spawner→Admins
  escalation, and dropping the webhook `min_priority` filter each made the
  corresponding test **FAIL** (`mutation.log`). The tests are
  behavior-sensitive, not self-confirming.

## Verified working (no findings)

- **Domain types** (`gyre-domain/src/user.rs`): `NotificationChannels`
  (in_app/email/webhook/slack), `EmailConfig`, `WebhookConfig`, `SlackConfig`,
  `NotificationPriority` (ordered Low<Urgent; `from_band` maps HSI 1–10 onto
  the 4 bands), `DigestFrequency` — field-for-field conformant to the spec
  excerpt. `Default` keeps `in_app: true`.
- **Dispatcher** (`notification_dispatcher.rs`): per-channel
  `min_priority` threshold filtering (mutation-proven); webhook POST with
  HMAC-SHA256 signature header (independently cross-checked vs openssl);
  Slack POST with channel override; email queued as a durable outbox entry in
  the port-backed `kv_store` (SQLite + PG adapters wired via `store!`) — the
  task contract explicitly allows queue-interface-only email. All outbound
  HTTP is latency-bounded (10 s reqwest timeout), satisfying the
  unbounded-external-HTTP invariant.
- **Routing engine**: agent-escalation chain (spawned_by primary → Admins if
  spawner offline per the verified-identity presence map → Owners when
  Urgent; dedup'd; mutation-proven), persona approval → owner (wired in
  `create_persona`), merge queue paused → all Admins/Owners (wired in
  `pause_merge_queue`), MR reverted → author + workspace Admins
  (`notify_mr_reverted`), spec approval requested → manifest approvers with
  owner fallback (`save_spec`, replacing the old system-inbox-only fan-out),
  gate failure / MR merged / invitation rows on pre-existing paths now fanned
  out through `dispatch_to_channels` via the central `notify`/`notify_rich`
  helpers and per-site `else` branches (spec_patrol, spec_link_staleness,
  abandoned_branch, trust_suggestion, mcp, constraint_check, git_http via the
  new `DivergencePorts::on_created` callback).
- **Preferences API**: `GET/PUT /api/v1/notifications/preferences`, self-scope
  per-handler auth (resolver-exempt entry in `abac_middleware.rs`, consistent
  with the sibling `me/notifications` exemptions; exempt-handler script OK),
  rejects `in_app: false` and non-absolute webhook/Slack URLs; round-trip
  test reads back through a fresh router over the same state (durable store,
  not handler memory).
- **Storage**: migration `2026-10-08-000056_user_channel_preferences` is
  plain portable SQL (next free sequence number; version script OK);
  `UserChannelPreferenceRepository` port implemented in SQLite, Postgres, and
  mem with an upsert-replaces (not merges) round-trip test in the SQLite
  adapter — the mem/SQLite contract check passes.
- **Spec amendment** (56059f8d): the team-lead routing rows are amended with
  an interim superset routing and an explicit dependency on Teams gaining a
  lead concept. Verified accurate: `Team` carries only `member_ids` (no lead
  field, no repo→team mapping), and `process_breaking_changes` does notify all
  members of the dependent repo's workspace. This follows the
  amend-don't-reinterpret rule.
- **Repair round**: the `check_divergence` regression (dropped `entity_ref` /
  `repo_id`) is fixed byte-identical to the pre-regression code and the
  deferred `graph_integration` probe now passes on this HEAD in this sandbox.

## Decomposition boundary (verified accurate, not a defect)

`notify_agent_budget_warning`, `notify_agent_budget_exhausted`, and
`notify_security_finding` are real routing functions (real recipient
resolution against real membership/user state, real notification creation +
channel fan-out, hard tests asserting exact recipient sets) but have **no
production emission triggers yet** — owned by task-191 (platform-model
§Enforcement Behavior) and task-137 (agent-gates §Gate Failure Feedback).
Independently confirmed: at the assigned base, `BudgetWarning` had zero
emission sites in gyre-server (type definition only, listed in the
dead-message-kind exemptions), so there was nothing to wire into without
doing those tasks' work. The task record discloses this honestly; the
coverage auditor must not mark those three routing rows live until the
triggers land.

## Minor observations (non-blocking, for the record)

- The Urgent→Owners branch of the escalation chain is currently exercised
  only by tests: the sole production trigger (`fail_agent`) escalates at
  AgentEscalation's default priority 5 (High band). Correct per spec — Owners
  are added only for Urgent — and reachable by any future Urgent caller.
- `user_is_online` = WebSocket presence with verified user identity, not
  auth-session liveness; a session-holding user without an open tab counts
  offline, which only widens the recipient set (safe direction).
- The in-repo HMAC test computes its expected value via the same
  `sign_payload` helper (mirrored logic); mitigated here by the independent
  openssl cross-check and the signature-header mutation probe.
- URL validation on PUT preferences accepts any absolute scheme (e.g.
  `ftp:`); delivery failures are logged best-effort with a 10 s bound, so
  this is cosmetic hardening, not a security gap.
- Pre-existing (untouched by this diff, out of scope): the merge-processor
  circuit-breaker path notifies all workspace members with a literal
  `"default"` tenant id. Not a regression of this candidate; noted for a
  future cleanup task.
