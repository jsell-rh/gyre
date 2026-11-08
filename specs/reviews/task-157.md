# Review — task-157 (Implement meta-spec rollout policies, meta-spec-reconciliation.md §7)

Candidate: `c2904d4f68702563024dc636a6ca13eb04b5ea4a` (branch `pipeline/task-157/fd31a1cb34744c95b36158b24d447921-1`), assigned base `124a852bedb4e5759bf143b6c2683df6eeb03849`, commits `a1ff51f2`, `e67c4671`, `aa73b69a`, `de208c14` (+ process/merge commits).
Verdict: **needs revision** (2 findings, one confirmed by an executable probe).

## Spec-contract scoping (verified)

The task excerpt (immediate/rolling/manual, batch delay, repo-count auto-selection) diverges from the live spec. Verified `git diff 124a852be..c2904d4f6 -- specs/system/` is empty — the candidate did **not** modify the spec to match its implementation. Live §7 defines Immediate/Rolling/Background with `max_concurrent_agents`, `ReconciliationPriority`, `auto_merge`, default Rolling/Normal/auto_merge false. Following the live spec per the goal rules ("specs are the contract") is correct; the coverage-matrix update (`specs/coverage/system/meta-spec-reconciliation.md` §7 → implemented) accurately describes the shipped behavior. No spec fakery.

## Evidence (all at candidate `c2904d4f6`, this checkout)

- `cargo test -p gyre-server --lib reconciliation::tests` → **30 passed** (immediate/rolling/background, priority mapping, strategy-switch flush, batch-size change, overlapping waves, re-trigger supersede, auto_merge on/off/ignore, default policy, completion-exactly-once).
- `cargo test -p gyre-server --lib api::meta_specs` → **25 passed** (GET default, PUT round-trip, partial update, Admin-only 403 for Developer JWT, invalid strategy/priority/batch-0 → 400, unknown workspace → 404).
- `cargo test -p gyre-domain meta_spec` → **6 passed**; `cargo test -p gyre-adapters rollout_policy` → **5 passed** (SQLite adapter round-trip/overwrite/delete).
- Mutation probes (source restored to pristine after each; `git status --short` clean):
  - batch_size → `usize::MAX` in `run_reconciliation` → 3 rolling tests **FAIL** (batching is really tested).
  - remove `maybe_advance_rolling_rollout` from the terminal hook → `rolling_strategy_creates_batches_and_advances_on_completion` **FAILS**.
  - revert `merge_pending_rollout` to plain overwrite → `overlapping_rolling_waves_retain_both_remainders` **FAILS** (the de208c14 repair is genuinely guarded).
  - remove the `auto_merge` gate → `auto_merge_disabled_leaves_mr_unenqueued` **FAILS**.
- Guard scripts: `check-arch.sh`, `check-migration-versions.sh`, `check-abac-route-registry.sh`, `check-mcp-write-tools.sh`, `check-in-memory-state-stores.sh`, `check-migration-sql-portability.sh` → all OK; no `scripts/` changes in the diff (no exemption growth).
- Wiring verified by reading: `on_reconciliation_task_terminal` called from all three terminal surfaces (REST task update `api/tasks.rs:355`, REST repo archive `api/repos.rs:291`, MCP `update_task` `mcp.rs:1067`); `maybe_auto_enqueue_reconciliation_mr` from agent completion (`api/spawn.rs:1345`); route registered in `api/mod.rs` and the ABAC resolver (same `meta_spec` mapping as the sibling meta-spec-set route; PUT additionally Admin-gated in-handler, proven by the 403 test); `store!` wiring for the new port in `lib.rs` + mem adapter; SQLite and Postgres adapters both real Diesel implementations; migration `2026-11-06-000057_rollout_policies` uses portable SQL with a unique Diesel version prefix.

## Findings

### F1 — Background strategy switch mid-rollout flushes agent-spawning tasks (code, confirmed)

`crates/gyre-server/src/reconciliation.rs`, `maybe_advance_rolling_rollout`: the flush path (policy re-read shows a non-Rolling strategy → `batch_size = usize::MAX`) calls `create_reconciliation_task(..., /* delegation */ true, ...)` unconditionally. A mid-rollout switch to **Background** therefore creates the deferred remainder with `task_type: Some(Delegation)`, so those tasks can trigger agent spawning (`spawn_agent_core` task-type gate passes) — contradicting §7 Background ("Create tasks but leave them in Backlog... Repo orchestrator decides priority") and inconsistent with `run_reconciliation`'s own Background branch, which correctly passes `delegation = false`. The shipped notes explicitly advertise mid-rollout strategy switches ("a mid-rollout strategy switch flushes the remainder at once"), and the switch test covers only Rolling→Immediate, where `true` happens to be right.

Reproduction (probe test, evidence `/tmp/stage/review-evidence/task-157-probe-background-switch.txt`): rolling policy batch 1 of 3 → switch policy to Background → complete batch 1 → `on_reconciliation_task_terminal` → flushed tasks assert `task_type == Some(Delegation)`; spec requires `None`. Fix: derive the delegation flag from the re-read policy's strategy (`policy.strategy == RolloutStrategy::Immediate`) in the advance, and extend the switch test to cover Background.

### F2 — `merge_pending_rollout` fails open on a pending-state read error (code, moderate-low)

`crates/gyre-server/src/reconciliation.rs`, `merge_pending_rollout`, `kv_get` `Err` arm: it warns "keeping only the incoming remainder" and then `kv_set`s the incoming-only list, **overwriting** the stored deferred remainder. This is the exact silent-work-loss class the `de208c14` repair fixed (deferred repos left unreconciled, no error), reachable behind a transient read failure that does not affect the subsequent write (e.g. a busy-timeout read followed by a successful write on the same pool). A read failure should abort the merge (leave the stored value untouched, return its count or an error) rather than overwrite; "cannot determine state" must not become "state is empty" — the same fail-closed principle the repo enforces for `resolve_ref`. The corrupt-JSON arm is defensible (data already lost), but the transient-`Err` arm is not.

## Observations (non-blocking, recorded for the verifier)

- **O1 (TOCTOU in the advance):** the open-task check and batch creation in `maybe_advance_rolling_rollout` are not atomic; two concurrent terminal transitions (two agents finishing the last batch tasks near-simultaneously) can both pass the check and double-create the next batch. Same check-then-act class as the pre-existing §11 emit path; narrow window, and later dedup suppresses duplicates in subsequent waves.
- **O2 (no startup sweep):** nothing scans `ROLLOUT_PENDING_NS` at startup. A crash in the window between the last batch task's terminal persistence and the advance leaves the deferred remainder stalled until the next reconciliation terminal event or a new wave (persistence is restart-safe; resumption is event-driven).

## Verified working (no findings)

- Domain types match live §7 exactly (strategies, priority enum, default Rolling/Normal/auto_merge false; `DEFAULT_MAX_CONCURRENT_AGENTS = 5` is a documented config choice the spec leaves open).
- Persistence is real on all three adapters; mem adapter guards the same contract in code; port has no unenforced "Fails if" constraints.
- Immediate creates all tasks at once (mutation-verified); Rolling batches and advances on terminal status across all three terminal surfaces; Background (at trigger time) creates tasks without `task_type` so they cannot spawn agents; priority maps onto `TaskPriority`; auto_merge enqueues reconciliation MRs into the merge queue at agent completion (gate passage still owned by the merge processor); `ReconciliationCompleted` fires exactly once at rollout end.
- API: GET returns stored-or-default; PUT is Admin-only with strategy/priority validation, `max_concurrent_agents >= 1`, partial-update merge, audit event, ABAC resolver registration; docs updated.
- Tests are hard (state/pending-shape/event-count assertions, no mirrored logic); each core behavior mutation-verified to fail its test.

— Reviewer, independent review of attempt `fd31a1cb…-1`
