# Review — task-119 (Implement budget enforcement cascade)

Spec: `specs/system/agent-runtime.md` §4 (Budget Enforcement: Cascade,
Enforcement Levels, What's Tracked, Budget Reset) and §5 (Agent Prompt
Structure). Coverage rows 27–32.
Candidate under review: `ef57059cad550146fbcc8ae4bfff1fd6d1b7c580`
(base `f98b179253b9e3f12cbc9a165532cacf709244d9`).
Verdict: **needs-revision** — one material finding (F1, missing production
usage-reporting trigger behind an "implemented" coverage row); one
verification requirement (F2, the suite's last recorded run failed and the
repair is unexecuted). Everything else in the diff verified real.

## Probes run (review checkout at candidate head)

- `cargo test -p gyre-adapters --lib budget_call` → **5/5 passed** (SQLite:
  list_since window/order/limit, tenant-spanning, round-trip, duplicate-id).
- `cargo test -p gyre-adapters --lib budget` → **7/7 passed** (adds
  `kv_store::budget_tests::compute_minutes_accumulate` — kills counter
  clobbering — and `repository::tests::repo_budget_override_persists`).
- `cargo check -p gyre-server --lib --tests` → **clean** (7m19s; server test
  binaries exceed the sandbox 4 GiB budget, so test execution is delegated
  to the verifier per the review-stage rules).
- Mechanical gates all green at the head: check-arch, check-migration-versions,
  check-abac-route-registry, check-mcp-write-tools, check-dead-message-kinds,
  check-migration-sql-portability, check-mem-port-contracts,
  check-abac-exempt-handlers, check-in-memory-state-stores,
  check-inert-enforcement, check-unbounded-external-http,
  check-forged-scope-fields, check-forwarded-header-trust.
- Diff inspection of the full base..candidate range (budget.rs +2043,
  repos.rs, spawn.rs, mcp.rs, jobs.rs, lib.rs, mem.rs, adapters, migration,
  prompt_assembly.rs, docs, coverage).
- Evidence: `/tmp/stage/review-evidence/`.

## Verified working (no findings)

- **Cascade validation is real.** Workspace ≤ tenant on PUT
  `/api/v1/workspaces/:id/budget` for all four `BudgetConfig` fields (400);
  repo ≤ **effective** workspace limit (min of workspace config and tenant
  ceiling — a dimension the workspace leaves unset inherits the tenant) on
  PUT `/api/v1/repos/:id`, every field, `{}` clears → inherit; live
  recomputation so a later ceiling reduction re-binds persisted overrides
  (tested: `update_repo_budget_cascade_validated`,
  `repo_budget_validated_against_effective_workspace_limit`). Repo
  `max_agents` ≤ workspace `max_agents_per_repo` pre-existed (task-118) and
  still holds.
- **Enforcement levels are real.** `enforce_workspace_budget` runs after
  every usage report and every budget PUT: ≥80% → AgentBudgetWarning
  notification (High/rank 4, spawner + workspace Admins, per-period dedup)
  + BudgetWarning bus event on the crossing; ≥100% → BudgetExhausted to
  every Active agent's inbox (`Destination::Agent`) + one 60s grace timer
  per workspace (`budget_grace_in_progress` guard), then kill via
  `ComputeTarget::kill_process()` on the spawn-recorded backend, marked
  Stopped, worktree preserved, task InProgress, token revoked, AgentKilled
  audit; usage-report overshoot correctly gets grace (cause-aware), and
  budget-reduced-below-usage (BudgetChanged) hard-kills synchronously
  (tested end-to-end through the PUT route:
  `budget_reduction_below_usage_triggers_beyond_limit_kill`). The
  "priority 7" → High/rank-4 spec amendment is a faithful reconciliation
  with user-management.md's notification taxonomy.
- **Tracking is real where it has a trigger.** Per-call BudgetCallRecord +
  workspace/repo/tenant counters (repo charge only for agent_run; llm_query
  charged workspace-only even when the route knows the repo — enforced both
  incrementally and in the rolling-window resync, tested); compute_minutes
  charged from spawned_at wall clock on every terminal transition
  (complete REST+MCP, fail, stop, admin kill, stale death, budget kill),
  persisted via a portable migration (INTEGER NOT NULL DEFAULT 0) and all
  three adapters; user-initiated llm_query paths (briefing/ask, explorer,
  specs/assist, MCP spec_assist, graph) all funnel through
  `record_llm_call_usage`.
- **Budget reset is a real rolling 24h window.** Hourly job recomputes
  counters from per-call records ≥ now−24h via the new unscoped
  `BudgetCallRepository::list_since` (mem/SQLite/PG all implement it
  unscoped — tenant-spanning behavior tested against real SQLite); zeroes
  entities with no in-window records; replaces the old midnight-UTC
  calendar reset (port method `reset_daily_counters` removed everywhere).
- **§5 prompt structure is real.** GYRE_META_SPEC_PROMPT/SET_SHA +
  GYRE_PROTOCOL_PROMPT (system-managed five protocol areas) + GYRE_ITERATION
  injected at spawn; the spawn-time record pins `rendered_prompt` (persona
  section ONLY — repair from an earlier round); four MCP prompts serve
  persona (pinned set verbatim), protocol, context (task/spec/branch/gate
  failures/inbox), constraints (budget + usage + lifetime + iteration).
  BudgetWarning/BudgetExhausted removed from the dead-message-kind
  exemption list — emitters are real.
- **Repo budget storage is real.** `repositories.budget_json` (nullable
  TEXT) with corrupt-JSON fail-safe to inherit (warn, named row), SQLite +
  PG adapters, spawn-time enforcement on both surfaces (REST spawn and the
  Phase-4 scheduler, signal_chain.rs), lifetime ceilings enforced
  continuously from every usage report + the hourly job.

## Findings

- [-] **F1 (major): the spec's usage-reporting mechanism has no production
  implementation, yet §4 "What's Tracked" is marked implemented.**
  agent-runtime.md §4 requires: "The credential proxy (cred-proxy.mjs)
  reports token usage back to the server after each LLM call via
  `POST /api/v1/agents/:id/usage`." `docker/gyre-agent/cred-proxy.mjs`
  (392 lines) never POSTs usage — it proxies requests, exchanges OAuth
  tokens, and attaches `Bearer $GYRE_AUTH_TOKEN` when the target is
  GYRE_SERVER_URL (cred-proxy.mjs:175), but reports nothing; a repo-wide
  grep for callers of the usage endpoint matches only the server's own
  route registration (api/mod.rs:285), docs, and the server's test. No MCP
  tool, CLI command, agent-runner, or entrypoint calls it either. The
  server-side aggregation/enforcement from this task is real, but with
  zero reporters the 80%/100%/beyond-limit cascade never fires for agent
  LLM usage in a production deployment — only the user-initiated llm_query
  paths feed it. Coverage row 30 ("What's Tracked", depth 3) is flipped to
  `implemented` by this candidate without disclosing the gap (the note
  describes the recording counters only). Same defect class this repo
  already flags elsewhere (task-095 R3-F4: "BudgetCallRecord has ZERO
  instantiations" was tracked as a gap).

  **Repair path:** either implement cred-proxy usage reporting (after each
  proxied LLM call, POST the parsed token usage to
  `$GYRE_SERVER_URL/api/v1/agents/$GYRE_AGENT_ID/usage` with the agent
  bearer token, best-effort/fail-open on the reporting side so a server
  hiccup never breaks the LLM call, with a test), or — if the sentence is
  deemed owned elsewhere/outdated — amend the spec via the lifecycle and
  disclose the divergence in coverage row 30 instead of claiming the
  section implemented. Do not leave the row "implemented" with the
  cascade's primary production trigger absent.

- [-] **F2 (verification): `cargo test --all` is unproven at this head.**
  The last recorded full-suite run on this task's work (candidate
  8a7571754) failed exactly one test:
  `api::spawn::tests::mcp_persona_prompt_serves_meta_spec_section_only`
  (assertion 404 != 200 — the test posted `/mcp` to an `api_router()` app,
  but `/mcp` is registered only on `build_router`, lib.rs:884). The repair
  commit `199e47c4a` routes the `prompts/get` call through
  `build_router` over the shared `AppState` — the correct fix, matching
  the established MCP test pattern (mcp.rs:3995) — but it has zero
  recorded executions (the sandbox blocks gyre-server test compilation
  under the 4 GiB budget; the candidate's own notes ship cargo-check-only
  evidence). Verifier must execute:
  `cargo test --all`; focused:
  `cargo test -p gyre-server --lib api::spawn::tests::mcp_persona_prompt_serves_meta_spec_section_only`
  and `cargo test -p gyre-server --lib api::budget::tests`.

## Minor (non-blocking, recorded for completeness)

- Migration `2026-11-13-000061_repo_budget_and_compute_minutes` reuses
  sequence number 000061 (base already had
  `2026-11-12-000061_repo_access_overrides`). The Diesel versions differ
  (date prefix), so both run and `check-migration-versions.sh` is green —
  a convention nit against "next unused 6-digit sequence number", not a
  functional defect.
- The spec's beyond-limit example "tenant admin lowered the tenant
  ceiling" has no production route (no tenant-budget setter exists); the
  reachable path (workspace budget PUT) is the one enforced. Pre-existing
  condition, unchanged by this diff.
- `record_agent_usage` carries no auth extractor (doc comment claims
  agent-scoped JWT matching `:id`) — pre-existing at base, unchanged by
  this diff, and covered by the ABAC exempt-handler gate's current
  baseline.
- `check-task-commit-attribution.sh` fails only against the working tree,
  where the controller reset `specs/tasks/task-119.md` to the assigned
  body (`commits: []`); at the candidate commit all six task-labeled
  product commits are recorded in the frontmatter.

Test-inflation check: none found. The enforcement tests drive the real
router/state (status transitions, inbox delivery, persistence, spawn
blocking) and the adapter tests run against real SQLite; each would fail
if the corresponding production behavior were disabled.
