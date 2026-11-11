# Review — task-163 (Dependency graph — persistent storage for breaking changes and policies)

Spec: `specs/system/dependency-graph.md` §Enforcement Policies, §Cascade Testing.
Candidate: `08069512f0a8fc811c3ee4ce33781708bc36de15` (base `cba416b46a73d3568f745d7b4eae49a0700a90a6`).
Product-surface commits: `c1585054`, `c5864610`, `0777605e`, `202c9a2d`, `38e4ae86`, `a5417644` (dist only), plus implement checkpoints.
Verdict: **approved** — no findings. Evidence under `/tmp/stage/review-evidence/` (see INDEX.md).

## What was reviewed

Net diff base→candidate (13 files): SQLite + Postgres adapters for the two ports, `schema.rs` table registrations, migration `2026-10-08-000061_dep_breaking_policies` (up/down), `AppState` `store!` rewiring in `gyre-server/src/lib.rs`, a SCIM test race fix, and the integration test `crates/gyre-server/tests/task163_dependency_persistence.rs`.

## Verification per acceptance criterion

1. **`BreakingChangeRepository` port + SQLite adapter — met.** The port pre-existed on this task's base (`gyre-ports/src/breaking_change.rs`: create / find_by_id / list_unacknowledged / list_by_source_repo / acknowledge→bool); the plan's `list_by_dependency(edge_id)`/tenant-scoped `list_unacknowledged` signatures would have broken existing base callers, and the shipped note documents that choice honestly. `crates/gyre-adapters/src/sqlite/breaking_change.rs` is a real Diesel implementation (spawn_blocking over the r2d2 pool, real queries, indexes used), with a Postgres mirror. Focused suite: 7/7 passed, including `records_survive_fresh_storage_instance` — a fresh `SqliteStorage` over the same DB file observes prior writes, which kills any in-memory stand-in.
2. **`DependencyPolicyRepository` port + SQLite adapter — met.** Real upsert keyed on `workspace_id` (PK), behavior string↔enum mapping that errors on unknown values, all five policy fields round-tripped. 4/4 focused tests passed, including fresh-instance persistence and unset-workspace→domain-default parity (`Warn`/3/30/true/true matches `DependencyPolicy::default()` and the migration column defaults).
3. **Migration — met.** `breaking_changes` (id PK, edge/source-repo/unacknowledged indexes, acknowledged fields) and `dependency_policies` (workspace_id PK, five policy columns, updated_at). Portable SQL (`CREATE TABLE/INDEX IF NOT EXISTS`, BIGINT/INTEGER) — `check-migration-sql-portability.sh` exit 0; `migrations_create_tables` (3/3 passed) asserts both tables exist after the real embedded migrations run on a fresh DB. Numbering `000061` is the next unused sequence; `check-migration-versions.sh` exit 0.
4. **In-memory adapters replaced in AppState — met.** `lib.rs` rewires `breaking_changes`/`dependency_policies` from `Arc::new(mem…)` to the `store!` macro — SqliteStorage/PgStorage in DB-backed mode, mem only in pure in-memory mode, identical to every sibling store. The integration test `task163_dependency_persistence.rs` builds two `AppState`s over the same SQLite file and asserts cross-rebuild visibility of records, acknowledgments, and policies — it fails against mem-only wiring. (Execution deferred to the verifier per the 4 GiB build rule; `cargo check -p gyre-server --tests` exit 0 confirms it compiles.)
5. **Cascade test tasks auto-created when `require_cascade_tests` is true — met.** `trigger_cascade_tests` (merge_processor.rs, pre-existing on base, invoked at line 2613 on every merge) reads the policy through `state.dependency_policies` — now persistent — honors the dependent workspace's opt-out, and creates a High-priority `cascade-test`/`auto-created` labeled task per dependent repo with the breaking-change context, plus `cascade_test_triggered` events. The task's scope ("this task just ensures the tasks are created") is satisfied; the executor existed on base and is unchanged except that it now reads durable policy state.
6. **`cargo test --all` — deferred to the independent verifier** (hard 4 GiB budget rule). Verifier must run at minimum: `cargo test -p gyre-server --test task163_dependency_persistence`, `cargo test -p gyre-server --lib merge_processor`, `api::dependencies`, `api::scim`, then the workspace suite.

## Round-12 migration renumber repair — verified against diesel source

The base merge introduced `2026-10-08-000060_task_generation`, colliding with this task's migration. Verified in vendored diesel_migrations 2.3.1: `version_from_string` normalizes both dirs to `20261008000060`, and `pending_migrations` collects into a `HashMap<version, _>` — one of a colliding pair is silently dropped. The renumber to `000061` (byte-preserving `git mv`) fixes it; the idempotent up.sql makes re-application on DBs that already ran the old version a no-op. This closes the round-11 verification finding.

## Static gates re-run at candidate head

migration-versions, migration-sql-portability, arch, mem-port-contracts, in-memory-state-stores, byte-slice-truncation, scope-literal-defaults, inert-enforcement, unbounded-external-http — all exit 0 (logs in review-evidence).

## Test-inflation check — none found

The persistence tests assert survival across fresh storage instances / AppState rebuilds (fail against mem stand-ins); `migrations_create_tables` runs the real embedded migrations; the policy roundtrip asserts every field plus neighbor-workspace isolation; acknowledge-missing→false and pre-acknowledged-field preservation are genuine contract probes. The SCIM change (`38e4ae86`) fixes a real process-global env race (`scim_unauthorized_without_token` wrote a divergent token while parallel tests use `test-scim-token`) — test-only, in service of criterion 6.

## Non-blocking observations (recorded, no action required)

- Stale comment in `crates/gyre-adapters/src/sqlite/mod.rs` (~line 275): "(migration 000060, task-163)" should read 000061 after the renumber. Cosmetic.
- `check-task-commit-attribution.sh` fails against the working tree only because the executor reset `specs/tasks/task-163.md` to the pristine assigned contract before review; at the candidate commit all product-surface commits are listed in the frontmatter.
- Generated `web/dist/` was reset to base artifacts (`a5417644`); no task-163 UI source changes exist — dist ownership belongs to the deterministic verifier per assignment rules.
