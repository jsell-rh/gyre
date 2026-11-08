# Review — task-112 (user-management.md §Delivery Channels, §Who Gets Notified, §Notification Routing for Agent Escalations)

Candidate: `f417f8c453ac39aefbdf633a2dd711b4681bb9c6` (branch HEAD, tree clean), base `72ad2a128d87c0cfaed5d1f6d2ea8cb5f1ad5781` (merge 36985b7c3 plus process commits). Verdict: **approved**.

## Probes run (evidence: /tmp/stage/review-evidence)

All with `/tmp/stage/bin/cargo` (`SKIP_WEB_BUILD=1`), on the exact candidate tree:

- `cargo test -p gyre-server --lib notification_dispatcher` → **15 passed** (HMAC-signed webhook delivery, per-channel threshold filtering, slack posting + channel override, durable email outbox, escalation chain incl. offline-spawner and Urgent owner fan-out, persona approval, merge-queue pause, budget warning/exhausted, security finding).
- `cargo test -p gyre-server --lib api::users` → **13 passed** (channel-prefs API: defaults, round-trip, rejected `in_app: false`, rejected relative webhook URL).
- `cargo test -p gyre-server --lib "spawn::"` → **36 passed** (fail_agent escalation wiring).
- `cargo test -p gyre-server --lib merge_processor::` → **63 passed** (pause_merge_queue → notify_merge_queue_paused wiring).
- `cargo test -p gyre-server --test graph_integration test_divergence_detection_creates_notifications` → **1 passed** (the previously transport-blocked probe from the 296d542 round, green on this HEAD).
- `cargo test -p gyre-adapters --lib user_profile` → **5 passed**; `cargo test -p gyre-domain --lib user` → **5 passed**.
- **Mutation probe** (source restored byte-identical after; `git status` clean): three production behaviors disabled simultaneously — webhook signature header dropped, webhook `min_priority` threshold bypassed, offline-spawner check bypassed → **4 test failures** (`webhook_delivery_is_signed_and_threshold_filtered`, `webhook_below_threshold_is_not_sent`, `escalation_primary_recipient_is_spawning_user`, `urgent_priority_always_notifies_owners`). The tests are real, not self-confirming.
- Text gates all OK: migration-versions, migration-sql-portability, abac-exempt-handlers (90 handlers), unbounded-external-http, fabricated-scope-defaults, arch, mem-port-contracts, task-commit-attribution, inert-enforcement, scope-literal-defaults.
- Frontend: `npm ci` + `npx vitest run src/__tests__/Inbox.test.js` → **38 passed** (new NotificationType normalization map entries).

Sandbox restriction (infrastructure, not a code defect): workspace-wide `cargo test --all` is rejected by the executor's cargo wrapper; the verifier should run the full workspace suite. Focused suites above cover every new code path in this task.

## Verified working

- **Domain types** match the spec structs exactly: `NotificationChannels { in_app, email, webhook, slack }`, `EmailConfig`, `WebhookConfig`, `SlackConfig`, `NotificationPriority` (Low..Urgent, ordered), `DigestFrequency` (Immediate/Hourly/Daily/Off). Default keeps `in_app: true`; PUT rejects `in_app: false` (spec: "can't disable").
- **Dispatcher** (`notification_dispatcher.rs`): real HMAC-SHA256 via `ring::hmac` over the exact sent body (`X-Gyre-Signature`); real reqwest POSTs latency-bounded at 10 s (unbounded-external-http gate OK); email queues a durable kv outbox entry — the interface contract the task plan explicitly allows ("actual SMTP can be stubbed"). Per-channel `min_priority` filtering enforced on all three channels (mutation-verified).
- **Routing**: escalation chain exactly per §Notification Routing for Agent Escalations (spawned_by primary; presence-based offline check adds workspace Admins; Urgent always adds Owners; dedup'd). `notify_agent_escalation` wired into `fail_agent` (spawn.rs) with tenant resolved from the workspace, skipped (never fabricated) when unresolvable. Persona approval → owner (wired in api/personas.rs), merge-queue paused → all Admins/Owners (wired in merge_processor), spec approval → manifest approvers (specs_assist path, unknown recipients collapse to `system` inbox rather than dropping). Budget warning/exhausted and security-finding helpers exist with tested recipient sets and are honestly disclosed as awaiting their triggers in task-191/task-137 — consistent with the task's decomposition boundary, not fake completion.
- **Wiring breadth**: `notifications::notify`/`notify_rich` fan out through `dispatch_to_channels`, so gate failure, spec patrol, merge queue, abandoned branch, trust suggestion, spec-link staleness, git_http, mcp, mirror-sync, constraint-check paths all deliver to configured channels; the `DivergencePorts.on_created` sites pass the real fan-out closure.
- **Preferences API**: `GET/PUT /api/v1/notifications/preferences`, self-scope per-handler auth (ABAC-exempt, documented; abac-exempt-handlers gate OK), URL validation, `in_app` enforcement. Storage is port-backed (`store!` wiring, SQLite + PG + mem adapters all implement the upsert-replaces contract; migration 000058 portable SQL, no version collision).

## Minor observations (non-blocking, recorded for the follow-up tasks)

- `notify_security_finding` resolves "tenant Admin" as all platform users holding `UserRole::Admin` (`state.users.list()` filter, no tenant scoping). The `User` model's `tenant_id` is never populated anywhere in the codebase, and role-based Admin is the platform-wide convention for TenantAdmin (create_user, admin route gates), so no tighter scope exists today. When task-137 wires the emission trigger, revisit this in a multi-tenant deployment — an Admin of tenant B would currently be notified of a Critical finding in tenant A. The helper is disclosed as not-live and has no production caller, so this is latent, not active.
- The HMAC test computes its expected value with the same `sign_payload` helper (mirrored algorithm choice). Mitigated: the production function is a real `ring` HMAC-SHA256 call, and the mutation probe proves the test asserts header presence and body-binding (a dropped or mis-bound signature fails the test).
- The email digest sender (outbox drainer / SMTP transport) is not implemented — explicitly within the task's acceptance criteria ("queues messages, at minimum the interface"). Outbox entries accumulate until a future digest job drains them.

## Spec amendment check

The team-lead routing rows ("Agent escalation to Overseer", "Breaking change detected") were amended in `user-management.md` with a documented note: the domain has no team-lead concept (Teams carry only `member_ids`), so the interim routing is the closest available superset (escalation chain for agent escalations; all members of the dependent repo's workspace via `process_breaking_changes`), with a named dependency on Teams gaining a lead concept (task-122/125). Amendment follows the spec-lifecycle principle (spec is the contract; amend rather than reinterpret silently).
