# Manager Agent: Orchestrating Development via Ralph Loops

> **Superseded.** The Manager Agent concept has been split into two specs:
> - **Orchestration**: [`specs/system/platform-model.md`](../system/platform-model.md) (Two-Level Orchestration -- workspace orchestrator + repo orchestrator)
> - **Implementation loop**: [`specs/system/agent-runtime.md`](../system/agent-runtime.md) §1 (agent lifecycle, signal chain, Ralph loop)
>
> This file is kept for historical context only.

## Role

The Manager Agent is the top-level orchestrator. It does not write code. It decomposes work, spawns sub-agents, defines their Ralph loops, and runs its own meta Ralph loop over the entire system. It is the reconciliation controller for the development process.

## Manager Agent Ralph Loop

The Manager operates a continuous outer loop:

```
LOOP:
  1. OBSERVE  - Read specs/, open issues, current branch state, test results,
                quality grades, tech debt tracker, execution plan status.
  2. PLAN     - Identify the highest-priority work. Decompose into discrete,
                parallelizable tasks with clear acceptance criteria.
  3. DISPATCH - Spawn sub-agents, each with:
                - A scoped task description
                - Pointers to relevant specs (cache-hit-optimized paths)
                - Defined Ralph loop parameters (see below)
                - A worktree/branch assignment
                - A lifetime/budget
  4. MONITOR  - Watch sub-agent progress via their Ralph loop outputs:
                - PR status, review feedback, test results
                - Agent health (stuck? looping? drifting?)
                - Resource consumption vs. lifetime budget
  5. RECONCILE - Converge toward desired state:
                - Merge completed, passing PRs
                - Restart or reassign stuck agents
                - Escalate to human when judgment is required
                - Update execution plans, quality grades, tech debt tracker
  6. REPEAT
```

## Sub-Agent Ralph Loop Definition

Each sub-agent spawned by the Manager runs its own inner Ralph loop. The Manager defines these parameters at dispatch time:

```
SUB-AGENT RALPH LOOP:
  1. IMPLEMENT  - Write code/tests/docs to satisfy the task acceptance criteria.
  2. SELF-REVIEW - Review own changes against:
                   - specs/ (does it match the spec?)
                   - Architecture lints (does it pass mechanical checks?)
                   - Test suite (does everything pass?)
                   - Pre-commit hooks (block-secrets, i18n, cargo2nix, etc.)
  3. REQUEST REVIEW - Ask one or more peer agents to review.
                      Peer agents are assigned by the Manager or discovered
                      via convention (e.g., domain ownership).
  4. RESPOND    - Address all review feedback. Do not dismiss - resolve.
  5. TEST PLAN  - Before validation, produce a **fully comprehensive test plan**
                  covering every edge case, failure mode, and integration point.
                  The test plan is a deliverable - it gets reviewed alongside the code.
                  No code merges without a bullet-proof test plan.
  6. VALIDATE   - Run full validation per the test plan:
                  - Tests pass
                  - TUI snapshots match (if applicable)
                  - Web UI renders correctly at all breakpoints (if applicable)
                  - OTel traces show no regressions
                  - Observability stack is clean (no new errors/warnings)
  7. CONVERGE   - If all reviewers satisfied and validation passes - open MR.
                  If not - go to step 1.
  8. SIGNAL     - Report status back to Manager:
                  - SUCCESS: MR ready for merge
                  - BLOCKED: needs human judgment or missing capability
                  - FAILED: unrecoverable, needs reassignment
```

## Manager Responsibilities

| Responsibility | Detail |
|---|---|
| Task decomposition | Break epics/specs into agent-sized units. Each task should be completable in a single Ralph loop cycle. |
| Parallelism | Maximize concurrent sub-agents on independent tasks. Serialize only when there are true dependencies. |
| Conflict resolution | Detect when sub-agents are working on overlapping code. Sequence or merge as needed. |
| Capability gap detection | When a sub-agent is stuck, diagnose: is the spec unclear? Is a tool missing? Is the codebase illegible? Fix the environment, not the agent. |
| Quality reconciliation | Periodically spawn garbage-collection agents to scan for drift, stale docs, pattern violations. |
| Escalation protocol | Escalate to human only when: (a) judgment call on product direction, (b) ambiguous spec, (c) security-sensitive decision, (d) sub-agent has failed its Ralph loop 3+ times on the same task. |
| Progress tracking | Maintain execution plans in `specs/exec-plans/active/` with decision logs. Move to `completed/` on finish. |

## Meta Ralph Loops

Because the Manager owns the outer loop, it can compose Ralph loops hierarchically:

- **Feature-level:** Manager dispatches N sub-agents for a feature - each runs their Ralph loop - Manager merges and validates the feature as a whole.
- **System-level:** Manager runs a periodic full-system reconciliation - architecture lints, integration tests, quality grades, doc freshness - and spawns fix-up agents for any deviations.
- **Entropy sweep:** Scheduled garbage-collection Ralph loops that scan for and correct drift (the "golden principles" pattern).

## Agent Lifecycle Rules

- Every sub-agent has a **lifetime budget**. If exceeded, the Manager kills the agent and reassesses the task.
- Agents that signal BLOCKED are not retried blindly - the Manager must diagnose and address the blocker first.
- Agent context windows are captured and stored for audit. If a sub-agent fails, its full context is available for post-mortem.
- The Manager itself has no lifetime - it runs continuously as long as there is work in the backlog.
