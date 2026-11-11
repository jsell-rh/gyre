---
title: "Implement budget enforcement cascade"
spec_ref: "agent-runtime.md §4 Budget Enforcement"
depends_on:
  - task-118
progress: ready-for-review
coverage_sections:
  - "agent-runtime.md §4. Budget Enforcement"
  - "agent-runtime.md §Cascade"
  - "agent-runtime.md §Enforcement Levels"
  - "agent-runtime.md §What's Tracked"
  - "agent-runtime.md §Budget Reset"
  - "agent-runtime.md §5. Agent Prompt Structure"
commits: ["bfb2dc3dec94916312b1688752ce6401b06dc9c0", "7bdc6060362e759c303d4890302c9c0181a78091", "4bfdf85b8c1f992f0aa604c753d5277ab0a739b5"]
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

## Shipped

agent-runtime.md §4 (Budget Enforcement) and §5 (Agent Prompt
Structure) implemented in production code — no stubs, no audit-only
paths. The cascade, enforcement levels, tracking, reset, and prompt
structure shipped in commit 4bfdf85b8; review-round repairs (BudgetExhausted
inbox delivery, repo concurrent/lifetime override enforcement, priority-7
spec reconciliation, persona-only `system://persona`) in 7bdc60603.

This round repaired review finding 9af67822 F1: the repo-level
`max_agent_lifetime_secs` override could exceed the tenant absolute
ceiling.

### Repair: lifetime override bounded by the effective ceiling

- **Validation (api/repos.rs):** PUT /api/v1/repos/:id now validates
  every set repo budget field against the EFFECTIVE workspace limit
  (min of workspace config and tenant ceiling) instead of the raw
  workspace config. A dimension the workspace leaves unset inherits
  the tenant (platform-model.md §1) — previously the `(Some, Some)`
  guard skipped the check, so tenant {lifetime: 1800} + workspace
  unset + repo {lifetime: 3600} was accepted; it is now a 400.
- **Enforcement (api/budget.rs `effective_lifetime_limit_secs`):** the
  repo override is min'd against the effective workspace limit rather
  than replacing it, so a later workspace/tenant reduction below a
  previously-legitimate override binds immediately — the same live
  recomputation semantics the token/cost/concurrent dimensions honor.

### Test evidence

Three regression tests added:

- `repo_budget_validated_against_effective_workspace_limit` (repos.rs):
  the review's exact repro (tenant 1800 / workspace unset / repo 3600 →
  400, not persisted), the tokens dimension at the same corner, valid
  values persisting, and a post-reduction re-assertion of a
  previously-valid override rejected.
- `repo_lifetime_override_bounded_by_tenant_ceiling_workspace_unset`
  (budget.rs): a stale repo override of 3600s under a tenant ceiling of
  1800s budget-kills an agent aged 2000s (min(3600, 1800) = 1800).
- `repo_lifetime_override_rebinds_after_workspace_reduction`
  (budget.rs): an override of 3000s under a 3600s workspace keeps an
  agent aged 2000s Active; after the workspace budget drops to 1800s
  the same agent is budget-killed with the worktree preserved.

Sandbox probes: `cargo check -p gyre-server` and `cargo check -p
gyre-server --lib --tests` both exit 0 (test code type-checks).
gyre-server test compilation exceeds this sandbox's 4 GiB budget
(build wrapper refuses; recorded in
/tmp/stage/review-evidence/task119-repair.md), so the independent
verifier must run:

```
cargo test -p gyre-server --lib lifetime
cargo test -p gyre-server --lib repo_budget
```

plus the full `cargo test --all` gate.
