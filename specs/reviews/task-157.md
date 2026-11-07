# Review — task-157 (Implement meta-spec rollout policies)

Spec: `specs/system/meta-spec-reconciliation.md` §7 Rollout Policy.
Commits under review (per `commits:` frontmatter, on candidate `241de28c`, assigned base `7da7bbf`): `a1ff51f` (full implementation), `e67c467` (live batch-size re-read on rolling advance).
Verdict: **complete** — approved.

## Contract resolution (read first)

The task excerpt in `specs/tasks/task-157.md` describes `immediate`/`rolling`/`manual` strategies with configurable batch delay and repo-count-based auto-selection (≤10 repos → immediate). The live spec §7 defines **Immediate / Rolling / Background** with `max_concurrent_agents`, `ReconciliationPriority` (High/Normal/Low), and `auto_merge`, defaulting to Rolling / Normal / auto_merge false, with no repo-count auto-selection.

Verified: `git diff 7da7bbf..241de28c -- specs/system/` is **empty** — the candidate did not touch the spec, and the assigned base `7da7bbf` already contains §7 in the Immediate/Rolling/Background form. The excerpt is a stale decomposition artifact; the repo's authoritative goal rules ("Specs are the contract... never implement around a spec by reinterpreting it") resolve the conflict in favor of the live spec, which is what the implementation follows. The divergence is documented in the task's Shipped section with the acceptance checkboxes left unchecked and a pointer to spec-lifecycle amendment if the manual/auto-threshold semantics are still wanted. This is the correct handling, not a finding.

## Independent verification (this checkout at `241de28c`, tree clean before and after)

Test runs (output retained in `/tmp/stage/review-evidence/`):

- `cargo test -p gyre-domain meta_spec` → **6 passed** (spec default, wire-name round-trips, priority mapping, full JSON shape).
- `cargo test -p gyre-server --lib reconciliation::` → **28 passed**, including all §7 strategy tests (immediate-all-at-once, rolling batches + advance on completion, background inert-in-backlog, priority mapping, strategy-switch flush, batch-size change on next batch, auto_merge on/off/non-reconciliation, unset-policy default, ReconciliationCompleted fires exactly once at rollout end) plus the pre-existing §6/§10/§11 tests.
- `cargo test -p gyre-server --lib api::meta_specs` → **25 passed**, including GET default, PUT round-trip (read back through GET, not request echo), partial update, Developer-JWT 403, invalid strategy/priority/zero-batch 400, unknown workspace 404.

Mutation probes (temporary edits, restored and verified via `git status --porcelain` clean):

- **Strategy dispatch disabled** (`match policy.strategy` → `match RolloutStrategy::Immediate` in `run_reconciliation`): `rolling_strategy_creates_batches_and_advances_on_completion`, `rolling_strategy_switch_flushes_remaining_on_advance`, `rolling_batch_size_change_applies_to_next_batch`, and `reconciliation_creates_task_for_bound_repo` all **FAIL** (4 failed / 24 passed). The rolling tests are anchored to the real dispatch, not to incidental behavior.
- **Background delegation gate disabled** (`task.task_type = if delegation {...}` → unconditional `Some(Delegation)` in `create_reconciliation_task`): `background_strategy_leaves_tasks_inert_in_backlog` **FAILS** on "Background tasks must not carry the Delegation spawning signal".

Mechanical gates (all exit 0, output retained): `check-abac-route-registry.sh`, `check-migration-versions.sh`, `check-mem-port-contracts.sh`, `check-task-commit-attribution.sh`, `check-arch.sh`, `check-api-auth.sh`, `check-fabricated-scope-defaults.sh`, `check-in-memory-state-stores.sh`, `check-inert-enforcement.sh`, `check-mcp-write-tools.sh`, `check-dead-message-kinds.sh`, `check-migration-sql-portability.sh`. No `scripts/` or exemption-file changes in the diff.

## Spec fidelity (checked against §7 line by line)

- **Domain types** (`gyre-domain/src/meta_spec.rs`): `RolloutPolicy { strategy, max_concurrent_agents, priority, auto_merge }`, `RolloutStrategy` (Immediate/Rolling/Background), `ReconciliationPriority` (High/Normal/Low) — field-for-field the spec's declarations, doc comments carried over. Default impl = Rolling / Normal / auto_merge false exactly as §7 pins; `max_concurrent_agents` default 5 is the config freedom the spec leaves (it only pins strategy/priority/auto_merge). Priority maps onto `TaskPriority` (High→High, Normal→Medium, Low→Low) — the ordering lever the repo orchestrator actually consumes.
- **Immediate**: all candidates created at once with the Delegation `task_type` (§6 step-5 spawning signal); budget queuing is the pre-existing budget system's job, untouched.
- **Rolling**: first batch of `max_concurrent_agents` created at trigger; remainder persisted in the port-backed kv store (`rollout_pending` namespace, `KvJsonStore` — DB-backed in DB mode, so the restart-safety claim holds); `maybe_advance_rolling_rollout` creates the next batch only when the workspace has no open reconciliation task. Every advance re-reads the live policy (batch-size change applies to next batch; mid-rollout strategy switch flushes the remainder — both covered by tests). Advance runs inside the shared `on_reconciliation_task_terminal` hook before the §11 completion check, so `ReconciliationCompleted` fires exactly once at the end of the whole rollout (test-verified).
- **Background**: tasks created without `task_type`; the pre-existing `spawn_agent_core` gate (spawn.rs:366-386) rejects tasks without a task_type, so they sit in Backlog until the orchestrator engages them — verified by the mutation probe above.
- **auto_merge**: `maybe_auto_enqueue_reconciliation_mr` enqueues a reconciliation MR into the merge queue at agent completion (§6 step 6) and triggers gate execution; the merge processor's gates-pass requirement is untouched — only the merge *trigger* is automatic. Scoped to reconciliation-labeled tasks only (test-verified).
- **Persistence**: `RolloutPolicyRepository` port with real Diesel queries in both SQLite and Postgres adapters (upsert via `on_conflict ... do_update`), a mem adapter, migration `2026-11-06-000057_rollout_policies` (next unused sequence after 000056; portable SQL — `CREATE TABLE IF NOT EXISTS` runs on both backends), wired through the `store!` pattern so DB mode gets the real adapter.
- **API**: `GET/PUT /api/v1/workspaces/{id}/meta-specs/rollout-policy`. PUT is Admin-only in-handler (Developer JWT → 403, tested), validates strategy/priority names, rejects `max_concurrent_agents < 1`, merges omitted fields with current values, emits a `rollout_policy_updated` workspace event. Route registered in the ABAC resolver under the `meta_spec` resource — no exemption entries. GET returns stored policy or spec default. `docs/api-reference.md` updated.

## Notes (non-blocking, not findings)

- Two concurrent terminal transitions of batch tasks could in principle race in `maybe_advance_rolling_rollout` (both observe "no open tasks" and create the same next batch). This is the same unlocked-handler race class the pre-existing `maybe_emit_reconciliation_completed` callers had, is mitigated by the advance-loop dedup on the next advance, and has no security impact — noted for a future hardening pass, not a defect against this task's contract.
- If every first-batch task creation fails (DB-level errors), the persisted remainder waits for the next reconciliation trigger or terminal event to advance — pathological path only, and each failure is warned.

Findings:

- (none)

— Reviewer, 2026-11-06
