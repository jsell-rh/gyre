---
title: "Dep Graph — Wire persistent DependencyRepository into AppState"
spec_ref: "dependency-graph.md §Dependency Entity"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "dependency-graph.md §Dependency Entity"
commits: ["10d5df6dafc0859c2f1360be65366d795f74ddd4", "02056fa0fe79474c325ec7cc91b8499680b0e333", "0248e9bf9c2d7b234fca8115d1412f70c4201596"]
---

## Spec Excerpt

From `dependency-graph.md` §Dependency Entity — the cross-repo dependency graph is
persistent forge state. Each `DependencyEdge` (source repo → target repo, type,
detection method, status, version pinning/drift) is a durable record: the graph, its
breaking-change history, and per-workspace policies MUST survive a server restart, the
same way tasks, agents, merge requests, and every other repository in the system persist
through the `store!`-selected SQLite/Postgres adapters.

## Problem (root cause)

The graph persistence is **hollow**. `AppState` construction hardcodes the in-memory
adapter for the dependency graph instead of the `store!` macro that every other
persistent repository uses:

`crates/gyre-server/src/lib.rs:872`
```rust
dependencies: Arc::new(mem::MemDependencyRepository::default()),
```

The real persistent adapters already exist and are complete:
- `impl DependencyRepository for SqliteStorage` — `crates/gyre-adapters/src/sqlite/dependency.rs:139`
- `impl DependencyRepository for PgStorage` — `crates/gyre-adapters/src/postgres/dependency.rs:139`
- `dependency_edges` table — migration `2026-03-23-000007_platform_entities/up.sql`, schema at `crates/gyre-adapters/src/schema.rs:419`

Because `AppState.dependencies` (`Arc<dyn DependencyRepository>`, lib.rs:255) is wired to
`MemDependencyRepository`, the persistent adapter is dead code at runtime: the entire
cross-repo dependency graph lives in memory only and is **lost on every restart**. All
push-time detection (`detect_dependencies_on_push`), reconciliation, blast-radius queries,
and the dependency API handlers write to / read from this volatile store.

## Scope

- **In scope:** wire `AppState.dependencies` (lib.rs:872) through the `store!` macro so a
  DB-backed deployment uses the SQLite/Postgres `DependencyRepository`, falling back to
  `MemDependencyRepository` only in pure in-memory mode (no `GYRE_DATABASE_URL`).
- **Out of scope:** `breaking_changes` (lib.rs:873) and `dependency_policies` (lib.rs:874)
  persistence — those port traits have **no** SQLite/Postgres adapter or migration yet and
  are owned by **task-163** (§Enforcement Policies, §Cascade Testing). Do not touch them
  here; do not stub adapters for them.

## Implementation Plan

1. In `crates/gyre-server/src/lib.rs`, replace the hardcoded line 872:
   ```rust
   dependencies: Arc::new(mem::MemDependencyRepository::default()),
   ```
   with the same `store!` pattern used by every sibling repository (e.g. lib.rs:823-831):
   ```rust
   dependencies: store!(dyn DependencyRepository, mem::MemDependencyRepository::default()),
   ```
   Confirm `DependencyRepository` is in scope at the macro call site (it is already used as
   the field type at lib.rs:255; add the `use` import only if the bare trait name does not
   resolve there).

2. Build the full workspace (`cargo build --all`) to confirm the `store!` type coercion to
   `Arc<dyn DependencyRepository>` compiles for both the SQLite and Postgres branches.

## Acceptance Criteria

- [x] `AppState.dependencies` is constructed via `store!(dyn DependencyRepository, …)`; no
      remaining `Arc::new(mem::MemDependencyRepository…)` literal in `build_state`.
- [x] A new integration/persistence test proves the graph survives a restart: with
      `GYRE_DATABASE_URL` pointing at a temp SQLite file, `build_state`, `save()` a
      `DependencyEdge` through `state.dependencies`, drop that state, `build_state` **again
      on the same DB file**, and assert the edge is returned by the second instance
      (`list_for_source` / `find` / equivalent). This test MUST fail against the old
      `Arc::new(mem::MemDependencyRepository)` wiring (the second instance would see an
      empty store) and pass after the fix. Do not assert against the same in-process
      `Arc` — the test must exercise a genuinely fresh storage instance over the same file.
- [x] Pure in-memory mode (no `GYRE_DATABASE_URL`) still works: `store!` falls back to
      `MemDependencyRepository`; existing dependency-graph tests keep passing.
- [x] `cargo test --all` passes; `bash scripts/check-arch.sh` passes (no hexagonal
      boundary violation introduced).

## Agent Instructions

1. Set `progress: in-progress` in the frontmatter.
2. Read `dependency-graph.md` §Dependency Entity and the coverage note in
   `specs/coverage/system/dependency-graph.md` row 4.
3. Read `crates/gyre-server/src/lib.rs:820-891` for the `store!` macro (802-812) and the
   surrounding `AppState` field wiring — mirror the sibling pattern exactly.
4. Read `crates/gyre-adapters/src/sqlite/dependency.rs` for the `DependencyRepository`
   method surface your test will call.
5. For the persistence test, follow the `GYRE_DATABASE_URL=sqlite://…` + `build_state`
   pattern (`build_state` at lib.rs:753; path parsing at 780-790). Use a `tempfile::TempDir`
   so the DB file is deterministic across both `build_state` calls, as
   `tests/git_integration.rs` does.
6. This is the sole remaining `not-started` section repo-wide — do NOT expand scope into
   task-163's breaking-change/policy persistence. Wire the dependency graph, prove
   persistence, stop.
7. Run `cargo test --all` and `bash scripts/check-arch.sh`; record the fix commit SHA in
   the `commits` frontmatter list and set `progress: ready-for-review`.

## Shipped

`AppState.dependencies` is wired through the `store!` macro
(crates/gyre-server/src/lib.rs:909-912), exactly like every sibling repository: DB-backed
deployments (`GYRE_DATABASE_URL` SQLite or Postgres) get `SqliteStorage`/`PgStorage` as
the `DependencyRepository`; pure in-memory mode still falls back to
`MemDependencyRepository`. The cross-repo dependency graph — every `DependencyEdge`
written by push-time detection, reconciliation, staleness jobs, and the dependency API —
is durable across server restarts instead of dying with the process.

Evidence for this repair attempt (contract finding
`945c3524c43d4f76ad68c5533a1a61ce`; logs under `/tmp/stage/review-evidence/`):

- Wiring: `store!(dyn DependencyRepository, mem::MemDependencyRepository::default())` at
  lib.rs:909-912. No `Arc::new(mem::MemDependencyRepository…)` literal remains in
  `build_state`; the only remaining one in the tree is the intentional `#[cfg(test)]`
  `test_state_inner` builder. `breaking_changes` (lib.rs:913) and `dependency_policies`
  (lib.rs:914) untouched per scope — owned by task-163.
- Persistence: `cargo test -p gyre-server --test dependency_persistence` → 1 passed;
  0 failed; exit 0 (`task-199-good-persistence.log`). The test round-trips an edge
  through three genuinely fresh `build_state` instances over one SQLite file
  (save → restart-read → update → restart-read), asserting field-level round-trip,
  list_by_repo, list_dependents, list_all, and the status/version_pinned update path.
- Mutation probe (test kills the defect): in an isolated worktree, the old
  `Arc::new(mem::MemDependencyRepository::default())` literal was restored in
  `build_state` → the test fails at the restart assertion
  (dependency_persistence.rs:70 "edge must survive restart on SQLite-backed state";
  0 passed; 1 failed). `store!` wiring restored → passes (`task-199-mutation-probe.log`).
- Pure in-memory mode: `cargo test -p gyre-server --lib -- api::dependencies
  dep_staleness` → 72 passed; 0 failed; exit 0 (`task-199-mem-mode.log`).
- Hexagonal boundary: `bash scripts/check-arch.sh` → "Architecture lint passed", exit 0
  (`task-199-arch-lint.log`).
- Attribution: `bash scripts/check-task-commit-attribution.sh` previously failed with the
  empty `commits:` list (0248e9bf/02056fa0/10d5df6d unlisted); the frontmatter now lists
  the three product-surface fix commits.

Full-workspace `cargo test --all` and CI are owned by verification/publication; this
sandbox's seccomp denies `accept()` (Errno 95, `/tmp/stage/capabilities.json`), so
loopback-listener integration suites cannot run here — exact-head GitHub checks remain
required, no code defect inferred from that restriction.
