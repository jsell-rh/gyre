---
title: "Implement budget enforcement cascade"
spec_ref: "agent-runtime.md §4 Budget Enforcement"
depends_on:
  - task-118
progress: not-started
coverage_sections:
  - "agent-runtime.md §4. Budget Enforcement"
  - "agent-runtime.md §Cascade"
  - "agent-runtime.md §Enforcement Levels"
  - "agent-runtime.md §What's Tracked"
  - "agent-runtime.md §Budget Reset"
  - "agent-runtime.md §5. Agent Prompt Structure"
commits: []
---

## Spec Excerpt

From `agent-runtime.md` §4:

**Cascade:**
```
Tenant budget (absolute ceiling)
  └── Workspace budget (cannot exceed tenant)
        └── Repo budget (cannot exceed workspace)
              └── Per-agent enforcement (charged to repo, rolled up to workspace)
```

Repo-level budget provides fine-grained control. When no repo budget set, inherits workspace limit. `max_agents` per repo enforced at spawn time (fallback to `Workspace.max_agents_per_repo`). Repo's `max_agents` cannot exceed workspace's `max_agents_per_repo`.

**Enforcement Levels:**

| Threshold | Action |
|---|---|
| 80% | Warn — `BudgetWarning` notification (priority 7). Agents continue. |
| 100% | Graceful stop — `BudgetExhausted` message to all agents. 60s grace period to commit + complete. After 60s, agents killed. |
| Beyond limit | Hard kill — `kill_process()` immediately. Worktree preserved. Task remains InProgress. |

**What's Tracked:**

| Cost Type | Source | Unit |
|---|---|---|
| `llm_tokens` | Agent LLM calls (cred-proxy reports) | Input + output tokens |
| `llm_cost_usd` | Agent LLM calls | Dollar amount |
| `compute_minutes` | Wall-clock time (spawn to complete) | Minutes |
| `llm_query` | User-initiated LLM calls (briefing, explorer, specs/assist) | Per-call |

The cred-proxy reports usage via `POST /api/v1/agents/:id/usage`. Server aggregates per workspace and checks limits on every report.

**Budget Reset:** Time-based budgets (max_tokens_per_day, max_cost_per_day) reset on rolling 24h window. Background job runs hourly, clears expired usage entries.

From §5 (Agent Prompt Structure): Describes the three-section prompt assembly (meta-specs, task context, protocol) — covered here as it's the final piece of the agent spawning pipeline.

## Implementation Plan

1. **Budget cascade validation:**
   - On workspace budget update: validate ≤ tenant budget
   - On repo budget update: validate ≤ workspace budget
   - On repo max_agents update: validate ≤ workspace max_agents_per_repo
   - Reject invalid values with 400 error

2. **Usage aggregation:**
   - On `POST /api/v1/agents/:id/usage`: record usage, aggregate to repo and workspace totals
   - Maintain running totals in memory (backed by DB) for fast threshold checking
   - Charge `llm_query` to workspace directly (no repo context for user queries)

3. **Threshold enforcement:**
   - After each usage report, check workspace total against budget:
     - ≥ 80%: emit `BudgetWarning` message + create notification (priority 7)
     - ≥ 100%: emit `BudgetExhausted` messages to all active agents in workspace. Start 60s grace timer. After timeout, `kill_process()` on remaining agents.
     - Beyond limit (budget reduced below current usage): immediate `kill_process()` on all agents

4. **Compute minutes tracking:**
   - Track agent wall-clock time: spawn timestamp → complete/kill timestamp
   - Calculate minutes and add to workspace usage

5. **Budget reset background job:**
   - Runs hourly
   - For budgets with time windows (per_day): clear usage entries older than 24h
   - Use rolling window, not calendar day

6. **Agent prompt structure (§5):**
   - Verify the prompt assembly from task-116 produces the three-section structure:
     - System prompt: assembled meta-specs
     - Task context: task details, spec ref, gate failure messages, inbox messages
     - Protocol: Ralph loop rules, MCP tools, heartbeat, escalation, completion
   - Map to MCP prompts: `system://persona`, `system://protocol`, `system://context`, `system://constraints`

## Acceptance Criteria

- [ ] Budget cascade validated: repo ≤ workspace ≤ tenant
- [ ] max_agents validated: repo ≤ workspace max_agents_per_repo
- [ ] Usage aggregated per repo, rolled up to workspace
- [ ] 80% threshold: BudgetWarning notification created
- [ ] 100% threshold: BudgetExhausted messages + 60s grace + kill
- [ ] Beyond-limit: immediate kill
- [ ] Compute minutes tracked (spawn-to-complete wall clock)
- [ ] User-initiated LLM queries charged to workspace
- [ ] Budget reset job clears expired entries on rolling 24h window
- [ ] Agent prompt structure follows three-section model
- [ ] MCP prompts deliver assembled content
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/agent-runtime.md` §4 (Budget Enforcement) and §5 (Agent Prompt Structure) in full. Existing budget code: `gyre-server/src/api/budget.rs` and `gyre-domain/src/budget.rs`. Agent usage reporting: `POST /api/v1/agents/:id/usage` in `gyre-server/src/api/spawn.rs`. BudgetWarning/BudgetExhausted message kinds: `gyre-common/src/message.rs`. Compute target kill: grep for `kill_process` in server. MCP prompt delivery: `gyre-server/src/mcp.rs` — look for `system://persona` or `system://context`. Background jobs: grep for `tokio::spawn` or `interval` in `gyre-server/src/main.rs` or `lib.rs`.
