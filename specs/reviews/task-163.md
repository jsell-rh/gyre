# Review — task-163 (Dependency graph — persistent storage for breaking changes and policies)

Spec: `specs/system/dependency-graph.md` §Enforcement Policies + §Cascade Testing; task plan in `specs/tasks/task-163.md`.
Candidate under review: `370fbbb4` (base `e96d25ab`, assigned pair verified by `git rev-parse` at worktree HEAD).
Product-surface commit scoped by `commits:` frontmatter: `bb921746` (the only task-163-labeled commit touching `crates/`; the other four range commits — `54790eed`, `ec2de557`, `38783f63`, `370fbbb4` — touch only `specs/tasks/*.md` and `web/dist/` artifacts, so the attribution gate's product-surface filter correctly ignores them; verified per-commit with `git show --name-only --format=`).
Verdict: **approved** — no findings.

## What the task closes

The coverage matrix (`specs/coverage/system/dependency-graph.md` rows 11–12) recorded the defect precisely: `AppState` hardcoded `Arc::new(mem::MemBreakingChangeRepository)` / `MemDependencyPolicyRepository` at lib.rs:907-908, bypassing the `store!` macro every sibling repo uses — policies and breaking-change records were lost on restart. The candidate is the surgical fix plus the persistence layer behind it:

- **SQLite adapters** (`crates/gyre-adapters/src/sqlite/breaking_change.rs`, `dependency_policy.rs`): full Diesel implementations of the pre-existing ports (ports unchanged from base — verified `git diff` on `gyre-ports` is empty). Policy upsert uses `on_conflict(workspace_id).do_update()` matching every sibling upsert (`sqlite/agent.rs:147`, `attestation.rs:79`, …); unknown behavior strings are rejected (`str_to_behavior` → `Err`), not silently defaulted. `create` preserves pre-acknowledged fields (matters for merge-block tests that seed acknowledged records).
- **Postgres mirrors** (`postgres/{breaking_change,dependency_policy}.rs`): same queries, `check_for_backend(diesel::pg::Pg)`. PG adapters carry no unit tests anywhere in that directory (0 of 45 files) — consistent with repo convention, not a deviation.
- **Migration 000056**: next unused sequence after 000055; portable SQL (portability gate passes); `breaking_changes` and `dependency_policies` registered in `schema.rs` and in `sqlite/mod.rs::migrations_create_tables`.
- **AppState wiring** (lib.rs:910-917): both stores now through `store!` — SqliteStorage/PgStorage in DB-backed mode, mem fallback only in pure in-memory mode, identical to every sibling store. This is the exact cutover the coverage matrix demanded.

Cascade Testing (§Cascade Testing) was already code-verified on base — `trigger_cascade_tests` (merge_processor.rs:2210, invoked at :1740 after every merge) creates High-priority `cascade-test` labeled tasks in dependent repos, gated on the persistent policy, with per-dependent-workspace opt-out; `report_cascade_test_result` routes agent completion. The task's contribution there is that the policy reads (`get_for_workspace`) now hit durable storage, so an opt-out survives restart — the mem-store hole the matrix flagged ("Policy flag with no executor" is row 12's note; the executor existed on base and is exercised by 8 green tests).

## Test runs (this checkout at candidate HEAD)

- `cargo test -p gyre-adapters --lib breaking_change` → **7 passed** (roundtrip, missing→None, unacknowledged filter, acknowledge-missing→false, pre-acknowledged persistence, source-repo scoping, records survive a fresh storage instance).
- `cargo test -p gyre-adapters --lib dependency_policy` → **4 passed** (default policy for unset workspace, full-field roundtrip, overwrite, survives fresh storage instance).
- `cargo test -p gyre-adapters --lib migrations` → **3 passed** (incl. `migrations_create_tables` asserting both new tables exist, `migrations_pending_zero_after_startup`).
- `cargo test -p gyre-server --test task163_dependency_persistence` → **2 passed** — records/policies written via one `build_state` are observed by a second `build_state` over the same DB file.
- `cargo test -p gyre-server --lib merge_processor::tests::trigger_cascade` → **8 passed** (task per dependent, policy-disabled skip, no-dependents noop, workspace resolution, member notification, default-policy enable, dependent opt-out, tasks-for-dependents).
- `cargo test -p gyre-server --lib api::dependencies::tests` → **64 passed**, including the merge-policy enforcement tests that run the real merge processor (`run_once`) against the wired stores: Block → queue entry Failed with "unacknowledged breaking change", Warn → proceeds, proceed-after-acknowledgment, breaking-change auto-task creation.

## Mutation probe (decisive)

Reconstructed the exact base defect — replaced the two `store!` wirings with the mem literals — and re-ran the integration test: **both tests FAILED** (0 passed, 2 failed). The tests are anchored to the persistence contract, not to incidental behavior. Wiring restored; `git status --short` clean at `370fbbb4`; test re-run on restored wiring passes (2/2). Full source + exit codes in `/tmp/stage/review-evidence/07-mutation-probe-hollow-wiring.txt`.

## Mechanical gates (all exit 0)

`check-arch.sh`, `check-migration-versions.sh`, `check-migration-sql-portability.sh`, `check-mem-port-contracts.sh`, `check-in-memory-state-stores.sh`, `check-task-commit-attribution.sh`, `git diff --check e96d25ab..370fbbb4` (clean). `web/` is byte-identical to base (`git diff e96d25ab..370fbbb4 -- web/` empty — the accidental dist rebuild in the pipeline checkpoint was correctly reverted).

## Notes for the verifier

- Full `cargo test --all` is explicitly owned by verification (task's last unchecked box); this review ran the focused suites above per the review contract.
- TCP listener probes are unsupported in this sandbox (errno 95, recorded in `/tmp/stage/review-evidence/sandbox-transport-restriction.json`). Host-side check if desired: start with `GYRE_DATABASE_URL=sqlite://…`, set a Block policy via the policy endpoint, create a breaking change, restart the process, confirm both survive via `GET /api/v1/dependencies/breaking`. The wiring-level persistence contract is already covered by the integration test that rebuilds `AppState` over the same file, which is the same restart semantics minus the process boundary.
- `cargo test -p gyre-server` triggers the build.rs web rebuild side effect; the regenerated `web/dist` bundles were restored to committed state after probing (`git checkout -- web/dist/`), worktree verified clean.
- Sandbox note from the implementer about crates.io flakiness was reproduced here (transient DNS failure), resolved by `cargo fetch --locked`; all suites ran to completion at the true candidate HEAD.
