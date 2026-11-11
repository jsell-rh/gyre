# Review — task-119: Implement budget enforcement cascade

Round: 3 (re-review of the lifetime-ceiling repair `bfb2dc3de`).
Candidate: `08235796cd756bd235a352957f3927ff8a06ba6b` (base `cba416b4`).
Evidence: `/tmp/stage/review-evidence/summary.md` (probe commands, sources,
exit codes, outputs).

## Verdict (round 3)

Approved. Rounds 1–2 findings (BudgetExhausted inbox delivery, repo
concurrent/lifetime override enforcement, priority-7 spec reconciliation,
persona-only `system://persona`, and the 9af67822 F1 lifetime-ceiling hole)
are repaired in the candidate with regression tests that target the exact
repros. This round found no new blocking defect.

## Independent evidence

- **Probes run in this sandbox** (all exit 0): 13 mechanical invariant scripts
  (arch, ABAC route registry, dead message kinds — BudgetWarning/
  BudgetExhausted emitters real, exemptions shrunk —, mem port contracts,
  migration versions, migration SQL portability, inert enforcement,
  in-memory state stores, MCP write tools, exempt handlers, relative-path
  defaults, forged scope fields, lossy secret conversion);
  `cargo check -p gyre-server`; `cargo check -p gyre-server --lib --tests`
  (candidate test code type-checks); `cargo test -p gyre-adapters --lib
  budget` (6 passed: list_since window scan, compute-minute accumulation,
  repo budget_json round-trip through create/update/clear);
  `cargo test -p gyre-domain`.
- **Commit attribution**: passes at the candidate commit (verified in a
  throwaway worktree; the working-tree failure is the stage harness's reset
  of `specs/tasks/task-119.md` to the assignment template, present before
  review began). All three product commits (4bfdf85b8, 7bdc60603, bfb2dc3de)
  are listed in the task frontmatter.
- **Verifier-must-run** (this sandbox's build wrapper refuses gyre-server test
  binaries at the 4 GiB budget — infrastructure, not a code defect):
  `cargo test -p gyre-server --lib budget`, `--lib repo_budget`,
  `--lib lifetime`, `--lib mcp_prompts`, `--lib record_agent_usage`, and the
  full `cargo test --all` gate.

## Contract check (agent-runtime.md §4 + §5)

- **Cascade repo ≤ workspace ≤ tenant**: real. Workspace ≤ tenant validated on
  PUT for all four `BudgetConfig` fields (400). Repo ≤ workspace validated on
  `PUT /api/v1/repos/:id` against the EFFECTIVE workspace limit (per-field min
  of workspace config and tenant ceiling; a dimension the workspace leaves
  unset inherits the tenant — the 9af67822 F1(a) corner is a 400 with
  non-persistence asserted). Enforcement recomputes the effective limits live
  on every pass, so post-hoc ceiling reductions bind immediately.
- **max_agents ≤ workspace max_agents_per_repo**: shipped by task-118,
  verified present (validation on PUT, scheduler enforcement with fallback).
- **Usage aggregated per repo, rolled up to workspace + tenant**: real —
  `record_llm_call_usage` is the single funnel for agent reports and
  user-initiated queries; repo entries keyed `repo:{id}`, workspace/tenant
  counters incremented; `POST /api/v1/agents/:id/usage` resolves the repo from
  `agent.repo_id` or the current task.
- **80% warn**: `AgentBudgetWarning` notification (High) to the spawning user
  + workspace Admins with per-period dedupe, plus a `BudgetWarning` bus event
  on the level crossing; agents continue. Spec row amended from the stale
  "Inbox priority 7" draft wording to the canonical `user-management.md`
  taxonomy (the notification system has no priority-7 scale) — legitimate
  spec reconciliation, applied consistently in agent-runtime.md and HSI.
- **100% graceful stop**: `BudgetExhausted` to every active agent's inbox
  (`Destination::Agent`, the surface `list_unacked` serves — round-1 repair),
  one 60s grace timer per workspace with re-evaluation before killing, then
  `ComputeTarget::kill_process()` via the spawn-recorded backend; agents
  completing in grace go Idle, killed agents are Stopped with worktree
  preserved and task InProgress.
- **Beyond limit**: immediate synchronous kill (usage > limit, e.g. budget
  reduced below usage on PUT — asserted end-to-end through the PUT route).
- **compute_minutes**: charged on every terminal transition (complete REST +
  MCP, fail, stop, admin kill, stale-detector death, budget kill) from
  `spawned_at`, whole minutes, to workspace + tenant + repo entries; migration
  000061 column, all three adapters.
- **llm_query charged to workspace**: verified on briefing/ask (graph),
  explorer/generate, specs/assist, MCP spec_assist.
- **Budget reset**: `apply_rolling_24h_window` recomputes every entity's
  token/cost counters from per-call records inside `timestamp ≥ now−24h`
  (new `list_since` port method, sqlite + PG + mem), hourly job in both the
  JobRegistry and the lib.rs background loop — rolling window, midnight-UTC
  reset removed.
- **Prompt structure §5 + MCP prompts**: `GYRE_PROTOCOL_PROMPT`
  (system-managed: Ralph loop, MCP tools, 60s heartbeat, escalation,
  completion) injected alongside `GYRE_META_SPEC_PROMPT`; spawn record pins
  `rendered_prompt` = persona section only; four real MCP prompts
  (`system://persona|protocol|context|constraints`), agent-scoped, prompts
  capability advertised; end-to-end test asserts persona is served verbatim
  and does NOT include the protocol text.

## Non-blocking observations (for future rounds, not defects)

1. Orchestrator spawn surfaces (`api/orchestrator.rs`) run only the
   workspace-level `check_spawn_budget`; the repo-level override is not
   consulted before spawning the singleton-per-repo orchestrator. Its usage
   still charges the repo entry and the workspace ceiling still binds; worker
   spawns remain gated by `check_repo_spawn_budget`.
2. `specs/assist` llm_query charging carries a repo_id (pre-existing task-190
   wiring, unchanged by this candidate), so it also creates a repo usage
   entry; coverage row 30's "no repo entry" wording over-generalizes. The
   acceptance criterion (charged to workspace) holds everywhere.
3. Server test binaries exceed this sandbox's 4 GiB build budget — the
   verifier must execute the recorded probe list above.
