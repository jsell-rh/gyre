# Review — task-162 (Message bus — MessageConsumer dispatcher + event TTL expiry job)

Spec: `specs/system/message-bus.md` §Relationship to Notifications, §Implementation Notes.
Candidate: `3b93bd98b7993789449bcf3ed9b4d1ad6bba993a` (base `3c39ab1f`; sole product commit `51221dc0`, merge `62862a9e`, attribution/process `3b93bd98` correctly outside `commits:`).
Verdict: **complete** (approved).

## Probes (SKIP_WEB_BUILD=1 via /tmp/stage/bin/cargo; cold build; logs in /tmp/stage/review-evidence)

- `cargo test -p gyre-server --lib message_dispatcher` → **9 passed, 0 failed** (bridge unit tests for all four kinds, viewer exclusion, unknown-workspace skip, dispatcher fan-out, end-to-end spawn_message_consumer, TTL retention boundaries, env-var TTL override).
- `cargo test -p gyre-server --lib gate_executor` → **25 passed**, incl. `failed_gate_creates_gate_failure_notification_via_bridge` (run_gate → emit_event → dispatcher → bridge → exactly one p3, correct tenant/entity_ref).
- `cargo test -p gyre-server --lib notifications` → **9 passed** (no callers broken by `notify_gate_failure` removal).
- `cargo test -p gyre-server --lib message` → **45 passed**; `--lib ack` → **61 passed**; `--lib mem::` → **10 passed** (mem ack-reason changes break nothing).
- `cargo check -p gyre-server --tests` → clean, zero warnings.
- Mechanical gates on the candidate tree all pass: arch, byte-slice, dead-message-kinds, fabricated-scope, scope-literal, inert-enforcement, mem-port-contracts, migration-versions, abac-route-registry, mcp-write-tools, task-commit-attribution, forwarded-header, lossy-secret, unbounded-http, relative-path, fail-open-ref, in-memory-state-stores.
- `check-notification-test-coverage.sh` exits 2 on this host before scanning any file — its awk uses gawk-only 3-arg `match()`; host has mawk 1.3.4. Candidate touches no scripts; pre-existing host-tooling issue, unrelated.

## Verified working (no blocking findings)

- **Dispatcher is real and single-owner.** `spawn_message_consumer` takes the 256-cap mpsc receiver via `AppState::take_message_dispatch_rx` (Mutex<Option<Rx>>; second call is a logged no-op) and spawns the drain task; `MessageDispatcher` holds `Vec<Arc<dyn MessageConsumer>>` and dispatches every message. Wired from `main.rs:86`. The old no-op drain spawned inside `build_state` is removed.
- **NotificationBridge is a genuine consumer.** GateFailure→p3 (MR→author agent→spawned_by; tenant resolved from the workspace record, never fabricated), BudgetWarning→p7 and ReconciliationCompleted→p6 for Admin/Developer/Owner members (Viewer excluded), AgentError→p5 AgentEscalation for the spawning user. Unknown workspace/agent → logged skip. Forged-event vector closed: these kinds are `server_only`, rejected on REST/MCP send. All four paths test-proven against real repositories through the real channel.
- **Single p3 creation path.** The gate executor's synchronous p3 code and `notify_gate_failure` are removed (no remaining callers, grep-verified); HSI §8 p3 amended via the established "Upstream Amendments Applied" table to align with message-bus.md §Relationship to Notifications (the canonical spec named by the task's spec_ref). This also removes the old hardcoded-tenant `"default"` call site.
- **TTL job is real and periodic.** `spawn_message_expiry` (main.rs:87) runs a 3600s interval loop (MissedTickBehavior::Delay; first tick immediate), each cycle calling `expire_events(now − GYRE_EVENT_TTL_SECS·1000)` and `expire_acked_inboxes(now − GYRE_DEAD_INBOX_TTL_SECS·1000)`; both env vars default 604800 with warn-on-invalid fallback, re-read per cycle. Registered as `message_expiry` in the admin job registry (on-demand `POST /admin/jobs/message_expiry/run`) with no double scheduling (own loop, not `spawn_job`).
- **Adapter contract parity.** `MemMessageRepository` now tracks ack reasons ("explicit" on `acknowledge`, passed reason on `acknowledge_all`) so `expire_acked_inboxes` deletes only agent-targeted messages bulk-acked `agent_completed`/`agent_orphaned` older than the cutoff — matching the SQLite and Postgres adapters (sqlite/message.rs:400-419). Retention boundaries tested: unacked Directed and explicitly-acked messages retained.
- **Tests kill real bugs, not mirrors.** End-to-end tests drive the real `spawn_message_consumer` + channel and assert exact counts/priorities/tenants; a disabled bridge or a broken emit path fails them (empty notification list → assert). The gate-executor test pins the whole rewired run_gate→bridge chain.

Minor (non-blocking, recorded for completeness):

- `api/messages.rs:306` and `mcp.rs:2008` use `let _ = try_send(...)` — no warning log on channel-full, unlike `emit_event`. Spec's "dropped with a warning log" is honored on the server emit path (the send path the spec paragraph describes); both lines are pre-existing and untouched by this diff. Observability-only.
- `message_expiry_env_ttl_respected` uses a 1s TTL with `created_at=now` for the retained fixture — an extreme (>1s) scheduler delay between store and expiry could delete it and fail the assert. Flake risk under load only; fails loudly, never passes falsely.
- `BudgetWarning`/`AgentError` still have no production emitters (pre-existing, tracked in `dead-message-kind-exemptions.txt` for task-191; HSI row 43 gap note discloses it). The consumer side — this task's contract — is wired and test-proven. Likewise `emit_reconciliation_completed` having no production caller is pre-existing at base (verified via `git grep` at `3c39ab1f`), not a regression of this candidate.
