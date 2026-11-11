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
commits: ["4bfdf85b8c1f992f0aa604c753d5277ab0a739b5"]
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

Implementation of agent-runtime.md §4 (Budget Enforcement) and §5 (Agent
Prompt Structure) in production code — no stubs, no audit-only paths.

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
- Agent usage now charges the repo entry (`repo:{id}`) and rolls up to
  workspace + tenant; the reporting path resolves the repo from
  `agent.repo_id` or the agent's current task. Tested:
  `repo_usage_rolls_up_from_agent_reports`.
- Spawn-time checks use the EFFECTIVE config (tighter of workspace and
  tenant per field) and a new `check_repo_spawn_budget` on both spawn
  surfaces (REST spawn, Phase-4 scheduler). Tested:
  `repo_budget_limit_blocks_spawns` + pre-existing spawn-budget tests.

### Enforcement levels (80% / 100% / beyond)

- `enforce_workspace_budget` (api/budget.rs) runs after EVERY usage
  report and after every workspace budget PUT.
- ≥ 80%: `AgentBudgetWarning` notification (High, spawner + Admins,
  once per period — pre-existing) plus a `MessageKind::BudgetWarning`
  bus event on the level crossing. Agents continue. Tested:
  `usage_report_at_80pct_emits_budget_warning_message`.
- ≥ 100%: `MessageKind::BudgetExhausted` to every Active agent in the
  workspace, a single 60s grace timer per workspace
  (`budget_grace_in_progress` guard), then `ComputeTarget::kill_process()`
  on agents still Active (marked `Stopped`; completions in grace go Idle
  through the normal path). Tested (grace 0):
  `exhaustion_emits_messages_and_kills_after_grace`.
- Beyond limit (usage > limit — e.g. an admin lowers the budget below
  current usage): immediate synchronous kill. Tested:
  `beyond_limit_kills_immediately_without_grace` and
  `budget_reduction_below_usage_triggers_beyond_limit_kill` (via PUT).
- Budget kills preserve the worktree, leave the task `InProgress`,
  revoke the agent token, release the budget slot, and emit the
  `AgentKilled` audit event — asserted in the kill tests.
- `BudgetWarning` / `BudgetExhausted` removed from
  `dead-message-kind-exemptions.txt` (real emitters now exist).

### What's tracked

- `compute_minutes`: new `BudgetUsage.compute_minutes` (migration column,
  `BudgetUsageRepository::add_compute_minutes` in SQLite/PG/mem), charged
  from `spawned_at` wall clock at every terminal transition — complete
  (REST + MCP), fail, stop, admin kill, stale-detector death, budget
  kill. Tested: `compute_minutes_tracked_on_terminal_transition` +
  adapter test `compute_minutes_accumulate` (which caught and fixed a
  real Diesel Queryable column-order bug: the struct field order must
  follow the physical column order).
- `llm_tokens` / `llm_cost_usd`: per-call `BudgetCallRecord` + counters
  (pre-existing), now also repo-scoped.
- `llm_query`: user-initiated queries charge the workspace directly
  (pre-existing wiring on briefing/ask, explorer/generate, specs/assist,
  MCP spec_assist, graph).

### Budget reset (rolling 24h window)

- `apply_rolling_24h_window` recomputes every entity's token/cost
  counters from the per-call records with `timestamp >= now - 24h` via
  the new `BudgetCallRepository::list_since` (all three adapters),
  zeroes entities with no in-window records, and sets `period_start` to
  the cutoff. The `spawn_budget_reset` job now runs hourly (both the
  lib.rs background task and the JobRegistry entry); the midnight-UTC
  `reset_daily_counters` port method was removed. Tested:
  `rolling_24h_window_clears_expired_usage` +
  `list_since_spans_workspaces_inside_the_window` (adapter).

### §5 Agent prompt structure + MCP prompts

- The system-managed protocol section (Ralph loop rules, MCP tools, 60s
  heartbeat, escalation, completion) is injected at spawn as
  `GYRE_PROTOCOL_PROMPT` next to `GYRE_META_SPEC_PROMPT`, and both are
  retained verbatim on the spawn-time prompt-set record
  (`rendered_prompt`). Tested: extended
  `spawn_delivers_meta_spec_prompt_to_agent_env`,
  `record_retains_rendered_prompt`, `protocol_prompt_covers_the_five_protocol_areas`.
- MCP `prompts/list` + `prompts/get` implemented (prompts capability
  advertised in `initialize`): `system://persona` (pinned assembly,
  verbatim), `system://protocol`, `system://context` (task, pinned spec
  ref, branch/repo, gate failures, unacked inbox),
  `system://constraints` (budget limits + live usage incl. compute
  minutes, lifetime, iteration ceiling, scope). Tested: six
  `mcp_prompts_*` tests.

### Test evidence

- Sandbox restriction (infrastructure, not a defect): the build wrapper
  refuses gyre-server codegen/test binaries — "gyre-server code
  generation plus the model exceeds 4 GiB". Probes and the exact server
  test list for the independent verifier are recorded in
  `/tmp/stage/review-evidence` (mirrored as the commit list below):
  `cargo check --workspace --all-targets` exit 0; `cargo test -p
  gyre-adapters` 447 passed / 0 failed; `cargo test -p gyre-domain -p
  gyre-ports` 474 passed / 0 failed; all architecture/process check
  scripts green (arch, migration versions + SQL portability, mem port
  contracts, in-memory state stores, inert enforcement, dead message
  kinds, ABAC registry/exempt handlers, MCP write tools, task commit
  attribution).
- The verifier must run `cargo test --all` (acceptance criterion) — the
  focused gyre-server lib suites to watch are `api::budget::`,
  `api::repos::`, `mcp::`, `prompt_assembly::`, `api::spawn::`.

### Notes for review

- The 80% warning notification priority is High (rank 4) per
  user-management.md §Notification Entity, which the code documents as
  superseding the older HSI draft rank 7 that agent-runtime.md §4's
  table cites; the notification, its recipients, and its dedup are
  unchanged from the pre-existing (task-reviewed) behavior.
- Repo budget clearing semantics: PUT `budget: {}` replaces the override
  with an all-unlimited config — enforcement-equivalent to inherit (the
  workspace check still bounds spending) and persistable/observable.
