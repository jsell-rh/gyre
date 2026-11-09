# Review — task-162 (Message bus: MessageConsumer dispatcher + event TTL expiry job)

Candidate: `cea3f23b49fc4f6e2062f623771e1a98bf6984e6` · Base: `fefde2946f3a0af1f212588f54591516fee0f7b2`
Round: independent review (round 3 lineage; prior code+process findings from round at `152e67033`).

## What was checked

**Diff surface** (14 files, +1228/−192): `message_dispatcher.rs` (new), `lib.rs`, `main.rs`, `gate_executor.rs`, `mem.rs`, `reconciliation.rs`, `notifications.rs`, `jobs.rs`, `middleware.rs`, plus coverage/HSI/task spec files.

**Dispatcher (§Relationship to Notifications) — genuine.**
- `spawn_message_consumer()` takes single ownership of the 256-cap mpsc receiver via `AppState::take_message_dispatch_rx()` (Option::take under mutex; second call is a logged no-op) and spawns the drain task. The old no-op drain loops in `build_state`/`test_state` are removed — no competing receiver can steal messages.
- `MessageDispatcher` holds `Vec<Arc<dyn MessageConsumer>>` and dispatches every drained message; `NotificationBridge` is the registered consumer.
- All three send paths clone into the channel **after** store: `AppState::emit_event` (lib.rs), REST `api/messages.rs:306`, MCP `mcp.rs:2028`. `try_send` failures log and never block (best-effort tier per spec).
- Bridge scope handling is real, not fabricated: tenant resolved from the workspace record (`None` → skip+warn, no `"default"` fallback); GateFailure resolves MR → author agent → `spawned_by` and skips when either lookup fails; BudgetWarning/ReconciliationCompleted fan out to Admin/Developer/Owner members only (Viewer excluded — tested). The gate executor's synchronous p3 path and `notify_gate_failure` are removed (single creation path, HSI §8 p3 as amended in this diff).

**TTL expiry job (§Implementation Notes) — genuine.**
- `spawn_message_expiry()`: 3600s interval, `MissedTickBehavior::Delay`, job-registry-tracked (`mark_scheduled`/`record_cycle`, same pattern as `spawn_budget_daily_reset`), registered as `message_expiry` in `start_job_registry` (on-demand `POST /admin/jobs/message_expiry/run`), wired in `main.rs`.
- `run_message_expiry()` reads `GYRE_EVENT_TTL_SECS`/`GYRE_DEAD_INBOX_TTL_SECS` per cycle (default 604800 each, warn-on-invalid) and calls `expire_events` + `expire_acked_inboxes`.
- Mem adapter `ack_reason` tracking checked line-by-line against the SQLite adapter: explicit acks and unacked Directed messages retained; only `agent_completed`/`agent_orphaned` bulk-acked agent inboxes older than the cutoff are deleted. Contracts match.

**Prior findings (73a69995).** Code half verified fixed: the reconciliation wave-completion test now spawns the real dispatcher and polls; it passes at the candidate. Process half **regressed at head** — see finding.

**Probes** (SKIP_WEB_BUILD=1, `/tmp/stage/bin/cargo`; evidence in `/tmp/stage/review-evidence`):
- `--lib message_dispatcher` → 9 passed; `--lib gate_executor` → 25 passed (incl. bridge e2e); `--lib reconciliation::tests::reconciliation_completed_fires_when_all_tasks_terminal` → 1 passed; `--lib notifications` → 12 passed.
- **Mutation probe**: disabling the bridge's `ReconciliationCompleted` arm makes the wave-completion test FAIL ("expected MetaSpecDrift notification on wave completion") — the e2e tests are anchored to real production behavior, not self-confirming. Source restored; HEAD re-verified at `cea3f23b4`, tree clean, suite re-green.
- Mechanical checks pass at candidate: arch, mem-port-contracts, dead-message-kinds, task-commit-attribution, fabricated/scope-literal defaults, inert enforcement.

## Finding

**F1 (process, `specs/tasks/task-162.md`).** Candidate head `cea3f23b4` ("process: record task-162 branch commits") removed merge commit `9e5a0171d` from the `commits:` frontmatter — regressing the round-2 process repair that `086b140f8` had just made (frontmatter there listed `51221dc02`, `9e5a0171d`, `5192ff750`; at head only the two non-merge commits remain). The Shipped "Review repair (finding 73a69995)" note still claims "merge commit 9e5a0171d is re-listed in commits: above" — **false at the assigned candidate**. The merge commit that resolved the `main.rs` import conflict is again outside verifier review scoping. Mitigating: `check-task-commit-attribution.sh` passes (the merge commit carries no task label) and the task-owned `main.rs` wiring exists in listed commit `51221dc02`, so no production change escapes scoping — but the shipped task file makes an inaccurate claim, and this is the exact defect class the prior round required repair for. Fix: re-add `9e5a0171d` to the frontmatter or correct the Shipped note.

Verdict: **not approved** (one process finding; the code implementation itself is real, spec-conformant, and regression-tested).
