# Review — task-119 (Implement budget enforcement cascade)

Spec: `specs/system/agent-runtime.md` §4 (Budget Enforcement) + §5 (Agent Prompt Structure).
Candidate: `7ecc3b9c7b1cf46c1f9fd8f9af8dd76505888042` (base `7e9b3cfee`).
Commits under review: 4bfdf85b8, 7bdc60603, bfb2dc3de, 29d1a9618, dc09d6950, d00113b22
(all recorded in the candidate's task-119.md `commits:` frontmatter).
Verdict: **approved** — real implementations for every coverage section, no fakery found;
non-blocking observations recorded below for follow-up.

## Probes run (independent, this review)

- `cargo test -p gyre-adapters budget` → **7 passed** (incl. `list_since_spans_tenants_inside_the_window`,
  `compute_minutes_accumulate`, `repo_budget_override_persists`).
  Evidence: `task119-adapters-budget-tests-mine.txt`.
- `cargo check -p gyre-server --tests` → **clean** (all new server tests type-check).
  Evidence: `task119-server-check.txt`.
- `cargo test -p gyre-domain` → passed.
- `cargo test -p gyre-server --lib api::budget` → **blocked by the build wrapper** (gyre-server
  test compilation exceeds the sandbox 4 GiB budget — infrastructure, not a code defect).
  The verifier MUST run the server suites listed below.
- Mechanical gates, all green: check-arch, check-migration-versions (000061 is the next unused
  sequence), check-migration-sql-portability, check-dead-message-kinds (BudgetWarning/
  BudgetExhausted emitters now real — exemptions removed, count reduced), check-abac-route-registry,
  check-mem-port-contracts, check-inert-enforcement, check-mcp-write-tools, check-in-memory-state-stores,
  check-fabricated-scope-defaults, check-scope-literal-defaults, check-unbounded-external-http.
- check-task-commit-attribution: FAILS **only against the working tree**, whose task-119.md was
  reset to the assigned contract (`commits: []`) by the executor's checkpoint staging. At the
  candidate commit itself the frontmatter lists all six SHAs; verified via
  `git show 7ecc3b9c7:specs/tasks/task-119.md`. Not a candidate defect.

## Verified working

- **Cascade validation is real and complete.** PUT /workspaces/:id/budget validates all four
  BudgetConfig fields against the tenant ceiling (the candidate extends this to
  `max_agent_lifetime_secs`, which the base missed). PUT /repos/:id validates every set repo
  budget field against the EFFECTIVE workspace limit (min of workspace config and tenant
  ceiling — a dimension the workspace leaves unset inherits the tenant), 400 + not-persisted on
  violation; `{}` clears the override. Repo `max_agents` ≤ workspace `max_agents_per_repo`
  (task-118, still present). Persistence is real: migration 000061 (`budget_json`,
  `compute_minutes`) is portable standard SQL, round-trip tested on SQLite.
- **Enforcement levels are real kills, not audit theater.** `enforce_workspace_budget` runs
  after every usage report and every workspace budget PUT: ≥80% → AgentBudgetWarning
  notification + BudgetWarning bus event (level-crossing gated); ≥100% → BudgetExhausted
  delivered to every Active agent's inbox (`Destination::Agent`, the surface `list_unacked`
  serves — verified against the mem adapter's destination filters) + 60s grace timer (deduped
  per workspace, re-evaluates before killing, fails CLOSED on usage-read error) then
  `ComputeTarget::kill_process()` via the spawn-recorded backend; beyond-limit (usage >
  limit under BudgetChanged) → immediate synchronous kill. Kills preserve worktrees, leave
  the task InProgress, revoke the agent token, and emit the AgentKilled audit event.
  Overshooting usage reports (950→1150) correctly route to the graceful-stop branch, not the
  hard kill — regression-tested.
- **Tracking is real.** Per-call BudgetCallRecords + workspace/repo/tenant counters;
  compute_minutes charged on every terminal path (REST complete, MCP complete, fail, stop,
  admin kill, stale-detector death, budget kill); llm_query charged to workspace only with
  no repo entry — enforced in BOTH the incremental path and the hourly rolling-window resync
  (the resync originally re-charged llm_query into repo entries; fixed in d00113b22 with a
  mixed-sequence regression test). Repo resolution for workers goes through the current task
  when agent.repo_id is unset.
- **Budget reset is a real rolling window.** `apply_rolling_24h_window` recomputes counters
  from per-call records ≥ now−24h (hourly job, both the lib.rs background task and the
  JobRegistry registration replaced the midnight-UTC full reset). `list_since` is deliberately
  deployment-wide in all three adapters (SQL adapters originally tenant-scoped the query,
  zeroing non-default tenants hourly — fail-open; fixed with mismatched-tenant adapter tests).
- **§5 prompt structure + MCP prompts are real.** GYRE_PROTOCOL_PROMPT injected at spawn;
  the prompt-set record pins the rendered PERSONA section only; four MCP prompts
  (`prompts/list`, `prompts/get`, capability advertised) serve agent-scoped content —
  system://persona verbatim from the pinned record (end-to-end test asserts it contains the
  seeded persona and NOT the protocol text), system://protocol the system-managed protocol,
  system://context the task/spec-ref/branch/inbox assembly, system://constraints budget
  limits + live usage.
- **Spec amendments are legitimate consistency fixes, not reinterpretation:** the 80% row's
  "Inbox priority 7" draft wording conflicted with user-management.md's unified taxonomy
  (4-level priorities; AgentBudgetWarning = High, numeric rank 4 in gyre-common's implemented
  taxonomy). Both agent-runtime.md and human-system-interface.md updated consistently.
- **Test quality:** the new tests drive production functions and the real router, and assert
  discriminating outcomes (grace vs hard kill, worktree preserved, task InProgress, inbox vs
  workspace-stream destinations, repo vs workspace charging). No self-confirming or
  stub-backed tests found.

## Non-blocking observations (recorded for follow-up, none violate the task contract)

1. **Tenant-aggregate concurrent check removed.** Base `check_spawn_budget` compared the
   deployment-wide tenant `active_agents` against the tenant config's `max_concurrent_agents`;
   the candidate compares per-workspace usage against min(workspace, tenant) per field. Each
   workspace is now bounded by the tenant ceiling (tighter than base for tokens/cost, which the
   base never bounded per workspace), but N workspaces can run N× the tenant concurrent cap in
   aggregate. The spec's cascade text bounds per-entity configs ("Workspace budget cannot
   exceed tenant") and its enforcement text is per-workspace ("aggregates usage per workspace
   and checks against workspace limits"), and the tenant config has no API write path today
   (seed-only), so this is a defensible model — but the aggregate hole on the concurrent
   dimension is worth a follow-up if tenant ceilings become operator-settable.
2. **Orchestrator spawn surfaces skip `check_repo_spawn_budget`.** `api/orchestrator.rs`
   (workspace + repo orchestrator spawn) checks only the workspace budget. A repo-orchestrator
   spawn can exceed a tighter repo budget override (bounded by the one-live-orchestrator guard
   and the workspace ceiling; its usage still charges the repo entry). The two primary spawn
   surfaces (REST worker spawn + Phase-4 scheduler) both check.
3. **stop/fail on a completed (Idle) agent double-charges compute minutes** (Idle→Stopped/
   Failed are valid transitions): `complete_agent` charges spawn→complete, then `stop_agent`
   charges spawn→stop again. Mirrors the pre-existing unconditional `decrement_active_agents`
   in the same handlers; tracking-only field, no limit consumes it. `budget_stop_agent`
   correctly guards on Active.
4. **`budget_grace_in_progress` is process-local** (documented deliberate): a restart during
   the 60s grace window drops the pending kill until the next usage report re-enters
   enforcement (usage is still ≥ limit). Passes check-in-memory-state-stores (sync guard,
   not a store).

## Verifier must run (blocked here by the 4 GiB build budget)

- `cargo test -p gyre-server --lib api::budget` — the enforcement suite (grace/beyond-limit/
  warning/lifetime/rolling-window/repo-budget tests).
- `cargo test -p gyre-server --lib api::repos` — cascade validation tests
  (`update_repo_budget_cascade_validated`,
  `repo_budget_validated_against_effective_workspace_limit`).
- `cargo test -p gyre-server --lib mcp` — prompt delivery tests.
- `cargo test -p gyre-server --lib prompt_assembly` — protocol prompt + record retention.
- `cargo test -p gyre-server --lib api::spawn` — GYRE_PROTOCOL_PROMPT injection +
  `mcp_persona_prompt_serves_meta_spec_section_only`.
- `cargo test --all` — the task's final acceptance criterion.
