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
      Verified: lib.rs:909-912 uses `store!`; the only remaining mem literal for this repo
      is the `#[cfg(test)]` `test_state_inner` builder, which is intentional pure-in-memory
      test state.
- [x] A new integration/persistence test proves the graph survives a restart:
      `crates/gyre-server/tests/dependency_persistence.rs` — three fresh `build_state`
      instances over the same SQLite file (save → restart-read → update → restart-read),
      asserting find_by_id/list_by_repo/list_dependents/list_all plus the update path
      (status→Stale, version_pinned). Mutation-probed fresh this attempt: with the old
      `Arc::new(mem::MemDependencyRepository)` literal restored in `build_state`, the test
      FAILS (panic at dependency_persistence.rs:70 "edge must survive restart on
      SQLite-backed state"; 0 passed; 1 failed; exit 101); with the `store!` wiring
      restored it passes (exit 0).
- [x] Pure in-memory mode still works: `cargo test -p gyre-server --lib api::dependencies`
      (64 passed) and `--lib dep_staleness` (8 passed) — all use `test_state()` mem state.
- [x] `bash scripts/check-arch.sh` passes ("Architecture lint passed", exit 0). Focused
      probes on this sandbox all exit 0; full `cargo test --all` is owned by the
      controller's verification gates (this sandbox's seccomp blocks the loopback-listener
      integration suites — `accept(): [Errno 95] Operation not supported`, recorded in
      /tmp/stage/capabilities.json).

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

`AppState.dependencies` is wired through the `store!` macro (crates/gyre-server/src/lib.rs:909-912),
exactly like every sibling repository: DB-backed deployments (`GYRE_DATABASE_URL` SQLite or
Postgres) now get `SqliteStorage`/`PgStorage` as the `DependencyRepository`, and pure
in-memory mode still falls back to `MemDependencyRepository`. The cross-repo dependency
graph — every `DependencyEdge` written by push-time detection, reconciliation, staleness
jobs, and the dependency API — is now durable across server restarts instead of being
lost with the process.

Actual behavior and evidence (all re-verified fresh this attempt on the retained source,
head 03c38d96 which contains the original task-199 commits; logs under
`/tmp/stage/review-evidence/`):

- Wiring: `store!(dyn DependencyRepository, mem::MemDependencyRepository::default())` at
  lib.rs:909-912; no `Arc::new(mem::MemDependencyRepository…)` literal remains in
  `build_state` — after the mutation probe the working tree was restored byte-identical to
  the committed fix (`git diff` on lib.rs empty).
- Persistence: `cargo test -p gyre-server --test dependency_persistence` passes — one test,
  `dependency_graph_survives_restart_on_sqlite`, round-trips an edge through three
  genuinely fresh `build_state` instances over one SQLite file (write → restart-read →
  update → restart-read), asserting field-level round-trip, list_by_repo,
  list_dependents, list_all, and the status/version_pinned update path.
- Mutation probe (test kills the defect): old `Arc::new(mem::…)` literal temporarily
  restored in `build_state` → test fails at the restart assertion (exit 101); `store!`
  wiring restored → passes (exit 0). Logs: task-199-mutation-probe.log,
  task-199-post-restore-persistence.log.
- Mem mode: `cargo test -p gyre-server --lib api::dependencies` (64 passed) and
  `--lib dep_staleness` (8 passed), exit 0.
- Hexagonal boundary: `bash scripts/check-arch.sh` → "Architecture lint passed", exit 0.

Out of scope, untouched: `breaking_changes` / `dependency_policies` persistence (task-163)
still use `Mem*` — per this task's contract. The build also ran the committed web/dist
via build.rs (npm ci + npm run build) successfully during `cargo test` compile.

Full-workspace `cargo test --all` and CI are owned by verification/publication; this
sandbox's seccomp denies `accept()` (Errno 95, /tmp/stage/capabilities.json), so
loopback-listener integration suites cannot run here — exact-head GitHub checks remain
required.
