# Review — task-119 (Implement budget enforcement cascade)

Spec: `specs/system/agent-runtime.md` §4 (Budget Enforcement: Cascade, Enforcement
Levels, What's Tracked, Budget Reset) + §5 (Agent Prompt Structure).
Candidate under review: `4fe0f07b005ed332ad1d424bdfa22dfa44c0b206` (base
`3af80bd21a8a3452420e014e645eefdfd137f9be`). Product commits: `4bfdf85b8`
(implementation), `7bdc60603`/`bfb2dc3de`/`29d1a9618`/`dc09d6950` (repair rounds).
Verdict: **needs-revision** — two material findings (F1, F2), both in the §4
Budget Reset / What's Tracked sections this task claims as implemented.

## Probes run (this round)

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → clean (no warnings, 5m53s cold).
- `cargo test -p gyre-adapters --lib sqlite::budget_call` → 4/4 passed
  (`list_since_spans_workspaces_inside_the_window`,
  `save_then_list_round_trips_every_field`, ...).
- `cargo test -p gyre-adapters --lib sqlite::repository` → 10/10 passed
  (incl. `repo_budget_override_persists`, `max_agent_limits_persist`).
- Full diff inspection `3af80bd2..4fe0f07b` (budget.rs, repos.rs, spawn.rs,
  jobs.rs, lib.rs, mcp.rs, prompt_assembly.rs, adapters, migration 000061).
- gyre-server test compilation is rejected by the sandbox build wrapper
  (4 GiB budget) — infrastructure restriction, not a code defect. Evidence and
  required verifier probes below.
- Evidence saved under `/tmp/stage/review-evidence/finding1-*` and
  `finding2-*`.

## Verified working (no findings)

- **Cascade validation is real and complete.** Workspace ≤ tenant validated on
  `PUT /api/v1/workspaces/:id/budget` for all four `BudgetConfig` fields (400 on
  violation); repo ≤ **effective** workspace limit (min of workspace config and
  tenant ceiling — a dimension the workspace leaves unset inherits the tenant)
  validated on `PUT /api/v1/repos/:id`, `{}` clears the override. Both backed by
  hard tests that drive the real router and assert non-persistence on violation
  (`update_repo_budget_cascade_validated`,
  `repo_budget_validated_against_effective_workspace_limit`,
  `cascade_validation_rejects_workspace_exceeding_tenant`). Repo budget persists
  through migration `000061` `budget_json` (SQLite round-trip verified by the
  adapter test I ran).
- **Prior round's F1/F2 repairs are present on the enforcement path.**
  `enforce_workspace_budget` distinguishes `BudgetEnforcementCause`: any
  `UsageReport` landing at ≥100% — including the common overshoot (950 → 1150
  vs 1000) — gets BudgetExhausted to every Active agent's inbox
  (`Destination::Agent`) + the 60s grace timer + kill-after-grace; only
  `BudgetChanged` with usage strictly beyond the new limit hard-kills
  immediately (the budget-reduced-below-usage case, exactly as the spec
  defines it). Regression tests route through the production
  `record_llm_call_usage` path (`overshooting_report_gets_graceful_stop_not_hard_kill`,
  `exhaustion_emits_messages_and_kills_after_grace`,
  `beyond_limit_kills_immediately_without_grace`,
  `budget_reduction_below_usage_triggers_beyond_limit_kill`) and assert
  worktree-preserved / task-InProgress recovery semantics. The incremental
  llm_query charge is filtered out of the repo budget
  (`record_llm_call_usage` `usage_type != "llm_query"` guard), with a real
  regression test (`llm_query_with_repo_context_charges_workspace_only`).
- **Kills are real.** `budget_stop_agent` transitions the agent, removes the
  process handle, and calls `ComputeTarget::kill_process` on the backend
  recorded at spawn (docker/kubectl/remote via `spawned_backends`; LocalTarget
  only as the unknown-backend fallback), revokes the token, emits the
  AgentKilled audit event with the attested pid, and charges compute minutes.
- **Spawn-time enforcement on both surfaces.** REST spawn returns 429 for
  budget-exceeded (direct API call); the scheduler path (signal_chain.rs)
  queues the task Backlog with the `BudgetExhausted` label and re-checks — the
  spec's "no 429, scheduler handles queuing internally" applies to that path
  and is honored. Repo override enforces all its fields (tokens/cost ceilings,
  live-counted `max_concurrent_agents`, lifetime via `enforce_agent_lifetimes`
  bounded by the effective workspace/tenant ceiling).
- **Compute minutes** charged from `spawned_at` wall clock on every terminal
  transition (complete REST, fail, stop, budget kill, admin kill, stale
  detector), whole-minute accounting, workspace + tenant + repo entries.
- **Hourly job wiring is real**: `spawn_budget_reset` re-registered at 3600s
  with `run_at_utc_hour: None` (both JobRegistry and the lib.rs background
  loop), replacing the midnight-UTC calendar reset.
- **§5 prompt structure is real.** `GYRE_META_SPEC_PROMPT` (assembled
  meta-specs) + `GYRE_PROTOCOL_PROMPT` (system-managed protocol: Ralph loop
  rules, MCP tools, 60s heartbeat, escalation, `agent.complete`) injected at
  spawn; the prompt-set record pins `rendered_prompt` = persona section only;
  four MCP prompts implemented (`prompts/list` + `prompts/get`):
  `system://persona` serves the pinned assembly verbatim (end-to-end test
  asserts it contains the seeded persona and NOT the protocol text),
  `system://protocol`, `system://context` (task/spec-ref/branch/gate-failure/
  inbox), `system://constraints` (limits + live usage + lifetime + iteration
  ceiling + scope). `prompt_assembly` tests cover ordering, dedup, pinned
  versions, approval gating.
- **Spec amendment** (80% row: "Inbox priority 7" → "High, rank 4 in the
  unified taxonomy") is a documented spec-lifecycle reconciliation with
  `user-management.md` §Notification Entity and matches the pre-existing
  `NotificationType::default_priority_rank` — accepted, not a finding.

## Findings

- [major] **F1 (agent-runtime.md §4 What's Tracked): the hourly rolling-window
  resync re-charges `llm_query` usage into the repo budget — the prior round's
  F2 repair only covers the incremental path.** `record_llm_call_usage`
  (crates/gyre-server/src/api/budget.rs:569) persists the `BudgetCallRecord`
  with `repo_id` unconditionally (specs/assist at api/specs_assist.rs:465 and
  MCP spec_assist at mcp.rs:2845 pass `repo_id: Some(repo.id)` with
  `usage_type: "llm_query"` — deliberately, for the audit trail), while
  filtering llm_query out of the repo *counter* charge. But
  `apply_rolling_24h_window` (budget.rs:1247) recomputes every existing repo
  usage entry's `tokens_used_today`/`cost_today` from `list_since` records with
  **no `usage_type` filter** (budget.rs:1297-1302: every record carrying a
  `repo_id` lands in `repo_sums`). After the first hourly
  `spawn_budget_reset` cycle, a repo with both agent usage and user queries has
  its repo entry inflated by the user queries, and `check_repo_spawn_budget`
  (budget.rs:332) rejects spawns — "user queries exhaust a repo budget and
  block agent spawns", the exact failure mode the F2 repair's own comment says
  the spec forbids ("Charged to workspace budget directly (no repo-level
  tracking — user queries have no repo context)"). Repro: repo budget override
  `max_tokens_per_day: 1000`; agent reports 900 tokens (repo entry 900); user
  runs specs/assist scoped to that repo consuming 500 tokens (repo entry stays
  900 — incremental filter works); hourly job runs → repo entry becomes 1400;
  next spawn rejected "repo budget exceeded". No test covers the intersection:
  `llm_query_with_repo_context_charges_workspace_only` checks only the
  incremental counters, and `rolling_24h_window_clears_expired_usage` uses only
  `agent_run` records. Fix: filter `usage_type == "llm_query"` out of
  `repo_sums` in `apply_rolling_24h_window` (they still belong in `ws_sums`
  and the tenant total), and add a regression test that runs the resync after
  a mixed agent_run + llm_query sequence.

- [major] **F2 (agent-runtime.md §4 Budget Reset): tenant-scoped
  `list_since` vs unscoped usage entries — the hourly job zeroes daily
  counters for every workspace not under the literal "default" tenant in
  DB-backed deployments.** `state.budget_calls` is the base storage handle
  (lib.rs:1329 via the `store!` macro at lib.rs:1136, built at lib.rs:1103/1113
  with `SqliteStorage::new`/`PgStorage::new` → tenant scope `"default"`).
  `SqliteStorage::list_since` (adapters/sqlite/budget_call.rs:120) and
  `PgStorage::list_since` (adapters/postgres/budget_call.rs:120) filter
  `budget_call_records.tenant_id = <handle scope>`, but records are saved with
  the workspace's real tenant id (`record_llm_call_usage` sets `tenant_id` from
  `ws.tenant_id`), and `apply_rolling_24h_window` iterates the **unscoped**
  `list_all_usage` (adapters/sqlite/kv_store.rs:261 — whole table) writing
  `ws_sums.get(...).unwrap_or((0, 0.0))`. In any DB-backed deployment with a
  workspace whose tenant ≠ "default" (multi-tenant is supported: admin tenant
  creation admin.rs:562, workspace tenant derived from auth workspaces.rs:104),
  the hourly job sets that workspace's tokens/cost to 0 and drops all
  non-default tenants' usage from the `tenant:global` ceiling — daily budgets
  effectively reset hourly and the 80%/100% enforcement stops binding
  (fail-open). The mem adapter's `list_since` (mem.rs:2968) has **no** tenant
  filter, so every test passes while both SQL adapters diverge — the exact
  adapter-contract-divergence class the repo's own
  `check-mem-port-contracts` invariant warns about. The adapter's own
  `list_since` test cannot catch it: it scopes the test storage to "tenant-1"
  and saves "tenant-1" records — matched by construction. Repro (verifier):
  `GYRE_DATABASE_URL=sqlite://...`; create tenant "acme" + a workspace under
  it; report agent usage via `POST /api/v1/agents/:id/usage`; invoke
  `apply_rolling_24h_window`; observe the workspace usage entry zeroed. Fix:
  either make `list_since` unscoped (it feeds a deployment-wide resync that
  already reads/writes unscoped usage entries — the tenant filter belongs to
  per-workspace audit reads like `list_by_workspace`), or scope the resync's
  usage-entry iteration to the same tenant; align mem and SQL adapters on the
  chosen contract and add a SQL-adapter test with a mismatched record tenant.

## Verifier must run (blocked in this sandbox by the build wrapper's 4 GiB gate)

- `cargo test --all` (task AC).
- `cargo test -p gyre-server --lib api::budget` — full budget module
  (cascade, enforcement levels incl. overshoot-grace and beyond-limit,
  llm_query, compute minutes, rolling window, repo rollup/override, lifetime
  ceiling).
- `cargo test -p gyre-server --lib prompt_assembly` and the spawn.rs prompt
  tests (`mcp_persona_prompt_serves_meta_spec_section_only`,
  `pod_manifest_carries_protocol_prompt`).
- `cargo test -p gyre-adapters` (budget_call / repository / kv_store suites).
- Optional targeted probe for F1/F2 once repaired: a test that runs
  `apply_rolling_24h_window` after a mixed agent_run + llm_query sequence with
  a repo entry present, and a SQL-adapter `list_since` test with a record whose
  tenant differs from the handle scope.

## Test-inflation check

None found in what I could execute/inspect. The enforcement tests drive the
production `record_llm_call_usage`/`enforce_workspace_budget` paths, assert
observable outcomes (agent status transitions, inbox messages, worktree/task
state, counter values), and would fail if the behavior regressed. The two
findings above are untested *intersections*, not self-confirming tests.

## Scope check

Diff is confined to the task's surface (budget/repos/spawn/mcp/jobs/lib/
prompt_assembly/adapters/migration + spec/coverage/task docs +
dead-message-kind exemption removals for the now-emitted
BudgetWarning/BudgetExhausted kinds). `web/dist` untouched by this candidate —
nothing to record on the generated-output side.
