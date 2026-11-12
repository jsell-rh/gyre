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
commits: ["d00113b22ab971ea5b5586e6e4841e0ab68f162e", "dc09d6950d1df2d7498448e66642908a747c4b0a", "29d1a9618ee5181319aacccb704397b88242d3c7", "bfb2dc3dec94916312b1688752ce6401b06dc9c0", "7bdc6060362e759c303d4890302c9c0181a78091", "4bfdf85b8c1f992f0aa604c753d5277ab0a739b5"]
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
Structure) implemented in production code across commits 4bfdf85b8,
7bdc60603, bfb2dc3de, 29d1a9618, dc09d6950 (prior rounds — cascade
validation, enforcement levels incl. 60s grace vs beyond-limit hard
kill, tracking incl. llm_query workspace-only charging, rolling-24h
reset job, three-section prompt assembly + MCP prompt delivery), and
d00113b22 (this round's review repairs).

### This round — repairs for review cf0145abb (commit d00113b22)

**F1 — rolling-window resync keeps llm_query off the repo budget**
(`crates/gyre-server/src/api/budget.rs`): `apply_rolling_24h_window`
recomputed repo usage entries from `BudgetCallRecords` with no
`usage_type` filter, while `record_llm_call_usage` (correctly) excludes
`llm_query` from the incremental repo charge. specs/assist, explorer,
and MCP spec_assist persist llm_query records WITH `repo_id`, so after
the first hourly cycle user queries were re-charged into the repo
budget entry — letting them exhaust a repo budget and block agent
spawns (`check_repo_spawn_budget`), exactly what the spec forbids
("Charged to workspace budget directly (no repo-level tracking)").
The resync now excludes `usage_type == "llm_query"` from `repo_sums`
(kept in ws_sums/tenant total), mirroring the incremental path. New
mixed-sequence regression test
`rolling_window_keeps_llm_query_out_of_repo_budget`: 900 agent_run +
500 llm_query tokens against one repo → after `apply_rolling_24h_window`
the repo entry stays 900 and the workspace total is 1400.

**F2 — `list_since` is deployment-wide in every adapter**
(`crates/gyre-adapters/src/sqlite/budget_call.rs`,
`crates/gyre-adapters/src/postgres/budget_call.rs`,
`crates/gyre-ports/src/budget_call.rs`): the SQL adapters filtered
`list_since` by the storage handle's tenant scope, but the hourly
resync runs on the BASE handle (tenant scope "default") and recomputes
usage entries for every workspace plus the storage-wide
`tenant:global` ceiling. In any DB-backed deployment with a workspace
whose tenant != "default", the job zeroed that workspace's
`tokens_used_today`/`cost_today` hourly — daily budgets reset hourly,
80%/100% enforcement stopped binding (fail-open). Both SQL adapters
now implement `list_since` unscoped, matching the mem adapter; the
port contract documents the deployment-wide scope. New SQL-adapter
test `list_since_spans_tenants_inside_the_window` saves a record
under a tenant different from the handle's scope and requires it
returned.

### Test evidence

- `cargo test -p gyre-adapters budget_call` — 5 passed (incl. the new
  mismatched-tenant test); output in
  `/tmp/stage/review-evidence/task119-adapters-budget-call-tests.txt`.
- `cargo check -p gyre-server --tests` — the new server-side
  regression test type-checks; the build wrapper blocks gyre-server
  test compilation in this sandbox (4 GiB budget), so the verifier
  must run `cargo test -p gyre-server --lib api::budget` (see
  `/tmp/stage/review-evidence/task119-README.txt`).
- rustfmt clean on all changed files; `scripts/check-arch.sh` and
  `scripts/check-mem-port-contracts.sh` pass; clippy reports no
  warnings in changed files.
