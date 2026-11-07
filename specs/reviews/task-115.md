# Review: TASK-115 — Spec approval signal chain & orchestrators

**Reviewer:** Independent review (round 4)
**Date:** 2026-11-06 (R4)
**Verdict:** needs-revision

**Candidate under review:** `e925756456b3ce9d04ee204e1df80b4cd5f75dbf` (base `88bd81bf`)
**Spec:** agent-runtime.md §1 Phases 1–3 (signal chain, workspace/repo orchestrators, task scheduler)

**Verification performed:** read the full base→candidate diff (signal_chain.rs in full — 2237 lines; api/specs.rs approval emission + on_spec_approved wiring; api/orchestrator.rs spawn cores + stale-detector refresh; jobs.rs task_scheduler registration; main.rs loop spawn; sqlite/postgres task adapters; domain Task.spec_ref; mcp.rs tool surface; migration 2026-10-10-000056_task_spec_ref; exemption-file changes — both counts lowered, the sanctioned direction, with the fixed sites verified real: specs.rs tenant resolution now skips+logs, rig_vertexai clients carry `.timeout(...)` and the remaining `Client::new()` is `#[cfg(test)]`). Ran focused probes on the clean candidate checkout via /tmp/stage/bin/cargo: `cargo test -p gyre-adapters --lib sqlite::` (258 passed, **7 failed**), `cargo test -p gyre-server --lib signal_chain` (19 passed, 0 failed), `bash scripts/check-migration-versions.sh` (**FAIL**, exit 1), plus check-arch / check-migration-sql-portability / check-dead-message-kinds / check-task-commit-attribution (all OK). Base comparison in a clean worktree of `88bd81bf` (removed after). Evidence: `/tmp/stage/review-evidence/task-115-round4-evidence.txt` and sibling files.

---

## Findings

- [-] **F1 (critical, candidate-introduced regression vs base, commit `336dd201`): duplicate Diesel migration version `20261010000056` — the new `tasks.spec_ref` migration shadows the base's `meta_spec_approved_content_hash` migration, and only one of them runs.** The candidate adds `crates/gyre-adapters/migrations/2026-10-10-000056_task_spec_ref/` while the base already ships `2026-10-10-000056_meta_spec_approved_content_hash/`; Diesel derives an identical version string (`20261010000056`) for both, so only one migration executes on a fresh database. Empirically, on the candidate tree the winner is `task_spec_ref`: a fresh SQLite DB gets `tasks.spec_ref` but **not** `meta_specs.approved_content_hash`, so all six pre-existing `sqlite::meta_spec::tests` fail with `table meta_specs has no column named approved_content_hash` and the repo's own guard `sqlite::tests::no_duplicate_migration_version_prefixes` panics on the duplicate pair (`cargo test -p gyre-adapters --lib sqlite::` → 258 passed, 7 failed). `scripts/check-migration-versions.sh` exits 1 on the candidate and passes on the base. This is the exact failure class AGENTS.md bans ("Every new migration MUST use the next unused 6-digit sequence number … duplicate versions silently never run") — on every fresh production deployment the meta-spec registry is broken (any meta-spec insert fails), which is damage outside this task's surface. It also fails the task's own acceptance criterion "`cargo test --all` passes" (the 7 failures are inside `cargo test --all`). Note the implementer's round-3 evidence cited `cargo test -p gyre-adapters --lib sqlite::task` → 11 passed — a module filter that excludes the failing `sqlite::meta_spec` and `sqlite::tests` modules, which is how the collision survived three repair rounds. **Fix:** renumber the directory to the next unused sequence (e.g. `2026-10-10-000057_task_spec_ref`); the up/down SQL is unchanged (and already passes the portability check). Re-run `cargo test -p gyre-adapters --lib sqlite::` and `scripts/check-migration-versions.sh` after the rename.

## Confirmed real (prior-round fixes verified this round)

- Round-3 F1 fixed: `workspace_id` restored to `NewTaskRow`, the insert row construction, and the upsert DO UPDATE set in BOTH sqlite and postgres task adapters (commit `147d8259`), with the `create_persists_workspace_scope` regression test passing (kill-verified by the implementer; source and passing run re-verified here).
- Round-3 F2 fixed: `decompose_delegation` hard-bails on an empty-SHA `spec_ref`, releasing the claim to Blocked with no fabricated sub-tasks (commit `147d8259`); `delegation_with_empty_sha_spec_ref_stays_blocked` passes.
- Signal-chain behavior verified by the passing 19-test suite: SpecApproved emission and routing, spawn-on-demand workspace orchestrator with exactly-one-active registry locks, delegation + coordination task creation with cross-workspace p4 notifications, repo orchestrator spawn and spec-pinned ordered decomposition with depends_on chaining, scheduler type discrimination (Delegation/Coordination vs Implementation), cycle serialization and stale-claim crash recovery, targeted (non-bulk) inbox ack.

## Verdict

Needs revision for F1 only. The implementation substance of Phases 1–3 and all prior repair-round fixes are real and verified; the migration renumber is mechanical and does not require re-review of the chain logic, but the candidate as shipped breaks fresh deployments and fails the repo's own invariant gates, so it cannot be approved.
