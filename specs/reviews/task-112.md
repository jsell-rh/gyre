# Review — task-112 (Implement notification delivery channels & routing)

Spec: `user-management.md` §Delivery Channels, §Who Gets Notified, §Notification Routing for Agent Escalations.
Candidate under review: `c9960c4a` (merge of base `c5deca80` at `43f51823`; product commits from checkpoint `7601f79a`).
Verdict: **needs-revision** (2 findings; evidence under `/tmp/stage/review-evidence/`).

## Probes run (candidate tree, /tmp/stage/bin/cargo, SKIP_WEB_BUILD=1)

- `cargo test -p gyre-server --lib notification_dispatcher` → 15 passed, 0 failed.
- `cargo test -p gyre-server --lib api::users` → 17 passed, 0 failed — but only 3 of the 4 claimed channel-prefs tests execute (F1 below; the compiler's dead_code warning is the proof).
- `cargo test -p gyre-server --lib merge_processor::tests::post_merge_fail_runs_full_recovery_protocol` → 1 passed (MrReverted admin fan-out asserted inside).
- `cargo test -p gyre-domain --lib user` → 11 passed. `cargo test -p gyre-adapters --lib user_profile` → 5 passed (SQLite round-trip/upsert-replaces included).
- Text gates: arch, abac-route-registry, abac-exempt-handlers (90 handlers), migration-versions, unbounded-external-http, in-memory-state-stores, dead-message-kinds, mem-port-contracts, byte-slice-truncation, relative-path-defaults, inert-enforcement, lossy-secret-conversion, scope-literal-defaults, fabricated-scope-defaults, forwarded-header-trust, forged-scope-fields, fail-open-ref-resolution, migration-sql-portability, mcp-write-tools, task-commit-attribution — all exit 0.
- gawk-gated notification checks (mawk-only sandbox): ported check-notification-test-coverage and check-notification-audience to Python and ran them on both candidate and base (worktree) — outputs byte-identical (36 pre-existing / 0 violations); **no new violations introduced by this task**. check-notification-priority's spec table is unchanged (ConstraintViolation:2 only) → passes.
- Web: `npm ci` then `npx vitest run src/__tests__/Inbox.test.js src/__tests__/WorkspaceHomeQueuePause.test.js` → 41 passed; `en.json` valid JSON.

## Verified working (no findings)

- **§Delivery Channels: PASS.** `NotificationChannels`/`EmailConfig`/`WebhookConfig`/`SlackConfig`/`NotificationPriority` match the spec structs exactly; `in_app` forced-true default and PUT rejection of `in_app: false`; PUT validates absolute webhook/Slack URLs. Dispatcher fans each persisted in-app notification to email (durable kv outbox — kv_store is SQLite/PG-backed in production via the `store!` wiring, mem only in pure-in-memory mode), webhook (real ring HMAC-SHA256 over the exact body bytes, `X-Gyre-Signature`, test pins signature-to-body and signature-to-secret), and Slack (channel override honored); per-channel `min_priority` threshold filtering proven by below-threshold suppression tests; all outbound HTTP latency-bounded (10 s reqwest timeout).
- **§Who Gets Notified: PASS on the wired rows.** Spec approval → manifest approvers (real `save_spec` resolution with system-inbox fallback, dedup); persona approval → owner (wired in `create_persona`); MR reverted → author + workspace Admins (wired in `apply_revert_side_effects`, asserted in the recovery test); merge queue paused → all Admins/Owners (wired in `pause_merge_queue`); invitation → invited user (existing); gate failure / MR merged rows unchanged and already covered. Budget warning/exhausted and security-finding helpers implement the correct recipient sets with real tests but have no production emission triggers — explicitly disclosed and owned by task-191/task-137; agreed this is a legitimate decomposition boundary, not fakery (the helpers are real, tested routing contracts).
- **§Notification Routing for Agent Escalations: PASS mechanically.** `escalation_recipients` implements spawner-primary / offline→Admins / Urgent→Owners with dedup; online status reads the live presence map (60 s janitor eviction); `fail_agent` routes through it with tenant resolved from the workspace (skip-and-log, never fabricated). The team-lead rows are honestly amended in the spec (superset interim routing, dependency on task-122/125) rather than faked.
- Storage: `user_channel_preferences` migration (000059, next unused sequence, portable SQL), port implemented in SQLite, Postgres, and mem with round-trip/upsert-replaces tests; ABAC-exempt route with genuine self-scope per-handler auth; mechanical gates all green.

## Findings

- [open] **F1 (test): `get_channel_preferences_returns_defaults` never runs.** `crates/gyre-server/src/api/users.rs:1373` — the async fn has no `#[tokio::test]` attribute and no caller; the compiler emits `warning: function 'get_channel_preferences_returns_defaults' is never used`. The task log claims four channel-prefs API tests; only three execute, and the GET-with-no-stored-row defaults path (`unwrap_or_default` → 200) is untested at the API level. Repair: add the attribute (or call it from a running test).
- [open] **F2 (code/spec): the 1-10 → band mapping contradicts §Notification Entity and kills the Urgent→Owners escalation branch in production.** `NotificationPriority::from_band` (gyre-domain/src/user.rs: 1-3 Urgent, 4-6 High, 7-8 Medium, 9-10 Low) is not derived from any spec (HSI §8 has no band thresholds and does not list AgentEscalation), and it inverts the spec's own band examples: agent escalation is spec-Urgent but maps to High (`AgentEscalation` numeric priority 5), gate failure is spec-High but maps to Urgent (3), budget warning is spec-High but maps to Medium (7). Consequence: in the only production escalation path (`fail_agent` → `notify_agent_escalation` → `escalation_recipients`), priority is always High, so §Notification Routing for Agent Escalations step 3 ("For Urgent priority: always notify workspace Owners") never fires — Owners are never notified on agent escalations, contradicting the spec's Urgent classification of that event. The unit test `urgent_priority_always_notifies_owners` hand-feeds Urgent, so the dead branch stays green. Repair options: map escalation-class types to their spec bands explicitly (agent escalation/MR reverted/security finding → Urgent; gate failure/budget warning → High), or amend user-management.md via spec lifecycle to define the numeric→band bridge — but as shipped the code contradicts the unamended spec. Also note the per-channel `min_priority` filtering semantics inherit the same inversion (an Urgent-threshold channel receives gate failures; a High-threshold email channel misses budget warnings).

## Non-findings (checked, judged acceptable)

- Webhook HMAC test uses mirrored logic (`sign_payload` on both sides) — would not catch a wrong-algorithm change, but pins signature-to-body/secret; `sign_payload` is genuine ring HMAC-SHA256 by inspection. Live HTTP round-trip deferred to host verification (sandbox mawk/listener restrictions are infrastructure, not code defects).
- User-configured webhook/Slack URLs can target internal addresses (SSRF surface) — the spec designs user-configured webhooks without an egress allowlist; worth a future hardening task, not a task-112 contract violation.
- Budget/security-finding helpers without triggers — disclosed decomposition boundary (task-191/task-137), helpers are real and tested.
- Migration `down.sql` comment says "Revert migration 000057" while the directory is 000059 — stale comment only, behavior-neutral (schema-name references).
- `ReqwestSender::default()` constructs a fresh reqwest client per dispatched notification — minor efficiency cost, not a defect.

## Summary

The delivery-channel layer, preferences API/storage, routing-table implementation, and wiring are real production code with meaningful tests, and every mechanical gate passes with no new lint violations. Two defects block approval: one claimed test that silently never runs (F1), and a priority-band bridge that contradicts the spec's band membership and renders the spec's Urgent→Owners escalation branch unreachable from the only production escalation trigger (F2). Both are narrowly repairable without disturbing the rest of the implementation.

— Reviewer, 2026-11 (independent review, candidate c9960c4a)
