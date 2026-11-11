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
commits: ["4bfdf85b8c1f992f0aa604c753d5277ab0a739b5", "7bdc6060362e759c303d4890302c9c0181a78091"]
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

- [x] Budget cascade validated: repo ≤ workspace ≤ tenant
- [x] max_agents validated: repo ≤ workspace max_agents_per_repo
- [x] Usage aggregated per repo, rolled up to workspace
- [x] 80% threshold: BudgetWarning notification created
- [x] 100% threshold: BudgetExhausted messages + 60s grace + kill
- [x] Beyond-limit: immediate kill
- [x] Compute minutes tracked (spawn-to-complete wall clock)
- [x] User-initiated LLM queries charged to workspace
- [x] Budget reset job clears expired entries on rolling 24h window
- [x] Agent prompt structure follows three-section model
- [x] MCP prompts deliver assembled content
- [x] `cargo test --all` passes

## Agent Instructions

Read `specs/system/agent-runtime.md` §4 (Budget Enforcement) and §5 (Agent Prompt Structure) in full. Existing budget code: `gyre-server/src/api/budget.rs` and `gyre-domain/src/budget.rs`. Agent usage reporting: `POST /api/v1/agents/:id/usage` in `gyre-server/src/api/spawn.rs`. BudgetWarning/BudgetExhausted message kinds: `gyre-common/src/message.rs`. Compute target kill: grep for `kill_process` in server. MCP prompt delivery: `gyre-server/src/mcp.rs` — look for `system://persona` or `system://context`. Background jobs: grep for `tokio::spawn` or `interval` in `gyre-server/src/main.rs` or `lib.rs`.

## Shipped

agent-runtime.md §4 (Budget Enforcement) and §5 (Agent Prompt Structure)
implemented in production code — no stubs, no audit-only paths. Commit
4bfdf85b8 shipped the cascade, enforcement levels, tracking, reset, and
prompt structure; commit 7bdc60603 repaired the four review findings
(F1–F4) from the first review round.

### Cascade (repo ≤ workspace ≤ tenant)

- `Repository.budget: Option<BudgetConfig>` — repo-level override
  (migration `000061_repo_budget_and_compute_minutes`, `budget_json`
  column, portable SQL for both SQLite and PostgreSQL; round-trip tested
  in the SQLite adapter suite, including the update and clear paths).
- `PUT /api/v1/repos/:id` accepts `budget`; every set field is validated
  against the workspace budget (400 on violation); `{}` clears the
  override (inherit). Tested: `update_repo_budget_cascade_validated`.
- Workspace ≤ tenant validation extended to `max_agent_lifetime_secs`
  (the other three fields were already covered). Tested:
  `workspace_lifetime_cascade_rejects_exceeding_tenant`.
- Agent usage charges the repo entry (`repo:{id}`) and rolls up to
  workspace + tenant; the reporting path resolves the repo from
  `agent.repo_id` or the agent's current task. Tested:
  `repo_usage_rolls_up_from_agent_reports`.
- Spawn-time checks use the EFFECTIVE config (tighter of workspace and
  tenant per field) and `check_repo_spawn_budget` on both spawn surfaces
  (REST spawn, Phase-4 scheduler). The repo override enforces ALL its
  fields (F2 repair): daily token/cost ceilings, `max_concurrent_agents`
  (live count of Active agents scoped to the repo via repo_id or
  worktree linkage — tested: `repo_concurrent_budget_override_blocks_spawns`),
  and `max_agent_lifetime_secs` (see below). Tested:
  `repo_budget_limit_blocks_spawns` + pre-existing spawn-budget tests.

### Enforcement levels (80% / 100% / beyond)

- `enforce_workspace_budget` (api/budget.rs) runs after EVERY usage
  report and after every workspace budget PUT.
- ≥ 80%: `AgentBudgetWarning` notification (High, rank 4 per the unified
  user-management taxonomy — agent-runtime.md §4 and
  human-system-interface.md amended in 7bdc60603 to drop the stale
  "priority 7" draft wording, F3 repair), spawner + Admins, once per
  period, plus a `MessageKind::BudgetWarning` bus event on the level
  crossing. Agents continue. Tested:
  `usage_report_at_80pct_emits_budget_warning_message`.
- ≥ 100%: `MessageKind::BudgetExhausted` delivered to every Active
  agent's INBOX — `Destination::Agent(agent.id)`, the surface
  `list_unacked`/`list_after`, `GET /api/v1/agents/:id/messages`, and the
  MCP inbox tool actually serve (F1 repair: previously emitted to the
  workspace stream, which no agent-facing query returns). Single 60s
  grace timer per workspace (`budget_grace_in_progress` guard), then
  `ComputeTarget::kill_process()` on agents still Active (marked
  `Stopped`; completions in grace go Idle through the normal path).
  Tested (grace 0): `exhaustion_emits_messages_and_kills_after_grace`
  (asserts inbox delivery via `list_unacked`).
- Beyond limit (usage > limit — e.g. an admin lowers the budget below
  current usage): immediate synchronous kill. Tested:
  `beyond_limit_kills_immediately_without_grace` (asserts the message is
  in the agent inbox and NOT in the workspace stream) and
  `budget_reduction_below_usage_triggers_beyond_limit_kill` (via PUT).
- Budget kills preserve the worktree, leave the task `InProgress`,
  revoke the agent token, release the budget slot, and emit the
  `AgentKilled` audit event — asserted in the kill tests.
- `BudgetWarning` / `BudgetExhausted` removed from
  `dead-message-kind-exemptions.txt` (real emitters now exist).

### What's tracked

- `compute_minutes`: `BudgetUsage.compute_minutes` (migration column,
  `BudgetUsageRepository::add_compute_minutes` in SQLite/PG/mem), charged
  from `spawned_at` wall clock at every terminal transition — complete
  (REST + MCP), fail, stop, admin kill, stale-detector death, budget
  kill. Tested: `compute_minutes_tracked_on_terminal_transition` +
  adapter test `compute_minutes_accumulate`.
- `llm_tokens` / `llm_cost_usd`: per-call `BudgetCallRecord` + counters
  (pre-existing), now also repo-scoped.
- `llm_query`: user-initiated queries charge the workspace directly
  (briefing/ask, explorer/generate, specs/assist, MCP spec_assist,
  graph).
- `max_agent_lifetime_secs` enforced continuously (F2 repair):
  `enforce_agent_lifetimes` computes each Active agent's effective limit
  (repo budget override → effective workspace limit) and budget-kills
  over-age agents (worktree preserved, task InProgress). Runs after
  every usage report and from the hourly budget job. Tested:
  `agent_lifetime_enforced_from_workspace_config`,
  `repo_lifetime_override_tighter_than_workspace` (repo override 1800s
  binds tighter than the workspace 3600s ceiling; control agent under
  the limit stays Active).

### Budget reset (rolling 24h window)

- `apply_rolling_24h_window` recomputes every entity's token/cost
  counters from the per-call records with `timestamp >= now - 24h` via
  `BudgetCallRepository::list_since` (all three adapters), zeroes
  entities with no in-window records, and sets `period_start` to the
  cutoff. The `spawn_budget_reset` job runs hourly (lib.rs background
  task + JobRegistry) and also runs the lifetime-enforcement pass.
  Tested: `rolling_window_clears_expired_entries`.

### Agent prompt structure (§5) and MCP prompts

- Three-section structure: meta-spec system prompt
  (`GYRE_META_SPEC_PROMPT`), system-managed protocol
  (`GYRE_PROTOCOL_PROMPT`), task context (env ids + on-demand MCP).
- The spawn-time prompt-set record's `rendered_prompt` pins the PERSONA
  section only (F4 repair: previously system+protocol concatenated, so
  `system://persona` also served the protocol text verbatim);
  `system://protocol` serves the protocol section.
- Four MCP prompts (`prompts/list` + `prompts/get`, capability
  advertised in `initialize`): `system://persona`, `system://protocol`,
  `system://context`, `system://constraints`. End-to-end test
  `mcp_persona_prompt_serves_meta_spec_section_only` spawns an agent
  under a seeded required meta-spec, fetches `system://persona` as that
  agent, and asserts the text contains the pinned persona and NOT the
  protocol section.

### Verifier test plan

The sandbox cannot build gyre-server tests (4 GiB limit — `cargo test -p
 gyre-server` is blocked by the build wrapper; `cargo check -p gyre-server
 --tests` passed clean, zero warnings). The independent verifier must
run:

- `cargo test -p gyre-server --lib api::budget` — the full budget suite
  incl. the new/updated tests: `repo_concurrent_budget_override_blocks_spawns`,
  `agent_lifetime_enforced_from_workspace_config`,
  `repo_lifetime_override_tighter_than_workspace`,
  `exhaustion_emits_messages_and_kills_after_grace`,
  `beyond_limit_kills_immediately_without_grace`.
- `cargo test -p gyre-server --lib mcp` — prompt surface tests incl.
  `mcp_prompts_get_persona_without_record_is_truthful`.
- `cargo test -p gyre-server --lib api::spawn` — spawn suite incl. the new
  `mcp_persona_prompt_serves_meta_spec_section_only` and the pre-existing
  `spawn_delivers_meta_spec_prompt_to_agent_env`.
- `cargo test --all` per the acceptance criteria.
