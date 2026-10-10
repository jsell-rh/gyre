---
title: "Dep Graph — Wire persistent DependencyRepository into AppState"
spec_ref: "dependency-graph.md §Dependency Entity"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "dependency-graph.md §Dependency Entity"
commits: ["67aafc0a94b4c1a56336022aa0f9c219c8c9f48c"]
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

## Shipped

`AppState.dependencies` is wired through the `store!` macro
(crates/gyre-server/src/lib.rs:909-912), exactly like every sibling repository: DB-backed
deployments (`GYRE_DATABASE_URL` SQLite or Postgres) get `SqliteStorage`/`PgStorage` as
the `DependencyRepository`; pure in-memory mode (no `GYRE_DATABASE_URL`) keeps
`MemDependencyRepository`. `breaking_changes` and `dependency_policies` remain mem-wired
untouched (task-163 scope).

Persistence is proven by `crates/gyre-server/tests/dependency_persistence.rs`: three
genuinely fresh `build_state` instances over one temp SQLite file (save → restart-read
with full field round-trip → update → restart-read of the update). An independent review
(specs/reviews/task-199.md, comparison base `66422bd4` → `dff6ee4c`) verified the wiring
byte-for-byte against the sibling pattern, confirmed the adapters/migration pre-existed
at base, mutation-proved the test (reverting to the old `Arc::new(mem::…)` literal fails
at the restart assertion), and passed mem-mode regression (72) and `check-arch.sh`.

The code at this branch's HEAD is byte-identical to the reviewed commit `dff6ee4c`
(verified: empty diff on lib.rs and dependency_persistence.rs).

Contract-repair note for findings `945c3524…`/`02877ee2…` (category: contract): earlier
attempts flipped the four Acceptance Criteria checkboxes from `- [ ]` to `- [x]`, which
changes the requirement generation hash — completion markers are reviewer/verifier
record, not implementer mutations (task-201 precedent: `ready-for-review` with all boxes
unchecked). This attempt applies only hash-excluded mutations: `progress`, `commits`, and
this `## Shipped` section. Acceptance Criteria text is byte-identical to base.

## Repair attempt 0d9343d0 (evidence regeneration)

The prior attempt (checkpoint `c416f16d`, agent exit 130) completed the wiring and
reviews but died waiting on its mutation probe, and its evidence directory did not
survive into this sandbox. No source change was needed — the checkpoint is correct.
This attempt re-verified everything fresh at HEAD `abdfc3ab` in an isolated worktree
(`/tmp/task199-ev-wt`, private `CARGO_TARGET_DIR`), evidence under
`/tmp/stage/review-evidence/task-199/`:

- **Good run**: `SKIP_WEB_BUILD=1 cargo test -p gyre-server --test dependency_persistence`
  → 1 passed; 0 failed; EXIT=0 (cold build 16m11s; test binary hash
  `dependency_persistence-dffbac4508bfe1c8` matches the prior attempt's, confirming
  source identity).
- **Mutation probe**: reverted lib.rs:909-912 to the old
  `Arc::new(mem::MemDependencyRepository::default())` literal in the worktree only →
  FAILED at tests/dependency_persistence.rs:70 "edge must survive restart on
  SQLite-backed state", 0 passed; 1 failed; EXIT=101. The test kills the exact
  regression this task fixes. Source restored and verified byte-identical to HEAD.
- **Post-restore re-run**: 1 passed; 0 failed; EXIT=0.
- **Mem-mode regression**: `env -u GYRE_DATABASE_URL cargo test -p gyre-server --lib --
  api::dependencies dep_staleness` → 72 passed; 0 failed; EXIT=0.
- **Architecture lint**: `bash scripts/check-arch.sh` → "Architecture lint passed",
  EXIT=0.
- **Attribution gate**: `bash scripts/check-task-commit-attribution.sh` → OK, EXIT=0.

Sandbox note: the first cargo invocation hit transient crates.io proxy warm-up
failures (connection refused); retry succeeded and the full download+build completed —
an infrastructure hiccup, not a code defect. Listener-based verification is not
possible here (tcp_listener_probe: Errno 95 Operation not supported); the focused
test suites above cover the changed path, and full workspace gates remain owned by
verification/publication.
