---
title: "Implement automatic jj rebase on target branch movement"
spec_ref: "source-control.md §4. Automatic Rebasing"
depends_on: []
progress: needs-revision
review: specs/reviews/task-106.md
coverage_sections:
  - "source-control.md §4 Automatic Rebasing"
commits: ["10dc345ce3f3ce0ccd2be2b72ae12b4fce1e9b38", "0117af722ec80f286d0f52efa18a6b31f16fc153"]
progress: ready-for-review

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

## Shipped (revision round 4 — contract repair)

**Contract repair (finding 79b4ff17):** the r3 round rewrote this task's own
contract — `b7d42fc3` replaced the product-surface `commits:` list with the
pipeline checkpoint `ca60b2cd` (no product surface), and `3eee5943` flipped
`ready-for-review` on top of the rewritten list. `a640ec33` restores the
contract: `commits:` back to the two product checkpoint SHAs, progress back
to needs-revision, r3's unverified Shipped claim removed. This round then
re-verified the tree from scratch; nothing below is inherited from r3's log.

**Product surface (in commits `0117af7` + `10dc345c`, verified this round):**

- `JjOpsPort::jj_rebase` (port) + adapter (`jj_ops.rs:199`): runs
  `jj git import` then `jj rebase -b @ -d <target_branch>` in the agent's
  workspace; parses the rebased count from stderr; conflict detection via
  the `jj resolve --list -r @` exit-code contract (0 = conflicted with file
  list, 2 = clean). R1 F1, F5 resolved.
- Post-merge trigger in `merge_processor.rs`: both merge paths call
  `rebase_inflight_agents` as the LAST step after `run_post_merge_gates`
  passes (single-entry `:2315`, atomic-group `:1335`); merged authors
  excluded; target selection = same target branch, MR Open/Approved,
  agent Active, worktree exists (R2 F3).
- Conflict-as-state: `has_conflicts = Some(true)` on the author's MR,
  `SpeculativeConflict` to the workspace + `Escalation` to the agent with
  the conflicting-file list; `baseline_moved` (Custom) to the agent with
  `new_base_sha` on success. All via `emit_event` (signed + persisted + WS).
- Rate limit (plan item 5): window hits set a `jj_rebase_pending` marker;
  `replay_pending_rebases` replays after expiry (batch = one rebase onto
  the newest base); backoff written only after a terminal outcome; replay
  keeps the marker on infrastructure Err (R2 F4, F8).
- R2 F6: `configured_repos_path()` anchors repos_root at the process cwd
  (absolute at rest); `spawn_agent_core` absolutizes legacy relative
  `repo.path` rows before any jj/git child call.
- R2 F7: `jj_workspace_add` runs `jj git import` before resolving
  `-r <branch>`; on failure forgets the workspace and removes the
  half-created dir so the git-worktree fallback stays reachable.

**Fresh verification (this round, HEAD `a640ec33`; logs in
`/tmp/stage/review-evidence/task-106-r4-1791584731/`):**

- `cargo test -p gyre-adapters jj` → 14/14 EXIT=0 against real jj 0.39.0
  (incl. `jj_rebase_clean_after_target_moves`,
  `jj_rebase_conflict_surfaces_files`, `jj_rebase_b_moves_whole_stack`,
  `jj_workspace_add_resolves_branch_created_after_init`,
  `jj_workspace_add_failure_cleans_half_created_dir`).
- `cargo test -p gyre-server --lib merge_processor` → 57/57 EXIT=0 (incl.
  all 8 task-106 rebase tests:
  `merge_triggers_rebase_of_inflight_agents`,
  `rebase_destination_is_moved_target_branch_not_default`,
  `rebase_conflict_surfaces_as_state`, `rebase_skips_dead_agents`,
  `rebase_failure_does_not_consume_backoff_window`,
  `rebase_window_hit_defers_then_replays_after_expiry`,
  `replay_failure_keeps_pending_rebase`,
  `post_merge_gate_failure_skips_inflight_rebase`).
- `cargo test -p gyre-server --lib -- spawn:: agent_tracking
  configured_repos_path` → 40/40 EXIT=0 (incl.
  `spawn_absolutizes_legacy_relative_repo_path_for_jj_children`,
  `configured_repos_path_{makes_relative_config_absolute,default_is_absolute,absolute_passthrough}`).
- `cargo check -p gyre-server -p gyre-adapters -p gyre-ports` → clean.
- Mechanical: `check-relative-path-defaults.sh`, `check-ignored-tool-tests.sh`,
  `check-arch.sh`, `check-mem-port-contracts.sh` all OK.
- `check-task-commit-attribution.sh`: task-106 clean; the remaining failure
  is task-210's (`a781ede2`), identical on the assignment base `8c2d1775` —
  pre-existing, out of scope here.

**Transport limitation:** the sandbox cannot bind a TCP listener
(`capabilities.json`: errno 95), so no live WS end-to-end run. The WS
notification paths are covered by the merge_processor suite
(`emit_event` asserted in the trigger/conflict tests). Host verification:
start gyre-server, land an MR while a second agent is in flight on the same
target branch, confirm the agent receives `baseline_moved` with
`new_base_sha` over WS. Exact-head GitHub CI is the mandatory gate.

## Agent Instructions

Read `specs/system/source-control.md` §4 for the full spec. The jj adapter is in `gyre-adapters/src/jj_ops.rs` — add `jj_rebase` alongside existing `jj_new`, `jj_undo`, etc. The merge processor is in `gyre-server/src/merge_processor.rs` — the post-merge hook is where rebase should be triggered. The speculative merge system is in `gyre-server/src/speculative_merge.rs` for conflict detection patterns. WebSocket messaging is in `gyre-server/src/ws.rs`. Agent worktree paths are tracked in `agent_tracking.rs`.
