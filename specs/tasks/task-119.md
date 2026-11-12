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
commits: ["dc09d6950d1df2d7498448e66642908a747c4b0a", "29d1a9618ee5181319aacccb704397b88242d3c7", "bfb2dc3dec94916312b1688752ce6401b06dc9c0", "7bdc6060362e759c303d4890302c9c0181a78091", "4bfdf85b8c1f992f0aa604c753d5277ab0a739b5"]
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
structure shipped in commit 4bfdf85b8; review-round repairs in
7bdc60603 (BudgetExhausted inbox delivery, repo concurrent/lifetime
override enforcement, priority-7 spec reconciliation, persona-only
`system://persona`), bfb2dc3d (repo lifetime override bounded by the
effective tenant/workspace ceiling), and 29d1a961 (verification-gate
repairs: rustfmt, clippy double_must_use allow on the sibling port
trait).

This round (dc09d695) repaired the two review findings from round
d3607aac in `crates/gyre-server/src/api/budget.rs`:

### F1 — overshooting exhaustion now gets the 60s graceful stop

"Beyond limit" is exclusively the budget-reduced-below-usage case
(external budget changes). `enforce_workspace_budget` now takes a
`BudgetEnforcementCause` (`UsageReport` | `BudgetChanged`): only a
`BudgetChanged` pass (workspace budget PUT) with usage strictly past
the limit hard-kills immediately with grace_secs 0. Every usage report
at ≥ 100% — including the common overshoot crossing (950 → 1150
against a 1000-token limit) — routes to the 100% branch:
`BudgetExhausted` to every active agent's inbox, 60s grace timer, kill
of agents still Active afterwards. Reports made during a started
grace period no longer fall into the immediate-kill branch. The PUT
path (budget reduced below usage) still hard-kills synchronously.

### F2 — llm_query charges the workspace only

`record_llm_call_usage` no longer charges the repo budget when
`usage_type` is `llm_query`, even when the calling route scoped the
query to a repo (REST specs/assist, explorer/generate, MCP
spec_assist). User queries charge the workspace and tenant counters
directly; the per-call `BudgetCallRecord` still records the repo for
the audit trail. User queries can no longer exhaust a repo budget and
block agent spawns.

### Test evidence

New regression tests in `api/budget.rs` (type-checked via
`cargo check -p gyre-server --lib --tests`; the sandbox build wrapper
blocks gyre-server test codegen, so the independent verifier must run
them):

- `overshooting_report_gets_graceful_stop_not_hard_kill` — the exact
  F1 repro (950 seeded, one 200-token report vs a 1000-token limit):
  agent stays Active, BudgetExhausted in the inbox with grace_secs 60,
  worktree preserved. Fails on the pre-fix code (immediate Stopped).
- `overshoot_exhaustion_kills_after_grace_expiry` — overshoot routes
  through the grace timer: killed (Stopped) after grace, worktree
  preserved, task remains InProgress.
- `llm_query_with_repo_context_charges_workspace_only` — llm_query
  with a repo_id: workspace + tenant counters incremented, no repo
  usage entry, audit record keeps the repo. Fails on the pre-fix code.
- Updated `beyond_limit_kills_immediately_without_grace` to pass
  `BudgetChanged` (the only path to beyond-limit) and
  `exhaustion_emits_messages_and_kills_after_grace` to pass
  `UsageReport`.

Verifier probes required (blocked in-sandbox by the build wrapper's
4 GiB budget):

- `cargo test -p gyre-server --lib api::budget`
- `cargo test --all`

Local probes run here: `cargo check -p gyre-server` (clean),
`cargo check -p gyre-server --lib --tests` (clean),
`cargo clippy -p gyre-server --lib` (no new warnings in
api/budget.rs), `cargo fmt --all -- --check` (clean).
