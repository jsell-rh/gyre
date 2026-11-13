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
commits: ["af627d03a43a12ffede27b8bee37a5feaa0748f6", "199e47c4a6db536f0c1a729e76cb6becbfac0803", "d00113b22ab971ea5b5586e6e4841e0ab68f162e", "dc09d6950d1df2d7498448e66642908a747c4b0a", "29d1a9618ee5181319aacccb704397b88242d3c7", "bfb2dc3dec94916312b1688752ce6401b06dc9c0", "7bdc6060362e759c303d4890302c9c0181a78091", "4bfdf85b8c1f992f0aa604c753d5277ab0a739b5"]
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
Structure) are implemented in production code:

- **Cascade validation** (repo ≤ workspace ≤ tenant, effective-limit
  semantics; repo `max_agents` ≤ workspace `max_agents_per_repo`,
  spawn-time enforcement with scheduler queuing).
- **Enforcement levels**: 80% BudgetWarning notification + bus event;
  100% BudgetExhausted-to-inbox + 60s grace timer then kill;
  beyond-limit immediate kill. Worktree preserved, task stays
  InProgress, token revoked, AgentKilled audit.
- **Tracking**: llm_tokens/llm_cost_usd per cred-proxy call (see the
  reporting repair below), compute_minutes charged on every terminal
  transition from spawned_at wall clock, llm_query charged to
  workspace only.
- **Budget reset**: hourly rolling-24h job recomputed from per-call
  records (budget_calls port + SQLite/Postgres adapters).
- **Prompt structure (§5)**: three-section assembly at spawn
  (GYRE_META_SPEC_PROMPT/SET_SHA + GYRE_PROTOCOL_PROMPT +
  GYRE_ITERATION) and four MCP prompts (system://persona,
  ://protocol, ://context, ://constraints) delivering the assembled
  content; system://persona serves the pinned meta-spec set verbatim
  from the spawn-time record, never the protocol section.

### This round — cred-proxy usage reporting + merge repairs (commit af627d03a)

Review b8330483e finding 1: the §4 "What's Tracked" reporting
mechanism had no production caller — cred-proxy.mjs never POSTed
usage, so the enforcement cascade never fired for agent LLM usage.
Fixed in `docker/gyre-agent/cred-proxy.mjs`: after every proxied LLM
call whose response carries a usage block (JSON or buffered SSE, with
cumulative `message_delta` output tokens), the proxy POSTs
`{tokens_input, tokens_output, cost_usd}` to
`POST $GYRE_SERVER_URL/api/v1/agents/$GYRE_AGENT_ID/usage` with the
agent's Bearer token. Cost comes from a model-specific pricing table
(`GYRE_PRICING_JSON` override). Reports are best-effort and
latency-bounded (10 s) — a failed report never fails the LLM call.
Supporting fixes: `GYRE_ANTHROPIC_BASE` upstream override; gyre-server
Bearer injection takes precedence over upstream key injection;
`GYRE_CRED_ALLOWED_HOSTS` read at startup (previously scrubbed before
the per-request read could see it — dead configuration);
`GYRE_CRED_SOCK_PATH` socket override. Five integration tests run the
real proxy child against a fake Anthropic upstream and a fake gyre
server (`docker/gyre-agent` `npm test`, 17/17 pass in-sandbox).

The merge of base 44a01390 into this branch left two semantic
conflicts that broke compilation; both repaired in the same commit:
`MemBudgetCallRepository::list_since` used unqualified
`BudgetCallRecord` (mem.rs), and six `api::budget.rs` test
initializers lacked main's new `prompt_template_sha` field
(task-171).

### Test evidence

- `npm test` in `docker/gyre-agent` — 17/17 pass, exit 0 (includes
  the five new usage-reporting integration tests). Log:
  `/tmp/stage/review-evidence/gyre-agent-npm-test.log`.
- `cargo check -p gyre-server --lib --tests` and
  `cargo check --workspace --all-targets` — clean, exit 0, at this
  round's head (evidence:
  `/tmp/stage/review-evidence/cargo-check-and-verifier-tests.md`).
- Verifier must execute (gyre-server test compilation blocked in
  sandbox under the 4 GiB budget): `cargo test --all`; focused
  `cargo test -p gyre-server --lib api::budget::tests` (task-119
  enforcement suite; also proves the prompt_template_sha merge
  repair compiles+passes) and `cargo test -p gyre-server --lib
  api::spawn::tests::mcp_persona_prompt_serves_meta_spec_section_only`
  (commit 199e47c4a repair; the prior full-suite failure at candidate
  8a7571754, durable finding a3663ce1, has zero recorded executions
  since the fix).
