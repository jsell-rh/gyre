---
title: "Ephemeral worktrees: teardown when work is terminally complete (task terminal states), preserving the Ralph-loop respawn working copy"
spec_ref: "Spec gap spec-4c39ca6ea3e71ce8a53398f3 / terminal-worktree-cleanup / d30f233daf86"
depends_on: [task-258]
progress: not-started
commits: ["e5b21daaca840e5e6ea60597d6247ffb62b45065"]
---

## Required behavior

Per specs/development/agent-workflow.md §Parallel Work via Worktrees, worktrees are ephemeral — created at task start (already real: spawn.rs provisions a jj workspace or plain-git worktree recorded in agent_worktrees) and cleaned up on completion. Today cleanup exists only on the stale-agent Dead transition (stale_agents.rs), the admin kill path, and the manual DELETE route — a normally-completing agent's worktree and its DB row persist indefinitely. CRITICAL placement constraint discovered in code: relaunch_agent_session (ralph_loop.rs) REUSES the agent's existing worktree record and working directory for Phase 7 gate-failure respawns ('agent has no worktree to re-spawn', ralph_loop.rs:684), so cleanup must NOT run at the MR-creation completion (complete_agent/create_mr_for_completion) — it must run when the work is terminally complete: (a) task Done after MR merge (the ralph_loop watcher's task→Done transition, ralph_loop.rs ~218-232); (b) the Phase 7 iteration-ceiling Blocked path (currently revokes the token but leaves the worktree); (c) the fail_agent / task-Cancelled terminal paths that lack cleanup. Cleanup must reuse the same teardown semantics as the stale_agents Dead path (jj workspace teardown or plain-git worktree removal via the git_ops/jj_ops ports, plus worktrees.delete for the DB record) — extract/reuse that helper rather than duplicating it. Ordering: cleanup runs AFTER snapshot-ref writing and AFTER any session-state capture at termination (task-258 — agent-session-commit-tracking, specs/system/source-control.md — records session end state: ended_at, current branch, and dirty state via git status --porcelain in the worktree on these same termination paths; removing the working copy first would destroy its data source, so this task must land after task-258. Note task-260 is the completed backlog-publication task for source-control.md and provides no such ordering guarantee). Only the working copy and its DB record are removed: the pushed branch, MR, commits, and snapshot refs must remain intact server-side. Update docs/agent-protocol.md (which currently claims complete 'removes the git worktree') to describe the actual terminal-cleanup semantics so the doc matches shipped behavior.

## Acceptance Criteria

- [ ] Happy path: an agent completes, its MR merges, the task transitions to Done — afterwards the worktree directory no longer exists on disk, worktrees.find_by_agent(agent) returns empty, and the MR, its branch, and the pushed commits are still present and readable.
- [ ] Iteration ceiling: an agent that exhausts max_iterations with failing gates (task Blocked, agent Failed) has its worktree removed from disk and DB.
- [ ] Respawn preserved: after a gate failure the agent is completed (MR created and closed) and re-spawned — the respawn succeeds in an existing worktree (cleanup did not fire at MR-creation completion).
- [ ] Teardown is the real VCS operation (jj workspace removed / git worktree pruned), not just a DB row delete — the directory is gone from the filesystem.

## Verification intent

Drive an agent to completion and merge (task Done), then stat the agent's worktree path and query worktrees.find_by_agent: today (the real defect) the directory and the agent_worktrees row persist indefinitely after completion — the probe must fail on the current tree by finding them, and pass only when both are gone. Companion probe proving correct placement: trigger a gate-failure respawn and assert the worktree still exists at relaunch time — a naive at-complete cleanup implementation (matching the stale docs/agent-protocol.md claim) breaks respawn with 'agent has no worktree to re-spawn' and fails this probe.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/development/agent-workflow.md`. Source: `fb1424fbac55cd23cb08606f4cd667a96f4dbc43`; plan: `b3acf94c2c31906d0a88cf9be08f87a86c09232cb273c0a110aa5daca9688c95`.

[
  {
    "id": "346ce862de4b15352f0239c7",
    "decision": "gap",
    "requirement_quote": "Worktrees are ephemeral - created at task start, cleaned up on completion.",
    "detail": "Bullets 1 and 2 are satisfied by real production wiring: POST /api/v1/agents/spawn provisions each agent an isolated working copy — a jj workspace ({repo}/workspaces/{agent_id}) or, as fallback, a plain-git worktree ({repo}/worktrees/{branch_slug}) via git_ops.create_worktree — recorded in the agent_worktrees table linked to agent and task (crates/gyre-server/src/api/spawn.rs:934-1143), so parallel agents work on separate branches/directories without stepping on each other. Bullet 3 is only half-implemented: worktrees are created at task start, but they are NOT cleaned up on completion. The complete handler (spawn.rs:2632-2820, shared by the REST and MCP completion surfaces via create_mr_for_completion) creates the MR, transitions the task to Review and the agent to Idle, emits audit events, revokes the agent token, revokes task-scoped secrets, writes snapshot refs, and notifies the spawner — it never calls git_ops.remove_worktree or worktrees.delete for the agent's worktree. Worktree removal exists only on three other paths: the stale-agent detector's Dead transition (stale_agents.rs:105-126), the admin kill path (admin.rs:405-407), and the manual DELETE /api/v1/repos/:id/worktrees/:id route. A normally-completing agent's worktree and its DB record therefore persist indefinitely after completion (docs/agent-protocol.md even claims complete 'removes the git worktree' — the code does not), contradicting the 'ephemeral, cleaned up on completion' requirement.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "quote": ".create_worktree(&repo_root, &worktree_path, &req.branch)",
        "detail": "Satisfied part: spawn creates a real per-agent git worktree (jj workspace when jj provisioning succeeds, plain-git worktree at {repo_root}/worktrees/{branch_slug} otherwise) and records it in agent_worktrees linked to the agent and its task — isolated copies per task, supporting parallel agents."
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "quote": "/// Agent signals it has finished its task:\n/// 1. Creates a MergeRequest (source->target)\n/// 2. Transitions task status to Review\n/// 3. Transitions agent status to Idle",
        "detail": "The completion contract itself lists no worktree-removal step. I inspected the full complete_agent handler body (spawn.rs:2632 through the ralph_loop on_agent_mr_created call at the end): it performs MR creation, status transitions, audit events, token revocation, interrogation-policy and conversation-context cleanup, task-secret revocation, snapshot-ref writing, analytics, and notifications — remove_worktree/worktrees.delete are never called on this path (grep confirms remove_worktree appears only in admin.rs, stale_agents.rs, gate_executor.rs, and the git2/mem adapters). The worktree of a successfully completed agent is therefore not cleaned up on completion."
      },
      {
        "file": "crates/gyre-server/src/stale_agents.rs",
        "quote": "// Clean up worktrees\n                if let Ok(worktrees) = state.worktrees.find_by_agent(&agent.id).await {",
        "detail": "Worktree teardown is implemented only on the Dead path: the stale-agent detector removes worktrees (including jj workspace teardown) when an agent is marked Dead after heartbeat expiry — not when the agent completes its task. The only other removal paths are the admin kill handler (admin.rs:405-407) and the manual DELETE /repos/:id/worktrees/:id route, so 'cleaned up on completion' has no production implementation."
      }
    ]
  }
]
