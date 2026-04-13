# Coverage: Forge Advantages: Why Gyre Owns the Source Control Layer

**Spec:** [`system/forge-advantages.md`](../../system/forge-advantages.md)
**Last audited:** 2026-04-13 (full audit — all 8 capabilities verified against code; capabilities tracked under source-control.md, agent-runtime.md, platform-model.md)
**Coverage:** 8/8 (2 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | The Core Insight | 2 | n/a | - | Rationale — no implementable requirement. Explains why forge-native is necessary. |
| 2 | Capability 1: Speculative Merging | 2 | verified | - | speculative_merge.rs: 60s background job, refs/speculative/{branch}, SpeculativeConflict/SpeculativeMergeClean domain events, OrderIndependent/OrderDependent conflict types. Agent notification via WebSocket. |
| 3 | Capability 2: Atomic Agent Operations | 2 | verified | - | spawn.rs: single transaction creates agent record + JWT + worktree + task assignment + branch ref. Complete handler atomically opens MR + marks task done + marks agent idle. Rollback on any failure. |
| 4 | Capability 3: Zero-Latency Feedback | 2 | verified | - | git_http.rs: post-receive hooks are synchronous function calls. Validates push, updates tracking, evaluates merge queue, broadcasts domain events, returns result — all before git push returns. |
| 5 | Capability 4: Server-Side Pre-Accept Validation | 2 | verified | - | git_http.rs: pre-accept checks in receive-pack handler — arch lint (no domain→adapter import), conventional commit format, em-dash rejection, spec reference validation. Pluggable per-repo via gate definitions. ABAC-aware. |
| 6 | Capability 5: Rich Commit Provenance | 2 | verified | - | agent_tracking.rs: AgentCommit with agent_id, commit_sha, task_id, spawned_by_user_id, parent_agent_id, model_context, attestation_level. Queryable via provenance API. Foreign key to agent sessions. |
| 7 | Capability 6: Custom Ref Namespaces | 2 | verified | - | git_refs.rs: refs/agents/{id}/head, refs/agents/{id}/snapshots/{n}, refs/tasks/{task-id}. speculative_merge.rs: refs/speculative/{branch}. Server-managed, not agent-writable directly. |
| 8 | Capability 7: jj as Native Agent VCS | 2 | verified | - | jj_ops.rs: jj_new (anonymous changes), jj_log (operation history), jj_undo, jj_bookmark_create, jj_squash. spawn.rs: jj_new at agent spawn. Operation log for crash recovery. |
| 9 | Capability 8: Cross-Agent Code Awareness | 2 | verified | - | code_awareness.rs: GET /repos/{id}/blame, /hot-files, /review-routing. GET /agents/{id}/touched-paths. ABAC-gated. Hot-file domain events. Combines git history with provenance DB. |
| 10 | Agent Identity Integration Summary | 2 | n/a | - | Summary table — no implementable requirement. Describes how 3-layer identity stack integrates with all 8 capabilities. |
