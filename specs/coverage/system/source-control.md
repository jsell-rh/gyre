# Coverage: Source Control

**Spec:** [`system/source-control.md`](../../system/source-control.md)
**Last audited:** 2026-04-13 (full audit — reclassification from not-started)
**Coverage:** 10/13 (2 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Core Decision | 2 | implemented | - | Git smart HTTP in git_http.rs (info/refs, git-upload-pack, git-receive-pack). jj agent interface via api/jj.rs (jj/init, jj/log, jj/new). Repository domain model with workspace isolation. |
| 2 | Why jj Is Critical for Agents | 2 | n/a | - | Rationale section — no implementable requirement. |
| 3 | 1. Every Tool Execution = Atomic Change | 3 | implemented | - | jj_new in spawn.rs:464-474 creates anonymous change on agent spawn. jj_ops.rs:53-72 implements jj new with description. No explicit git add/commit needed. |
| 4 | 2. Operation Log = Crash Recovery | 3 | implemented | - | jj_undo in jj_ops.rs:143-146. jj_log for operation history (jj_ops.rs:80-114). Session entity with start/end timestamps in agent_tracking.rs. |
| 5 | 3. Anonymous WIP Changes | 3 | implemented | - | jj_new creates anonymous DAG changes without branch names. jj_bookmark_create (jj_ops.rs:137-141) deferred until ready to push. |
| 6 | 4. Automatic Rebasing | 3 | task-assigned | task-106 | Conflict detection via speculative_merge.rs (runs every 60s). No automatic jj rebase trigger when target branch moves. Agent must manually handle rebase. |
| 7 | 5. Conflict as State, Not Error | 3 | implemented | - | Conflicts materialized as state in speculative_merge.rs. OrderIndependent vs OrderDependent conflict types. Merge processor continues processing other MRs during conflicts. |
| 8 | 6. Speculative Merge Compatibility | 3 | implemented | - | Speculative merge background job in speculative_merge.rs every 60s. Dependency-aware speculation. SpeculativeConflict and SpeculativeMergeClean events emitted. |
| 9 | 7. Session Checkpoints | 3 | implemented | - | refs/tasks/{task-id} written in spawn.rs:494. refs/agents/{agent-id}/snapshots/{n} via git_refs.rs:92-115 (count_refs_under). snapshot.rs for point-in-time checkpoints. |
| 10 | The Separation | 3 | n/a | - | Architecture diagram and rationale — no implementable requirement. |
| 11 | Merge Requests & Merge Queue as Primitives | 2 | implemented | - | Full MR domain model (merge_request.rs): id, lifecycle (Open/Approved/Merged/Closed/Reverted), atomic_group, depends_on. Merge queue with topological sort, priority ordering, atomic groups, dependency satisfaction checks (merge_processor.rs). |
| 12 | Agent-to-Commit Tracking | 2 | implemented | - | AgentCommit: agent_id, commit_sha, branch, task_id, spawned_by_user_id, parent_agent_id, model_context, attestation_level. AgentWorktree: initial/current branch, created_at. Session entity with timestamps. Provenance endpoints in api/agent_tracking.rs. |
| 13 | External Repository Mirroring | 2 | implemented | - | mirror_sync.rs background job every 60s. Repository model: is_mirror, mirror_url, mirror_interval_secs fields. Post-sync graph extraction and spec ledger sync. |
