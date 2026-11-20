# Review — task-269 (Ephemeral worktrees: terminal-state teardown, respawn preserved)

Spec: `specs/development/agent-workflow.md` §Parallel Work via Worktrees
("worktrees are ephemeral — created at task start, cleaned up on completion").
Commits under review: `e5b21daa` (implementation checkpoint), `5dfcf642`
(compile/format repair); `b3107123`/`10b9e962`/`52a6f8bf` are process commits.
Base: `9f0c709f`. Candidate: `52a6f8bf`.
Verdict: **approved** — all four acceptance criteria are implemented with real
production code and killing-direction tests; no blocking findings.

## Probes (evidence: /tmp/stage/review-evidence/task-269-review.md)

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → green (6m35s).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` → green — the new
  `#[cfg(test)]` code type-checks (plain `cargo check` does not cover it;
  `--all-targets` is wrapper-rejected).
- `cargo test -p gyre-server --lib ralph_loop::tests` → **blocked by the
  stage wrapper** (gyre-server codegen exceeds the 4 GiB budget) —
  infrastructure restriction, not a code defect. Required verifier probes:
  PRIMARY `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib ralph_loop::tests`
  (expect `merge_completion_tears_down_worktree`,
  `iteration_ceiling_tears_down_worktree`,
  `terminal_cleanup_is_the_real_vcs_operation`, and the AC3 extension to
  `gate_failure_respawns_agent_with_fresh_context` to pass); SECONDARY
  `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` for regression on the
  consolidated paths (stale_agents, api::tasks, api::spawn, mcp, repos,
  specs suites).
- Live git probe against a real bare repo: `git worktree add --checkout` with
  a nonexistent branch fails with exactly the "invalid reference" string the
  adapter's `-b` fallback matches; `git worktree list --porcelain` registers
  the path; `git worktree remove` (what `Git2OpsAdapter::remove_worktree`
  runs) deregisters AND deletes the directory — the AC4 test's premises and
  the "real VCS operation" claim hold against real git.
- `scripts/check-arch.sh`, `check-task-commit-attribution.sh`,
  `check-mcp-write-tools.sh`, `check-inert-enforcement.sh`,
  `check-relative-path-defaults.sh`, `check-in-memory-state-stores.sh`,
  `cargo fmt --check` → all green.

## Verified working

- **Shared teardown, not duplication**: `cleanup_agent_worktrees`
  (api/spawn.rs) performs the real VCS teardown per worktree —
  `git_ops.remove_worktree` (real `git worktree remove`), jj workspace forget
  on `{repo}/jj-main` with the spawn-time name `agent-{id}` plus
  working-copy removal (`cleanup_jj_workspace`), then the `agent_worktrees`
  record delete. The stale-agent Dead path and the admin kill path now call
  this one helper (the old admin inline block skipped the jj forget).
- **Terminal placements exactly as required**: (a) task Done after MR merge —
  `complete_task_on_merge` (ralph_loop.rs), after the Done persist;
  (b) Phase 7 iteration ceiling (`on_gates_failed`) and the terminal
  respawn-failure tail of `respawn_with_retry`; (c) `fail_agent`;
  (d) task cancellation on every surface (HTTP `transition_task_status`,
  MCP `gyre_update_task`, repo archival, spec rejection) via
  `cleanup_worktrees_for_task`, which resolves assignee + task-holding
  agents.
- **Respawn preserved (AC3)**: no cleanup at MR-creation completion — grep
  confirms `cleanup_agent_worktrees` never appears in
  `complete_agent`/`create_mr_for_completion`; `relaunch_agent_session`
  still requires the worktree ("agent has no worktree to re-spawn"), and the
  extended respawn test asserts record + directory survive the respawn.
- **Ordering**: every placement runs `agent_sessions::end_session` (real
  branch + `git status --porcelain` capture, no-op when no session is open)
  BEFORE removing the working copy; snapshot refs are written against the
  bare repo at completion and are untouched. Only the working copy and its
  DB row go — MR/branch/commits survive (asserted in the merge test).
- **Tests are killing-direction**: every teardown callsite is a pure
  addition vs base `9f0c709f`, so `merge_completion_tears_down_worktree`,
  `iteration_ceiling_tears_down_worktree`, and
  `terminal_cleanup_is_the_real_vcs_operation` fail on the pre-task-269
  tree by construction. The AC4 test drives the merge watcher with the real
  `Git2OpsAdapter` against a real bare repo and a really registered git
  worktree, asserting `git worktree list` deregistration, directory removal,
  the jj forget with the spawn-time name, and the DB delete. Fixture traces
  verified (real temp worktree dir in `loop_state`; mem worktree delete is
  real; `ConfigurableJjOps` records the forget call).
- **Docs**: docs/agent-protocol.md no longer claims complete "removes the
  git worktree"; it now states the deferred terminal-cleanup semantics
  accurately (which states fire it, session capture first, what survives).
- **Bookkeeping**: task-269.md frontmatter records both product commits
  (attribution check green); no coverage matrix exists for
  specs/development/agent-workflow.md (coverage tracks specs/coverage/system/
  only) — the "no rows to update" claim is accurate.

## Non-blocking observations

- Shipped-notes wording says the "budget-preserve kill path" now calls the
  helper; it does not (and must not — budget kills preserve worktrees for
  reassignment, admin.rs gates on `!preserve_worktree`). Code is correct;
  only the prose in the shipped section is imprecise.
- A dirty plain-git worktree can leave a stale `.git/worktrees` registration
  (`git worktree remove` refuses without --force; the fs backstop still
  removes the directory; no `git worktree prune`). Identical to the
  pre-existing stale_agents Dead-path semantics the task required reusing —
  not a regression.
- A manually-Done task (Done set outside the merge watcher) does not fire
  teardown; tasks.rs documents this deliberately — the watcher owns the
  terminal transition, matching the task's required placement (a).
- reject_spec/archive_repo remove the working copy immediately after the
  60s-grace shutdown message; intended for cancelled work (session end-state
  is captured first) and documented in-code.
