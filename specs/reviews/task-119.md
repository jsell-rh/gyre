# Review: TASK-119 — Implement Budget Enforcement Cascade

**Reviewer:** Independent review (round 2, candidate `57d1c74171218453e093a3dfc998ef2c8cabd5df`)
**Base:** `cba416b46a73d3568f745d7b4eae49a0700a90a6`
**Date:** 2026-11-11
**Verdict:** needs-revision (one minor finding; round-1 findings F1–F4 verified repaired)

---

## Round-1 findings — repair verification

- **F1 (major, BudgetExhausted to workspace stream only): REPAIRED.** `enforce_workspace_budget` now emits `Destination::Agent(agent.id)` per active agent at both ≥100% and beyond-limit (api/budget.rs ~769/808). Confirmed `list_unacked`/`list_after` (mem.rs ~3802-3851, sqlite/message.rs ~236) return only `Destination::Agent` messages, so the delivered messages are on the surface agents actually poll. Tests assert via `list_unacked(&agent_id)` (`exhaustion_emits_messages_and_kills_after_grace`, `beyond_limit_kills_immediately_without_grace`) — the assertion surface matches the delivery surface.
- **F2 (moderate, repo budget override fields accepted but never enforced): REPAIRED.** `max_concurrent_agents` binds at spawn in `check_repo_spawn_budget` (live count via repo_id + worktree linkage, blocks at cap, frees on completion — tested with both linkage paths); `max_agent_lifetime_secs` enforced continuously by `enforce_agent_lifetimes` (repo override → effective workspace limit), run after every usage report and from the hourly job, with a repo-tighter-than-workspace test plus an under-limit control agent.
- **F3 (spec, "Inbox priority 7" stale wording): REPAIRED via spec amendment.** agent-runtime.md §4 and human-system-interface.md amended to the unified notification taxonomy (AgentBudgetWarning, High/rank 4 per user-management.md §Notification Entity, which is the authority on the Notification entity and never had a priority-7 rank). Amendment direction verified against user-management.md lines 330/362; documented in the coverage matrix.
- **F4 (minor, system://persona served the protocol section verbatim): REPAIRED.** The spawn-time prompt-set record's `rendered_prompt` now pins `render_system_prompt(...)` only; `system://persona` serves it verbatim while `system://protocol` serves the protocol section. End-to-end test `mcp_persona_prompt_serves_meta_spec_section_only` spawns under a seeded required meta-spec and asserts the persona prompt contains the seeded persona and not the protocol text; `spawn_delivers_meta_spec_prompt_to_agent_env` verifies env delivery of both `GYRE_META_SPEC_PROMPT` and `GYRE_PROTOCOL_PROMPT` through a real kubectl-stub Pod-manifest capture.

## Round-2 finding

- [ ] **F5 (minor, code): repo `max_agent_lifetime_secs` override can exceed the tenant absolute ceiling.**
  (a) `PUT /api/v1/repos/:id` validates repo budget fields against the RAW workspace config; the `if let (Some(repo_max), Some(ws_max))` guards skip validation when the workspace field is unset — but platform-model.md §1 defines an unset workspace budget field as "Inherited from tenant", so the effective workspace limit is the tenant value and a repo override above it must be a 400.
  (b) `effective_lifetime_limit_secs` (api/budget.rs) returns the repo override immediately without min'ing against the effective workspace limit — unlike tokens/cost/concurrent, which are recomputed live via `effective_workspace_config` on every spawn/report.
  Reproduction: tenant budget `{max_agent_lifetime_secs:1800}`, workspace field unset, `PUT /api/v1/repos/:id {"budget":{"max_agent_lifetime_secs":3600}}` → accepted; a repo-bound agent runs to 3600s, past the tenant absolute ceiling. The same replacement-not-min semantics also keeps a repo override binding looser after a later workspace/tenant reduction.
  Tokens/cost/concurrent stay bounded by the effective-workspace checks; only the lifetime dimension escapes. Repair: validate repo budget against the effective workspace config and take `min(repo_limit, effective_workspace_limit)` in `effective_lifetime_limit_secs`; add regression tests for the tenant-set/workspace-unset/repo-higher corner and for a post-set workspace reduction.

## What was verified as real (evidence: /tmp/stage/review-evidence)

- **Cascade validation:** workspace ≤ tenant on all four `BudgetConfig` fields incl. `max_agent_lifetime_secs` (400 on violation, tested); repo ≤ workspace via `Repository.budget` (migration `2026-11-13-000061_repo_budget_and_compute_minutes`, portable SQL, SQLite/PG round-trip tested in the adapters suite); `{}` clears to inherit; `max_agents` ≤ `Workspace.max_agents_per_repo` (task-118, re-verified present).
- **Enforcement levels:** `enforce_workspace_budget` runs after every usage report (`record_llm_call_usage`) and after every budget PUT; 80% → AgentBudgetWarning notification (spawning user + Admins, per-period dedup) + BudgetWarning bus event on the crossing; 100% → BudgetExhausted to every active agent inbox + single grace timer per workspace (`budget_grace_in_progress` guard, re-evaluates before killing, fail-closed on usage read error) then kill; beyond-limit → immediate synchronous kill. Budget kills mark Stopped, preserve the worktree, leave the task InProgress, revoke the token, emit the AgentKilled audit event — all asserted by tests.
- **What's tracked:** per-call `BudgetCallRecord` + workspace/repo/tenant counters; `compute_minutes` charged from `spawned_at` at every terminal transition (REST complete/fail/stop, MCP complete, admin kill, stale-detector death, budget kill — all six call sites verified); `llm_query` charged to the workspace directly from briefing/ask, explorer/generate, specs/assist, MCP spec_assist, graph.
- **Budget reset:** `apply_rolling_24h_window` recomputes counters from per-call records ≥ now−24h (new `BudgetCallRepository::list_since`, all three adapters), zeroes window-empty entities, sets `period_start` to the rolling cutoff; hourly via the lib.rs background task and the JobRegistry registration (old midnight-UTC `reset_daily_counters` removed).
- **Prompt structure (§5):** three-section assembly real; four MCP prompts (`system://persona|protocol|context|constraints`) served agent-scoped from real state; prompts capability advertised in initialize.
- **Mechanical invariants:** check-arch, migration versions/portability, dead-message-kinds (BudgetWarning/BudgetExhausted exemptions removed — real emitters now), mem-port-contracts, ABAC route registry, in-memory-state-stores, inert-enforcement, relative-path-defaults — all pass.
- **Focused probes:** `cargo check -p gyre-server` PASS; `cargo test -p gyre-domain` 466+8 PASS; `cargo test -p gyre-adapters` 447 PASS / 6 ignored / 0 failed.

## Deferred to the independent verifier

gyre-server test compilation exceeds the review-stage memory budget. The verifier must run:
`cargo test --all`, plus focused: `cargo test -p gyre-server budget`, `cargo test -p gyre-server prompt_assembly`,
`cargo test -p gyre-server mcp_prompts`, `cargo test -p gyre-server spawn_delivers_meta_spec_prompt_to_agent_env`,
`cargo test -p gyre-server agent_usage_report`, `cargo test -p gyre-server update_repo_budget_cascade_validated`,
`cargo test -p gyre-server repo_concurrent_budget_override_blocks_spawns`.
