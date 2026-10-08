---
title: "Dep Graph — Wire persistent DependencyRepository into AppState"
spec_ref: "dependency-graph.md §Dependency Entity"
depends_on: []
progress: in-progress
coverage_sections:
  - "dependency-graph.md §Dependency Entity"
commits: ["2f312a4be7d66486f1231a1d12d4e962a8746000", "d532fe1b38f80cd7e5a2fc8f74b9da5968cdc76a"]
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

- [ ] `AppState.dependencies` is constructed via `store!(dyn DependencyRepository, …)`; no
      remaining `Arc::new(mem::MemDependencyRepository…)` literal in `build_state`.
- [ ] A new integration/persistence test proves the graph survives a restart: with
      `GYRE_DATABASE_URL` pointing at a temp SQLite file, `build_state`, `save()` a
      `DependencyEdge` through `state.dependencies`, drop that state, `build_state` **again
      on the same DB file**, and assert the edge is returned by the second instance
      (`list_for_source` / `find` / equivalent). This test MUST fail against the old
      `Arc::new(mem::MemDependencyRepository)` wiring (the second instance would see an
      empty store) and pass after the fix. Do not assert against the same in-process
      `Arc` — the test must exercise a genuinely fresh storage instance over the same file.
- [ ] Pure in-memory mode (no `GYRE_DATABASE_URL`) still works: `store!` falls back to
      `MemDependencyRepository`; existing dependency-graph tests keep passing.
- [ ] `cargo test --all` passes; `bash scripts/check-arch.sh` passes (no hexagonal
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
