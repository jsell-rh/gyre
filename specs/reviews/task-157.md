# Review — task-157 (Implement meta-spec rollout policies)

Spec: `specs/system/meta-spec-reconciliation.md` §7 Rollout Policy (lines 316–370). Candidate `6ed0fb0de39f1671945dfc82240522b22708d942`, base `637be7187617fcd00ccc75c70a6838bae9ad94eb`, branch `pipeline/task-157/03a0bca732d84781abc9f1cb5487d7fc-1`.
Verdict: **complete**.

## Spec-fidelity resolution (the task excerpt vs the live spec)

The task excerpt describes `immediate`/`rolling`/`manual` strategies with batch delays and repo-count auto-selection (≤10 repos → immediate). The live spec §7 — verified byte-for-byte at `specs/system/meta-spec-reconciliation.md:316-370` and **not modified by this work** (`git diff base..candidate -- specs/system/meta-spec-reconciliation.md` → 0 lines; last spec-touching commits are pre-branch merges `756e53a61`/`2e4218eaf`/`d2760bf4e`) — defines `Immediate`/`Rolling`/`Background` with `max_concurrent_agents`, `ReconciliationPriority` (High/Normal/Low), and `auto_merge`, defaulting to Rolling / Normal / auto_merge false, with no repo-count auto-selection. Per the goal rules ("specs are the contract; never implement around a spec by reinterpreting it"), following the live spec is correct; the excerpt's manual/delay/threshold semantics would be a spec-amendment request, not a code gap. The shipped domain types match the spec struct field-for-field (`crates/gyre-domain/src/meta_spec.rs`), and the spec default is pinned by `RolloutPolicy::default()` + the `unset_policy_is_the_spec_default` test.

## Test runs (this checkout, at candidate `6ed0fb0d`)

- `cargo test -p gyre-domain --lib meta_spec::` → **4 passed** (default-matches-spec, strategy/priority wire-name round-trips, priority→TaskPriority mapping, full-shape serialization).
- `cargo test -p gyre-adapters --lib rollout_policy` → **5 passed** (SQLite adapter on a real temp-file DB — the constructor runs the embedded migrations, so migration `2026-11-06-000057` is exercised — incl. a domain-type round-trip through the stored JSON).
- `cargo test -p gyre-server --lib reconciliation::tests` → **33 passed** (28 pre-existing + 5 repair regressions), run twice (pristine, and again after all mutations were reverted) — stable.
- `cargo test -p gyre-server --lib api::meta_specs` → **25 passed** (GET default, PUT round-trip read back through GET, partial-update field merge, Admin-only 403 via a real signed developer JWT, 400 on unknown strategy/priority and on `max_concurrent_agents: 0`, 404 on unknown workspace for both methods).
- Mechanical guards, all exit 0: `check-arch.sh`, `check-abac-route-registry.sh`, `check-mem-port-contracts.sh`, `check-mcp-write-tools.sh`, `check-migration-versions.sh`, `check-migration-sql-portability.sh`, `check-dead-message-kinds.sh`, `check-byte-slice-truncation.sh`, `check-relative-path-defaults.sh`, `check-fail-open-ref-resolution.sh`, `check-inert-enforcement.sh`, `check-in-memory-state-stores.sh`, `check-scope-literal-defaults.sh`, `check-fabricated-scope-defaults.sh`, `check-lossy-secret-conversion.sh`, `check-forwarded-header-trust.sh`, `check-forged-scope-fields.sh`, `check-unbounded-external-http.sh`.
- Migration numbering note (checked because the directory shows two `…-000057_` names): Diesel derives the version from everything before the first `_` with dashes removed — `20261008000057` ≠ `20261106000057` — so there is no version collision; `check-migration-versions.sh` confirms.

## Mutation probes (each reverted; `git status --short` clean and full suite re-passed afterwards)

- **A — disable policy dispatch** (force `strategy = Immediate` in `run_reconciliation`): `rolling…`, `background…` filter → **9 tests FAIL** (rolling batches, background inert, strategy-switch flush, background-switch flush, overlap, retrigger, kv-fail, batch-size change). The policy is genuinely load-bearing.
- **B — overwrite semantics in `merge_pending_rollout`** (discard the stored remainder, the pre-`de208c147` bug): → `overlapping_rolling_waves_retain_both_remainders` **FAILS** ("left: 3, right: 6" — the exact silent-work-loss the merge prevents). (`retriggered_rolling_wave_supersedes_not_duplicates` passing under this mutation is expected: a same-version re-trigger's candidate list covers the stored entries, so overwrite happens to be lossless there; the pair of tests covers both merge semantics.)
- **C — old stall gate** (count inert Backlog tasks as in-flight, the pre-`6b6cbbd8` bug): → `background_history_does_not_stall_rolling_rollout` **FAILS** ("left: 0, right: 1" — batch 2 never created, exactly the reviewer's reproduction point).

## Verified working (no findings)

- **Persistence is real and restart-safe.** `RolloutPolicyRepository` port + SQLite and Postgres diesel adapters (upsert on `workspace_id` PK) + mem adapter, wired through the `store!` macro in `build_state` — DB-backed in production, not an in-memory stand-in (satisfies the `check-in-memory-state-stores` invariant). Unset policy → spec default, not an error; corrupt row → default with a warning (the spec-mandated default is the safe configuration, not a fabricated scope identity).
- **Controller integration is complete.** `run_reconciliation` loads the policy and dispatches: Immediate creates all tasks with the Delegation signal; Rolling creates only `max_concurrent_agents` tasks and defers the rest into the port-backed kv store; Background creates all tasks **without** `task_type` so `spawn_agent_core`'s task-type gate keeps them inert in Backlog (the repo orchestrator decides engagement — the spec's exact wording). Task priority is `ReconciliationPriority::task_priority()` (High→High, Normal→Medium, Low→Low).
- **All three terminal surfaces share one hook.** `transition_task_status` (REST), `archive_repo` (REST), `handle_update_task` (MCP) now call `on_reconciliation_task_terminal`, which advances the rolling rollout **before** the §11 completion check — `ReconciliationCompleted` fires exactly once at the end of the whole rollout (asserted by count in the tests).
- **Rolling advance re-reads the live policy.** Mid-rollout strategy switch flushes the remainder at once (with delegation derived from the live policy — a Background switch flushes inert tasks, the fd31a1cb F1 repair), and a batch-size change applies to the next batch. The advance gate counts only reconciliation tasks carrying a `task_type` (the rollout's own in-flight work), so parked Background history cannot stall a later rollout (the c9dc5a11 repair).
- **Overlapping waves are lossless.** `merge_pending_rollout` merges (never overwrites) the deferred remainder; supersede-by-(repo,title) keeps re-triggers idempotent; a kv read failure aborts the merge with the stored state untouched (fail-closed, the fd31a1cb F2 repair) and already-created work deduplicates on the next advance.
- **auto_merge is a trigger, not a gate bypass.** `complete_agent` calls `maybe_auto_enqueue_reconciliation_mr` only for `RECONCILIATION_LABEL` tasks and only when the policy enables it: the MR is enqueued into the merge queue and gates are triggered; the merge processor still merges only after gate passage. Default false leaves the MR for a human/agent enqueue.
- **API authorization mirrors the sibling exactly.** `GET/PUT /api/v1/workspaces/:id/meta-specs/rollout-policy` registered in `api/mod.rs` and the ABAC resolver (`meta_spec` resource, no new exemptions); PUT additionally enforces the same handler-level Admin-role check + workspace-existence check as `put_meta_spec_set`, validates strategy/priority names, rejects batch 0, merges omitted fields with current values, and emits a `rollout_policy_updated` workspace event.

Findings:

- (none)

Minor observations (not blockers, no action required):

1. In `run_reconciliation`'s rolling branch, if `merge_pending_rollout` aborts on a kv read failure, that wave's deferred remainder is not persisted (warn-logged, stored state untouched, work re-triggerable) — a fail-safe tradeoff chosen over the risk of clobbering unreadable state.
2. A task-creation failure inside a first batch drops that candidate for the wave with only a warn log — identical to the pre-existing base behavior on the immediate path, not a regression.

Evidence: `/tmp/stage/review-evidence/task-157-review-6ed0fb0d.txt`.
