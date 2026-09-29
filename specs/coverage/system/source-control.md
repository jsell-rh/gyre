# Coverage: Source Control

**Spec:** [`system/source-control.md`](../../system/source-control.md)
**Last audited:** 2026-09-29 (full re-verification against current code — all 10 implemented sections confirmed genuine, promoted to verified)
**Coverage:** 10/13 (2 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Core Decision | 2 | verified | - | Git smart HTTP genuine (git_http.rs: git_info_refs/git_upload_pack/git_receive_pack registered at /git/:ws/:repo/*, real pkt-line encoding, ABAC + mirror read-only enforcement, 10+ integration tests). jj agent interface via api/jj.rs. Repository domain model with workspace isolation. |
| 2 | Why jj Is Critical for Agents | 2 | n/a | - | Rationale section — no implementable requirement. |
| 3 | 1. Every Tool Execution = Atomic Change | 3 | verified | - | jj_init + jj_new on agent spawn (spawn.rs:464-467) creates described change; JjOpsAdapter.jj_new (jj_ops.rs:53-72) shells to real `jj` binary. Per-tool atomicity is jj's inherent behavior; platform provisions jj. |
| 4 | 2. Operation Log = Crash Recovery | 3 | verified | - | jj_undo (jj_ops.rs:143-146) + jj_log operation history (80-114) shell to real jj; Session entity with timestamps in agent_tracking. |
| 5 | 3. Anonymous WIP Changes | 3 | verified | - | jj_new creates anonymous DAG change (no branch); jj_bookmark_create (jj_ops.rs:137-141) defers bookmark until push (spawn.rs:1207-1208 creates bookmark at MR time). |
| 6 | 4. Automatic Rebasing | 3 | task-assigned | task-106 | Conflict detection via speculative_merge.rs (runs every 60s). No automatic jj rebase trigger when target branch moves. Agent must manually handle rebase. |
| 7 | 5. Conflict as State, Not Error | 3 | verified | - | speculative_merge.rs materializes conflicts as SpeculativeResult state (not error); ConflictType::OrderIndependent/OrderDependent classified by prior speculated-clean branches (319-326); processor continues other branches. Tests confirm classification. |
| 8 | 6. Speculative Merge Compatibility | 3 | verified | - | Background job every 60s (speculative_merge.rs); per-branch speculative merge vs main; emits SpeculativeMergeClean (387) / SpeculativeConflict (397) MessageKinds to workspace. |
| 9 | 7. Session Checkpoints | 3 | verified | - | refs/tasks/{task-id} + refs/agents/{id}/head written on spawn (spawn.rs:493-496); refs/agents/{id}/snapshots/{n} via count_refs_under (spawn.rs:1230-1235, git_refs). |
| 10 | The Separation | 3 | n/a | - | Architecture diagram and rationale — no implementable requirement. |
| 11 | Merge Requests & Merge Queue as Primitives | 2 | verified | - | Full MR domain: MrStatus Open/Approved/Merged/Closed/Reverted with guarded transition_status (merge_request.rs:128-140), depends_on + atomic_group fields. merge_processor.rs: Kahn topological_sort_with_priority (priority-within-tier), build_queue_dependency_graph, dependencies_satisfied, atomic_group_ready + merge_atomic_group (rollback/requeue), chain-depth warning. ~15 topo/atomic-group tests kill real ordering bugs. |
| 12 | Agent-to-Commit Tracking | 2 | verified | - | AgentCommit (agent_tracking.rs:6-24) all spec fields (commit_sha, branch, task_id, spawned_by_user_id, parent_agent_id, model_context, attestation_level); instantiated + persisted by POST /repos/:id/commits/record (api/agent_tracking.rs:102) with list/provenance endpoints + tests. AgentWorktree with initial/current branch. |
| 13 | External Repository Mirroring | 2 | verified | - | mirror_sync.rs run_once filters is_mirror, calls git_ops.fetch_mirror, updates last_mirror_sync, runs post-sync spec-ledger sync + knowledge-graph extraction on default branch (32-86). Repository fields is_mirror/mirror_url/mirror_interval_secs. GitHub App integration explicitly deferred per spec (later milestone). |
