---
title: "Dep Graph — Wire persistent DependencyRepository into AppState"
spec_ref: "dependency-graph.md §Dependency Entity"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "dependency-graph.md §Dependency Entity"
commits: ["0248e9bf9c2d7b234fca8115d1412f70c4201596", "02056fa0fe79474c325ec7cc91b8499680b0e333", "10d5df6dafc0859c2f1360be65366d795f74ddd4"]
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
      Verified: lib.rs:909-912 uses `store!`; the only remaining mem literal is the
      `#[cfg(test)]` `test_state_inner` builder (mem.rs:3289), which is intentional
      pure-in-memory test state.
- [x] A new integration/persistence test proves the graph survives a restart:
      `crates/gyre-server/tests/dependency_persistence.rs` — three fresh `build_state`
      instances over the same SQLite file (save → restart-read → update → restart-read),
      asserting find_by_id/list_by_repo/list_dependents/list_all plus the update path
      (status→Stale, version_pinned). Mutation-probed: with the old
      `Arc::new(mem::MemDependencyRepository)` literal restored in `build_state`, the test
      FAILS (`dependency_graph_survives_restart_on_sqlite` … 0 passed; 1 failed); with the
      `store!` wiring it passes.
- [x] Pure in-memory mode still works: `cargo test -p gyre-server --lib api::dependencies`
      (64 passed) and `--lib dep_staleness` (8 passed) — all use `test_state()` mem state.
- [x] `bash scripts/check-arch.sh` passes ("Architecture lint passed"). Focused runs on
      this sandbox: `cargo build -p gyre-server` (SKIP_WEB_BUILD=1) and
      `cargo test -p gyre-server --test dependency_persistence` pass; full
      `cargo test --all` deferred to the controller's gates (sandbox cannot run the
      loopback-listener integration suites).

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
