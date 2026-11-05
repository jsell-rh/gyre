---
title: "Implement automatic jj rebase on target branch movement"
spec_ref: "source-control.md §4. Automatic Rebasing"
depends_on: []
progress: ready-for-review
review: specs/reviews/task-106.md
coverage_sections:
  - "source-control.md §4 Automatic Rebasing"
commits: ["d25adcd1c757da9534f64e6b51784b1667a2225d", "7de14740c491ae82c9bcdd928714f60e632a8cc2"]
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
including the R2 review findings F6/F7/F8. Behavior unchanged since
round 5; round 6 verified the merge of base `110180fb` (created by the
prior run, which died at the controller's pre-flight GitHub connectivity
check before building it) — all task source is byte-identical to the
round-5 verified HEAD `d25adcd1`, and the merged tree passes every
focused probe.

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

Round 6 (merge verification) at HEAD `e2ab1893` — merge of base
`110180fb` unioned with the task branch; the only overlapping file was
`gyre-server/src/lib.rs` (upstream `store!` conversions for the search /
dependency repositories), resolved as a union with the task's `jj_ops`
wiring. All seven task-106 source files verified byte-identical to the
round-5 real-jj-verified HEAD `d25adcd1`. Logs + exit codes in
`/tmp/stage/review-evidence/task-106-r6/`:

- `cargo check -p gyre-server --tests` → EXIT 0.
- `cargo test -p gyre-server --lib merge_processor` → 57/57 EXIT 0, incl.
  `merge_triggers_rebase_of_inflight_agents`,
  `rebase_destination_is_moved_target_branch_not_default`,
  `rebase_conflict_surfaces_as_state`, `rebase_skips_dead_agents`,
  `rebase_failure_does_not_consume_backoff_window`,
  `rebase_window_hit_defers_then_replays_after_expiry`,
  `replay_failure_keeps_pending_rebase`,
  `post_merge_gate_failure_skips_inflight_rebase`.
- `cargo test -p gyre-server --lib spawn` → 42/42 EXIT 0, incl.
  `spawn_absolutizes_legacy_relative_repo_path_for_jj_children`.
- `cargo test -p gyre-server --lib` → 1254/1254 EXIT 0 (post-merge
  sanity; same count as round 5).
- `cargo test -p gyre-server --test search_wiring --test
  dependency_persistence` → 2/2 EXIT 0 (upstream tests the merge
  brought in).
- `cargo test -p gyre-adapters jj` → 14/14 EXIT 0 in 0.02s — **guard-
  skip**: jj-cli could not be reinstalled this round
  (static.crates.io/index.crates.io connection refused from the sandbox),
  so these ran without the jj binary. The round-5 real-jj evidence
  stands for the byte-identical adapter code: 14/14 against real
  jj 0.39.0 (built from crates.io `jj-cli --locked` in that round),
  incl. `jj_rebase_clean_after_target_moves`,
  `jj_rebase_conflict_surfaces_files`, `jj_rebase_b_moves_whole_stack`,
  `jj_resolve_list_exit_code_contract`,
  `jj_workspace_add_resolves_branch_created_after_init`,
  `jj_workspace_add_failure_cleans_half_created_dir`; guard-skip
  disambiguation recorded then (with jj: 2.60s; without: 0.03s).

All builds used `SKIP_WEB_BUILD=1` (documented Rust-only path, committed
`web/dist/`): npm registry tarball fetches are blocked from this sandbox
(EACCES), so build.rs `npm ci` cannot run here. Recorded as transport
restrictions for remote verification, not code defects.
