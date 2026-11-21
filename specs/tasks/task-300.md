---
title: "Real spec-rejection grace period and workspace iteration tier"
spec_ref: "Spec gap spec-41a92c20354c6a108e6074e3 / spec-rejection-grace-and-iteration-tier / 4a3f940495a8"
depends_on: []
progress: not-started
commits: []
---

## Required behavior

Per agent-runtime.md §1 Phase 7 'Spec rejection mid-flight'. (1) The reject handler (crates/gyre-server/src/api/specs.rs) must implement the specced sequence instead of the current synchronous stop: cancel the in-flight tasks and delegation task, send the shutdown message, then start a real 60-second grace timer using the same mechanism as the budget path (start_budget_grace_timer); agents that commit and call agent.complete within the grace period are marked Idle; at expiry the server kills remaining agents via ComputeTarget::kill_process(), marks them Stopped, and revokes their JWTs; the terminal worktree cleanup (cleanup_worktrees_for_task) runs only after the kill, preserving the agent-workflow terminal-cleanup contract (coordinate with the active agent-workflow plan's terminal-worktree-cleanup task, which owns ephemeral-worktree teardown on terminal states). The existing priority-2 'Spec rejected' notification fanout and task cancellation are preserved. (2) Iteration-ceiling middle tier: add max_agent_iterations: Option<u32> to the Workspace entity (next free migration number, port contract enforced in EVERY adapter including the mem adapter per check-mem-port-contracts), settable via the workspace update API; resolution becomes repo.max_agent_iterations → workspace.max_agent_iterations → server default 10, applied consistently in the Ralph-loop ceiling check and every surface that renders the ceiling (constraints prompt, spawn).

## Acceptance Criteria

- [ ] On POST reject with an active implementing agent that does not complete: the agent is still Active within the grace window and has the shutdown message in its inbox; after 60 seconds its process is killed (kill_process invoked on the compute target), its status is Stopped, its JWT is rejected on next use, and its worktree teardown happens after the kill. An agent that completes within the grace window is marked Idle.
- [ ] A repo with max_agent_iterations unset in a workspace with max_agent_iterations=3 reaches the ceiling (task Blocked, agent Failed, TaskBlocked message to the repo orchestrator) after 3 iterations, not 10; with both unset the ceiling remains 10.
- [ ] Workspace max_agent_iterations is settable and readable via the workspace API and persisted durably (survives restart).

## Verification intent

Spawn an agent on a task whose spec is then rejected, keep the agent from completing, and inspect state 5 seconds after reject — at the base the handler has already synchronously transitioned the agent to Stopped and torn down its worktree (api/specs.rs transition_status(Stopped) runs before any grace handling; the 'grace_period_secs': 60 exists only as payload text sent to an already-Stopped agent; no timer, no kill_process, no JWT revocation runs on this path, in contrast to the real budget grace timer), so the 'still Active within the grace window, killed only at expiry, token revoked' assertions fail. Separately, set workspace max_agent_iterations=3 with the repo field unset and drive gate failures — at the base the ceiling resolves repo → server default 10 because the Workspace entity has no max_agent_iterations field (only Repository does, crates/gyre-domain/src/repository.rs:57), so the task is not Blocked at 3 iterations.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/agent-runtime.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `6a0d0dc0bb142af9f5ac4d1be1c142f795b9b34555ddd9a114ef1f10f0a59e6d`.

[
  {
    "id": "e60f62cd06826e5e4de9decf",
    "decision": "gap",
    "requirement_quote": "Agents have 60 seconds to commit current work (same grace period as budget exhaustion), then are killed and marked `Stopped`",
    "detail": "The Ralph loop core is implemented: pass path merges and marks the task Done, agent Idle, token revoked; fail path writes per-gate failure messages to the agent's inbox, closes the failed MR, and re-spawns the same agent entity with the same persona/task and a FRESH session (no conversation history — new JWT, re-assembled prompt set, reused worktree); iteration ceiling with repo-level max_agent_iterations falling back to a server default of 10; at the ceiling the task is Blocked, the agent Failed, the token removed, and a TaskBlocked message is routed to the repo orchestrator (spawning one when none is live). Spawn-failure policy: 3 retries at 1s/5s/30s, then Blocked plus a priority-1 'Compute target unavailable: {target_name}' notification, no fallback to a different target. TWO gaps: (1) Spec rejection mid-flight — the specced 60-second grace is decorative: the reject handler synchronously marks active agents Stopped and tears down their worktrees immediately; the 'grace_period_secs': 60 value exists only as a payload field in a shutdown message sent to an already-Stopped agent; there is no timer and no ComputeTarget::kill_process() call on this path (the budget path implements a real grace timer + kill via start_budget_grace_timer — the contrast proves the mechanism exists but is not applied here), and the agent's JWT is not revoked. (2) The iteration-default tier 'falls back to workspace default, then server default of 10' is missing its middle tier: resolution is repo.max_agent_iterations → DEFAULT_MAX_ITERATIONS (10); the Workspace entity has no max_agent_iterations field anywhere.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "let _ = agent.transition_status(gyre_domain::AgentStatus::Stopped);",
        "detail": "On spec rejection the agent is marked Stopped synchronously inside the task-cancellation loop — before any grace period could elapse — so 'agents have 60 seconds to commit current work, then are killed and marked Stopped' is not the implemented order of operations."
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "\"grace_period_secs\": 60,",
        "detail": "The 60-second grace exists only as a payload field of the shutdown message delivered to the already-Stopped agent; no timer, no deferred kill via ComputeTarget::kill_process(), and no JWT revocation run on this path."
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "// helper). The 60-second shutdown grace in the spec is for",
        "detail": "The code itself documents the deviation: worktrees are torn down immediately because 'the agents are stopped above' — an intentional code-versus-spec contradiction with no amending spec change."
      },
      {
        "file": "crates/gyre-server/src/api/budget.rs",
        "quote": "async fn start_budget_grace_timer(state: Arc<AppState>, workspace_id: Id, grace_secs: u64) {",
        "detail": "The budget-exhaustion path implements a real 60-second grace timer with deferred kill — the same grace the spec requires on spec rejection — proving the mechanism exists but is not applied to the rejection path."
      },
      {
        "file": "crates/gyre-server/src/ralph_loop.rs",
        "quote": ".unwrap_or(DEFAULT_MAX_ITERATIONS);",
        "detail": "Iteration resolution is repo.max_agent_iterations → DEFAULT_MAX_ITERATIONS (10) with no workspace tier; the Workspace struct has no max_agent_iterations field, so the specced 'falls back to workspace default, then server default of 10' middle tier does not exist."
      },
      {
        "file": "crates/gyre-server/src/signal_chain.rs",
        "quote": "pub(crate) const SPAWN_RETRY_BACKOFF_SECS: [u64; 3] = [1, 5, 30];",
        "detail": "Positive wiring for the spawn-failure policy: 3 retries at 1s/5s/30s, then task Blocked, orchestrator notified, priority-1 'Compute target unavailable' notification, and no fallback to a different compute target."
      }
    ]
  }
]
