---
title: "Implement automatic jj rebase on target branch movement"
spec_ref: "source-control.md §4. Automatic Rebasing"
depends_on: []
progress: ready-for-review
review: specs/reviews/task-106.md
coverage_sections:
  - "source-control.md §4 Automatic Rebasing"
commits: ["676e519647de367e7e95087bab596d449b9a4ada", "7b11ab828ffcdcbc2e533d1a6d6a1dd921d40f74"]
---

## Spec Excerpt

From `source-control.md` §4:

> When the target branch moves (because another agent's MR merged), jj automatically rebases the agent's in-progress work. With git, the agent must manually `git fetch && git rebase`, handle conflicts, and continue. In a system with 20+ agents pushing concurrently, the baseline moves constantly. jj handles this transparently.

Current state: The speculative merge system (`speculative_merge.rs`) detects conflicts when the target branch moves, but it does not trigger an automatic `jj rebase` on the agent's working branch. Conflict detection is passive — the agent must manually handle the rebase.

## Implementation Plan

1. **Rebase trigger in merge processor:**
   - After a successful merge (MR lands on default branch), identify all other in-flight agents targeting the same branch
   - For each affected agent, trigger a `jj rebase` operation on their working change
   - This should run in `gyre-server/src/merge_processor.rs` after the merge commit

2. **jj rebase adapter (`gyre-adapters/src/jj_ops.rs`):**
   - Add `jj_rebase(repo_path, revision, destination)` function
   - Execute `jj rebase -r <revision> -d <destination>` in the agent's worktree
   - Capture and return rebase result (success, conflict, or error)

3. **Conflict handling:**
   - If rebase produces conflicts, emit a `SpeculativeConflict` event
   - Set the conflict state on the agent's MergeQueueEntry
   - Notify the agent via WebSocket message (MessageKind::Escalation or similar)
   - Agent can continue working on non-conflicting files (jj conflict-as-state model)

4. **Agent notification:**
   - After successful rebase, send a WebSocket notification to the agent informing it the baseline moved
   - Include the new base commit SHA in the notification
   - After conflict rebase, send conflict details with affected file list

5. **Safeguards:**
   - Only rebase agents that are in Active status (not Dead/Completed)
   - Skip rebase if the agent's worktree path doesn't exist
   - Log all rebase operations for audit trail
   - Rate-limit rebases: if target moves multiple times in quick succession, batch into one rebase

## Acceptance Criteria

- [x] `jj_rebase` function added to jj_ops adapter
- [x] After MR merge, in-flight agents on same target branch are automatically rebased
- [x] Conflicts from rebase are surfaced as state (not errors)
- [x] Agents notified via WebSocket of baseline movement
- [x] Rebase skipped for dead/completed agents
- [x] Rebase operations logged
- [x] `cargo test --all` passes

## Spec-vs-Plan Divergence (review F5)

The plan said `jj rebase -r <revision> -d <destination>`; the spec (§4) says
the agent's "in-progress work" — the whole stack — moves onto the new base.
`-r @` would rebase only the working-copy commit and abandon its descendants
onto the old parent; the adapter therefore runs `jj rebase -b @ -d <dest>`,
which rebases the branch containing `@` (the entire in-flight stack).
Verified empirically against jj 0.39.0 (see
`jj_rebase_clean_after_target_moves` in `crates/gyre-adapters/src/jj_ops.rs`).

## Agent Instructions

Read `specs/system/source-control.md` §4 for the full spec. The jj adapter is in `gyre-adapters/src/jj_ops.rs` — add `jj_rebase` alongside existing `jj_new`, `jj_undo`, etc. The merge processor is in `gyre-server/src/merge_processor.rs` — the post-merge hook is where rebase should be triggered. The speculative merge system is in `gyre-server/src/speculative_merge.rs` for conflict detection patterns. WebSocket messaging is in `gyre-server/src/ws.rs`. Agent worktree paths are tracked in `agent_tracking.rs`.

## Shipped

Automatic jj rebase on target branch movement (source-control.md §4),
including review findings F6/F7/F8 (R2) and the R9 test-compile
repair. Production behavior is unchanged since round 5.

### R9 repair (test-target compile)

The R8 review found the candidate did not compile its own test suite:
the F9-fix test `spawn_creates_missing_branch_before_jj_workspace_add`
drives the real `Git2OpsAdapter` through `GitOpsPort` trait methods
(`init_bare`, `create_initial_commit`, `branch_exists`, `list_branches`),
but the `mod tests` block in `crates/gyre-server/src/api/spawn.rs` never
imported the trait — 4× rustc E0599, the entire gyre-server lib test
target failed to build, so every task-106 rebase test was unrunnable at
the candidate. Fixed by adding `use gyre_ports::GitOpsPort;` to the test
module (test-only; no production code touched) in commit `676e5196`,
verified at HEAD `676e5196` on top of the merged base `701cb1ad`:
the full gyre-server lib suite now builds and passes 1279/1279,
including the previously-uncompilable F9 test.

### Behavior

- **Rebase trigger**: after a merge lands and post-merge gates pass,
  `rebase_inflight_agents` (merge_processor.rs) rebases every other
  in-flight agent targeting the same branch — same target branch, MR
  Open/Approved, agent Active, worktree present; merged authors excluded.
  Runs as the LAST step in both the single-entry and atomic-group paths
  (F3: never onto a base that gate-failure recovery would revert).
- **jj layout**: per-repo shared jj main checkout at `{repo.path}/jj-main`
  (standalone, backed by the bare git repo — jj 0.39.0 refuses colocation
  inside git worktrees) with per-agent workspaces at
  `{repo.path}/workspaces/{agent_id}`; plain-git worktree fallback at
  `{repo.path}/worktrees/{branch}` when jj provisioning fails.
  `jj_workspace_add` runs `jj git import` before resolving `-r <branch>`
  (F7: branches created after the main checkout init resolve), and on
  failure forgets the workspace and removes the half-created dir so the
  git fallback stays reachable. Agent cleanup runs `jj workspace forget`.
- **`jj_rebase`** (port `JjOpsPort::jj_rebase`, adapter jj_ops.rs):
  `jj git import`, then `jj rebase -b @ -d <moved branch>` (whole
  in-flight stack). Conflict detection via the `jj resolve --list -r @`
  exit-code contract (0 = conflicted with files on stdout, 2 = clean) —
  no stdout/stderr text scraping for the decision (F1).
- **Conflict-as-state**: on conflict the author's MR gets
  `has_conflicts = Some(true)`; `SpeculativeConflict` to the workspace and
  `Escalation` to the agent carry the conflicting-file list. On success
  the agent gets `baseline_moved` (Custom) with `new_base_sha` and
  `rebased_count`. All via `emit_event` (signed, persisted, WS).
- **Deferral semantics** (plan item 5 / F4 / F8): a movement inside the
  60s backoff window sets a `jj_rebase_pending` marker (the newest base
  wins — batch semantics); `replay_pending_rebases` on processor idle
  cycles retries after expiry; the backoff timestamp is written only
  after a terminal outcome; an infrastructure Err keeps the pending
  marker on BOTH trigger and replay paths.
- **Absolute paths** (F6): `configured_repos_path()` anchors
  `GYRE_REPOS_PATH` (default `repos`) at the process cwd at server start;
  `spawn_agent_core` absolutizes legacy relative `repo.path` rows before
  any jj/git child call. All rebase attempts (success, conflict, error)
  are recorded as audit events (`jj_rebase` / `jj_rebase_conflict`).

### Test evidence

Round 9 (compile repair) at HEAD `676e5196` — the merge commit
`0a694867` (base `701cb1ad` unioned with the task branch; the only
overlapping file was `gyre-server/src/lib.rs`, resolved as a union)
plus the one-line import fix. Logs + exit codes in
`/tmp/stage/review-evidence/task-106-r9/`:

- `cargo test -p gyre-server --lib` → **1279/1279 EXIT 0** (up from
  1254 at round 6: the merged base and the F9/F11 rounds added tests).
  Includes all eight task-106 rebase tests
  (`merge_triggers_rebase_of_inflight_agents`,
  `rebase_destination_is_moved_target_branch_not_default`,
  `rebase_conflict_surfaces_as_state`, `rebase_skips_dead_agents`,
  `rebase_failure_does_not_consume_backoff_window`,
  `rebase_window_hit_defers_then_replays_after_expiry`,
  `replay_failure_keeps_pending_rebase`,
  `post_merge_gate_failure_skips_inflight_rebase`) and the F9 test
  `spawn_creates_missing_branch_before_jj_workspace_add`, which now
  compiles AND passes against the real `Git2OpsAdapter`.
- `cargo test -p gyre-adapters jj` → 15/15 EXIT 0 in 0.02s — **guard-
  skip**: jj-cli could not be installed this round (static.crates.io
  connection refused from the sandbox; `which jj` → exit 127), so these
  ran without the jj binary. The adapter production code is unchanged
  from the reviewed candidate except the F11 `jj_workspace_forget`
  idempotence fix (checkpoint `7b11ab82`); the round-5 real-jj evidence
  (14/14 against real jj 0.39.0, built from crates.io `jj-cli --locked`)
  stands for the rebase core — `jj_rebase_clean_after_target_moves`,
  `jj_rebase_conflict_surfaces_files`, `jj_rebase_b_moves_whole_stack`,
  `jj_resolve_list_exit_code_contract`,
  `jj_workspace_add_resolves_branch_created_after_init`,
  `jj_workspace_add_failure_cleans_half_created_dir`. The F11 test
  `jj_workspace_forget_absent_workspace_is_ok` is new since round 5 and
  has so far only run guard-skipped; it needs a real-jj re-run during
  remote verification.
- Pre-fix state (for the before/after pair): at `0a694867` (HEAD before
  the fix) the `mod tests` block in spawn.rs has no `GitOpsPort` import
  while the F9 test calls the four trait methods — the R8 review log
  documents the resulting 4× E0599 and failed build at the candidate;
  the identical source state is inspectable at `git show 0a694867`.

All builds used `SKIP_WEB_BUILD=1` (documented Rust-only path, committed
`web/dist/`): npm registry tarball fetches are blocked from this sandbox
(EACCES), so build.rs `npm ci` cannot run here. Recorded as transport
restrictions for remote verification, not code defects. The sandbox was
also heavily I/O-contended during round 9 (load ~19 on 4 cores,
92% iowait), which slowed compilation but did not affect results.
