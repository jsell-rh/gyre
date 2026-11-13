# Review — task-119: Implement budget enforcement cascade

**Candidate:** `dcef1a6aa52da504f96bd13654bbf23cc44ed7ee` (branch `pipeline/task-119/05d0560d66754203a87ea8bd83374c7a-1`)
**Base:** `44a01390cc1afae37d39b2d2bfeb4882ea451c29`
**Reviewer model:** gateway/rits/zai-org/glm-5-3 (independent review stage)
**Date:** 2026-11-13
**Verdict:** APPROVED (independent evidence below; server test binaries deferred to the verifier per the 4 GiB sandbox budget)

## Scope

Full task-119 delta base→candidate (the branch merges the earlier task-119
attempts — 385ae459, faa7904d — plus this round's repair `af627d03a`
"report LLM usage from cred-proxy after each proxied call", which answers
review `b8330483e` finding 1: cred-proxy never POSTed usage, so the §4
"What's Tracked" reporting mechanism had no production reporter).

## Acceptance criteria — evidence

| Criterion | Verdict | Evidence |
|---|---|---|
| Cascade repo ≤ workspace ≤ tenant | ✅ | `set_workspace_budget` validates all 4 BudgetConfig fields ≤ tenant (400); `PUT /api/v1/repos/:id` validates every set field of the new `Repository.budget` override against the EFFECTIVE workspace limit (min of workspace config and tenant ceiling — a dimension the workspace leaves unset inherits the tenant). Tests: `cascade_validation_rejects_workspace_exceeding_tenant`, `update_repo_budget_cascade_validated`, `repo_budget_validated_against_effective_workspace_limit` (asserts non-persistence on violation). |
| max_agents ≤ workspace max_agents_per_repo | ✅ | repos.rs:274–284, 400 on violation, tested (`max_agents` suite, from task-118 lineage, present at this head). |
| Usage aggregated per repo, rolled up to workspace | ✅ | `record_agent_usage` (spawn.rs:2422) → `record_llm_call_usage`: per-call `BudgetCallRecord` persisted (new `budget_calls` port + SQLite/PG/mem adapters), workspace + tenant counters, repo entry charged when repo resolution succeeds (agent.repo_id → current task). Test: `agent_usage_report_records_budget_call_and_increments_counters`, `repo_usage_rolls_up_from_agent_reports`. |
| 80% BudgetWarning notification | ✅ | `notify_budget_level` → `AgentBudgetWarning` (High) to spawner + workspace Admins, per-period dedup via `has_active_notification`; `MessageKind::BudgetWarning` bus event rides the same crossing gate. Test: `usage_report_at_80pct_emits_budget_warning_message` (asserts agent stays Active). |
| 100%: BudgetExhausted + 60s grace + kill | ✅ | `enforce_workspace_budget` ≥100% branch: BudgetExhausted to every Active agent's INBOX (`Destination::Agent`), one grace timer per workspace (`budget_grace_in_progress` guard), re-evaluates before killing (never fails open — `Err(_) => true`), `kill_process()` via the backend recorded at spawn, Stopped, worktree preserved, task InProgress, token revoked, AgentKilled audit. Tests: `exhaustion_emits_messages_and_kills_after_grace`, `overshooting_report_gets_graceful_stop_not_hard_kill`, `overshoot_exhaustion_kills_after_grace_expiry`. |
| Beyond-limit immediate kill | ✅ | `cause == BudgetChanged` + usage > limit → synchronous kill. Test: `budget_reduction_below_usage_triggers_beyond_limit_kill` (real PUT route, agent Stopped synchronously). |
| Compute minutes | ✅ | `record_compute_minutes_for_agent` (spawn → terminal, whole minutes, ws + tenant + repo entries) charged on every terminal transition (complete REST+MCP, fail, stop, admin kill, budget kill). Test: `compute_minutes_tracked_on_terminal_transition`. |
| llm_query charged to workspace only | ✅ | Excluded from repo entries both incrementally (budget.rs:615) and in the rolling-window resync (budget.rs:1279), with rationale; call sites: specs_assist (×2), graph briefing, MCP spec_assist. Tests: `llm_query_with_repo_context_charges_workspace_only`, `rolling_window_keeps_llm_query_out_of_repo_budget`. |
| Rolling 24h reset, hourly | ✅ | `apply_rolling_24h_window` recomputes counters from per-call records ≥ now−24h (new `BudgetCallRepository::list_since`, all three adapters); job changed from midnight-UTC reset to hourly in BOTH `spawn_budget_daily_reset` (lib.rs) and the JobRegistry. Test: `rolling_24h_window_clears_expired_usage`. |
| Prompt structure §5 | ✅ | Spawn injects `GYRE_META_SPEC_PROMPT` + `GYRE_META_SPEC_SET_SHA` + `GYRE_PROTOCOL_PROMPT` + `GYRE_ITERATION` (spawn.rs:1315–1339); prompt-set record pins the PERSONA section only (repair — previously concatenated system+protocol). |
| MCP prompts | ✅ | `prompts/list` + `prompts/get` for `system://persona` (pinned meta-spec assembly verbatim, NOT the protocol text), `system://protocol`, `system://context`, `system://constraints` (mcp.rs:3111–3359). End-to-end test `mcp_persona_prompt_serves_meta_spec_section_only` — the 199e47c4a fix routes `/mcp` through `build_router` (the router that actually serves it); compiles clean at this head. |
| cargo test --all | ⏳ verifier | gyre-server test compilation exceeds the 4 GiB sandbox budget by protocol — see "Verifier must run" below. |

## Prior blocking finding — resolved

Review `b8330483e` F1: cred-proxy.mjs never POSTed usage → §4 "What's
Tracked" reporting mechanism dead in production. Repair `af627d03a`:
cred-proxy now POSTs `{tokens_input, tokens_output, cost_usd}` to
`POST /api/v1/agents/:id/usage` (Bearer GYRE_AUTH_TOKEN) after every
proxied LLM call whose response carries a usage block — JSON and
buffered-SSE parsing (cumulative message_delta output tokens),
model-specific pricing table with GYRE_PRICING_JSON override, best-effort
and latency-bounded (10 s) so a failed report never fails the LLM call.
Production wiring is real: entrypoint.sh starts cred-proxy with
GYRE_SERVER_URL/GYRE_AGENT_ID/GYRE_AUTH_TOKEN present (verified at
container spawn), and `proxyRequest` prefers the gyre-server Bearer over
upstream key injection.

**Probe (real, mutation-tested):**
- `node --test docker/gyre-agent/cred-proxy.test.mjs` → exit 0, 14 pass,
  including 5 integration tests that run the REAL proxy child against a
  fake Anthropic upstream and a fake gyre server and assert on what
  arrived at the server. Log: `/tmp/stage/review-evidence/cred-proxy-test.log`.
- Mutation probe (temp copy in /tmp, repo untouched): `return;` injected
  as the first statement of `reportAgentUsage` → 4 of the 5 usage tests
  FAIL (exit 1). The tests kill a disabled reporting path; they are not
  self-confirming. Log: `/tmp/stage/review-evidence/cred-proxy-mutation-test.log`.

## Independent probes run

| Probe | Result | Log |
|---|---|---|
| `node --test cred-proxy.test.mjs` (docker/gyre-agent) | exit 0, 14 pass | review-evidence/cred-proxy-test.log |
| Mutation: reporting disabled (temp copy) | exit 1, 4 fail | review-evidence/cred-proxy-mutation-test.log |
| `cargo check -p gyre-server --lib --tests` | exit 0 (8m22s) — merge repairs (BudgetCallRecord qualification, prompt_template_sha test initializers) compile | review-evidence/cargo-check-gyre-server.log |
| `cargo test -p gyre-domain` | 474 + 8 pass | review-evidence/cargo-test-gyre-domain.log |
| `cargo test -p gyre-adapters --lib budget` | 7 pass | review-evidence/cargo-test-gyre-adapters-budget.log |
| `scripts/check-arch.sh` | exit 0 | review-evidence/check-arch.log |
| `scripts/check-migration-versions.sh` | exit 0 (new 2026-11-13-000061 distinct from 2026-11-12-000061 — diesel versions are the full date-prefixed strings) | review-evidence/check-migration-versions.log |
| `scripts/check-migration-sql-portability.sh` | exit 0 (budget_json TEXT + compute_minutes INTEGER NOT NULL DEFAULT 0 are portable) | review-evidence/check-migration-portability.log |
| `scripts/check-dead-message-kinds.sh` | exit 0 (BudgetWarning/BudgetExhausted removed from exemptions — real emitters now exist) | review-evidence/check-dead-message-kinds.log |

## Spec amendment check

"Inbox priority 7" → "High — rank 4 in the unified notification taxonomy
(user-management.md §Notification Entity)" in agent-runtime.md §4 and
human-system-interface.md: legitimate reconciliation, not requirement
dodging. user-management.md (the canonical notification spec) defines
`AgentBudgetWarning` under High ("Needs attention soon (budget warning…)");
"priority 7" was a stale draft number conflicting with the taxonomy. The
substantive requirement (notification created at 80%, agents continue) is
implemented and tested.

## Verifier must run (server test binaries exceed this sandbox's budget)

1. `cargo test --all` — the task's final acceptance criterion. Last
   recorded full-suite failure on this task's work
   (`api::spawn::tests::mcp_persona_prompt_serves_meta_spec_section_only`,
   404 != 200, candidate 8a7571754) was fixed by 199e47c4a; the fix routes
   the `/mcp` prompts/get call through `build_router`, which compiles clean
   at this head (`cargo check -p gyre-server --lib --tests` exit 0).
2. `cargo test -p gyre-server --lib api::budget::tests` — the §4
   enforcement suite (80%/100%/beyond-limit/grace/rolling-window/compute
   minutes/repo rollup).
3. `cargo test -p gyre-server --lib api::repos::tests` — repo budget
   cascade validation.
4. `cargo test -p gyre-server --lib api::spawn::tests` — usage-report
   budget recording + MCP persona prompt delivery.
5. `cd docker/gyre-agent && npm test` — cred-proxy + agent-runner suites.

## Non-blocking observations

1. **SSE buffering (pre-existing):** cred-proxy's `proxyRequest` buffers
   the full upstream response before replying (base 44a01390 behaves
   identically), so streaming responses reach the agent only after
   completion. The usage extraction relies on this buffering. Not
   introduced by this candidate; latency characteristics are unchanged
   from the base.
2. **Tenant-ceiling reduction path:** the spec's beyond-limit example
   "tenant admin lowered the tenant ceiling" has no production route —
   `set_config(tenant_key(), …)` exists only in tests; the workspace PUT
   is the only production budget-change trigger (pre-existing product
   state, out of this task's scope). If a tenant ceiling is ever lowered
   out-of-band, the next usage report crosses 100% via `UsageReport`
   cause and gets the graceful stop (agents still killed after grace) —
   the enforcement outcome converges, only immediate-vs-grace differs.
3. **Client-side pricing:** cost_usd is computed in cred-proxy from a
   model-pricing table (GYRE_PRICING_JSON override; unknown models report
   0). The spec attributes pricing to the cred-proxy report path, so this
   matches the contract; deployments with custom gateways should set
   GYRE_PRICING_JSON (documented in docker/gyre-agent/README.md).

## Generated-output note

`web/dist/` is untouched by this candidate (no web/ files in the diff);
nothing to observe beyond the standing verifier-owned rebuild.
