# Review — task-199 (Dep Graph — Wire persistent DependencyRepository into AppState)

Spec: `specs/system/dependency-graph.md` §Dependency Entity (cross-repo dependency graph is persistent forge state; the graph MUST survive a server restart through the `store!`-selected SQLite/Postgres adapters, like every other repository).
Comparison base: `66422bd4b99de70536cce8422ec23db5eaec082d` → HEAD `dff6ee4cc2d41cd32f229a7dc83bf448536c54e3`.
Diff: exactly 3 files — `crates/gyre-server/src/lib.rs` (+5/−1), `crates/gyre-server/tests/dependency_persistence.rs` (new, 120 lines), `specs/tasks/task-199.md` (status flip). No uncommitted repairs (working tree clean).

Verdict: **complete**.

## Round 1

All probes run in a dedicated git worktree (`/tmp/task199-review-wt`, detached at `dff6ee4`) with its own `CARGO_TARGET_DIR=/tmp/task199-target`, good and mutant builds sequential in the same lane. Evidence persisted under `/tmp/stage/review-evidence/task-199/` (command, revision, output, exit status each).

### Diff inspection

- **Wiring is the spec fix, nothing more.** `build_state` now constructs `dependencies: store!(dyn DependencyRepository, mem::MemDependencyRepository::default())` (lib.rs:909-912) — byte-for-byte the sibling pattern used by `repos`, `tasks`, `merge_requests`, etc. The `store!` macro (lib.rs:836-846) prefers `pg_db`, then `sqlite_db`, then the mem fallback, so a DB-backed deployment gets the persistent adapter and pure in-memory mode (no `GYRE_DATABASE_URL`) keeps `MemDependencyRepository`. `DependencyRepository` is already imported at lib.rs:68.
- **Scope discipline held.** `breaking_changes` (lib.rs:913) and `dependency_policies` (lib.rs:914) remain `Arc::new(mem::…)` — untouched, owned by task-163 (`not-started`, separate frontmatter verified). The only remaining `Arc::new(MemDependencyRepository…)` in the tree is the `#[cfg(test)]` `test_state_inner` builder (mem.rs:3289), which is the intentional pure-in-memory test state, not production wiring.
- **Adapter/table pre-existed at base** (last touched in commits that are ancestors of `66422bd4`): `impl DependencyRepository for SqliteStorage` (sqlite/dependency.rs:139) and `dependency_edges` migration + `target_version_current` (migration 000049). The task did not fake or touch adapters — correct, they were already real and dead at runtime.
- **Every production consumer flows through the wired field**: push-time detection + reconciliation (git_http.rs:2122-2215, 2637-2695), breaking-change blast radius (git_http.rs:2836, 2978), staleness pass (dep_staleness.rs:39, 209), REST handlers (api/dependencies.rs:585-588). `SqliteStorage::new` runs `run_pending_migrations` on open (sqlite/mod.rs:124), so each fresh `build_state` over an existing file is a faithful restart simulation. The SQLite `save` is a real upsert (`on_conflict(id).do_update()`), so the update path the test exercises is genuine SQL, not insert-only.
- **Test is a hard test, not mirrored logic.** Three genuinely fresh `build_state` instances over the same SQLite file (fresh `SqliteStorage` pool + migrations each time — a restart, not a reused handle): save → restart-read (find_by_id with full field round-trip incl. `detected_at`, list_by_repo, list_dependents, list_all) → update (status→Stale, version_pinned) → restart-read of the update. `GYRE_DATABASE_URL` is set inside the test binary's own process (single `#[tokio::test]` in the file, own test binary — no cross-binary env bleed, matching the git_integration.rs pattern). Fresh `TempDir` per run, so `list_all`'s `any` check cannot be satisfied by leftovers.

### Probes (all at `dff6ee4`, isolated worktree, private target dir)

- **Good run**: `SKIP_WEB_BUILD=1 cargo test -p gyre-server --test dependency_persistence` → **1 passed; 0 failed; exit 0**.
- **Mutation probe (independent, not the implementer's claim)**: reverted the field to `Arc::new(mem::MemDependencyRepository::default())` in the worktree only → **FAILED** at tests/dependency_persistence.rs:70 "edge must survive restart on SQLite-backed state" — 0 passed; 1 failed; exit 101. The test kills the exact regression this task fixes. Source restored byte-identical (`diff -q` clean; worktree `git status` clean at `dff6ee4`).
- **Mem-mode regression**: `cargo test -p gyre-server --lib -- api::dependencies dep_staleness` → **72 passed; 0 failed; exit 0** (64 + 8, matching the AC). The mem fallback path is not broken by the wiring.
- **Architecture lint**: `bash scripts/check-arch.sh` → "Architecture lint passed", exit 0.
- **Attribution gate**: `bash scripts/check-task-commit-attribution.sh` → OK. The three listed commits (`10d5df6`, `02056fa`, `0248e9b`) are the product-surface commits (verified via `git show --stat`); the `process:`/`docs:` commits are lifecycle bookkeeping, which the gate distinguishes.

### Minor observations (not blocking, recorded for completeness)

- `dependency_edges` has no tenant column, so the SQLite/Pg adapters are tenant-wide — identical semantics to the mem adapter they replace and to the port contract itself ("All edges in the graph (tenant-wide)", gyre-ports/src/dependency.rs:24). No behavior change from wiring; any tenant-scoping change is a port-contract redesign, out of scope.
- Full `cargo test --all` deferred to the controller's integration gates — consistent with the sandbox constraint (no loopback listeners). The changed code path is covered by the focused suites above.
- The three `wip(task-199): preserve sandbox attempt` commits are progressive snapshots of the same change across sandbox attempts; the net diff vs base is the reviewed surface and contains no stray intermediate state.

### Coverage section closure

`dependency-graph.md` §Dependency Entity (coverage row 4, the last `task-assigned` row in that file): the specced behavior — the dependency graph surviving a restart via the `store!`-selected adapters — is now enforced in production wiring and pinned by a mutation-proven restart test. Row 4 can be promoted from `task-assigned` to `verified` (controller's auditor call; the `breaking_changes`/`dependency_policies` persistence portion remains open under task-163, per the task's own scope note).
