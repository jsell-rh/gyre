# Review: TASK-171 — LLM Endpoint Contract (SSE streaming, prompt templates, rate limiting)

**Reviewer:** Independent review (pi runtime)
**Date:** 2026-11-08
**Comparison base:** fefde2946f3a0af1f212588f54591516fee0f7b2
**Candidate:** e31d4a7476296bce5357c8ece6a4c782b165fbb2 (merge `81607e62d` rebase repair + process-only docs commits)
**Verdict:** approved — no blocking findings

---

## Scope of this round

This candidate is the rebase repair merging task-171's LLM endpoint contract with the
new base `fefde2946` (task-089 HSI Explorer Generated Views), which had independently
built the same endpoint surface. The review focused on (a) the merge resolution
preserving both feature sets, (b) the eight acceptance criteria remaining enforced in
production code, and (c) mechanical invariants affected by the merge
(ABAC registry, migration numbering, unbounded-HTTP exemptions, MCP write gate).

## Merge resolution verification

Both sides of every conflicted surface survive and interlock correctly:

- **`explorer_views/generate`**: the handler layers task-171's contract
  (auth-principal rate limit → LLM availability → workspace lookup →
  `validate_repo_scope` on caller-supplied `repo_id` → git-tree prompt template with
  SHA + `{{...}}` substitution grounded in `workspace_graph_summary_parts` →
  `resolve_llm_model` with per-endpoint max tokens → `event: error` SSE on LLM
  failure → budget record with `prompt_template_sha`) on top of the base's task-089
  flow (`previous_spec` grammar validation, MR-trace-discovery user prompt via
  `build_generate_user_prompt`, `generated_view_explanation`, flow-layout trace
  backing with graph fallback). Read in full at
  `crates/gyre-server/src/api/explorer_views.rs:530-760`.
- **Budget**: unified on `LlmCallUsage` + `record_llm_call_usage`
  (`api/budget.rs:346-407`) carrying `prompt_template_sha`; every caller (explorer
  generate, briefing/ask, specs/assist REST + MCP, agent usage reports) funnels
  through it. Migration `2026-10-09-000056_budget_prompt_sha` is not a version
  duplicate (Diesel version = date+seq with dashes removed; `check-migration-versions.sh`
  passes).
- **`graph.rs` briefing/ask**: base's `check_repo_abac` + repo-filtered briefing
  assembly kept; budget recording unified with template SHA; mid-stream LLM failures
  surface as `event: error` (no `filter_map(|r| r.ok())` dropping).
- **`llm/mock.rs`**: carries both failure injection (`failing`, `stream_error_after`)
  and task-089's `scripted_tool_responses`.
- **`scripts/check-abac-route-registry.sh`**: production-region scoping kept;
  `FROZEN_DUPLICATE_COUNT` correctly lowered 2→1 to the merged resolver's real
  duplicate count (count verified: script passes at 1).
- **`scripts/unbounded-external-http-exemptions.txt`**: shrunk 5→0 — the former
  `rig_vertexai.rs` exemption sites now build real `.timeout(...)` clients
  (120s adapter, 10s token exchanges, 5s/2s metadata), verified by grep. This is a
  fix, not a grandfather.

## Acceptance criteria — independent evidence

- **SSE streaming**: `briefing_ask_with_mock_llm_streams_sse_events` parses the raw
  wire format (content-type `text/event-stream`, `event: partial` chunks,
  terminal `event: complete` with answer == concatenated partials, sources array).
  Connection-failure and mid-stream-failure tests assert `event: error`. Explorer
  generate SSE tests assert partial→complete and error events. All passed in the
  focused run.
- **Prompt templates**: `resolve_prompt_template` (DB override → repo git tree with
  branch-head SHA → hardcoded fallback) + `substitute_template`
  (`{{variable}}`, unknown vars left verbatim). Git-tree loading verified against a
  real `Git2OpsAdapter` tempdir repo with a 40-hex SHA assertion.
- **Rate limiting**: sliding window 10/60s keyed on auth principal
  (`rate_limit_principal`: user_id, else agent:agent_id — not display name), 429 +
  `Retry-After` via `ApiError::RateLimited`. Behavioral tests hit the 11th request
  on explorer generate, briefing/ask, and specs/assist; background eviction spawned
  from `main.rs`.
- **Budget charging**: `record_llm_call_usage` persists a `BudgetCallRecord` with
  `prompt_template_sha` and increments workspace+tenant counters; sqlite adapter
  round-trip test asserts the SHA column.
- **Three endpoints + ABAC**: routes registered in `api/mod.rs`; resolver mappings
  present for all four routes (`explorer-views/generate` → explorer_view/generate,
  `briefing/ask` → workspace/generate, `specs/assist` and `prompts/save` →
  spec/generate); `check-abac-route-registry.sh` OK.
- **`prompts/save`**: direct default-branch commit with `validate_prompt_path`
  (relative, plain components only, strictly inside `specs/prompts/`); 14 rejection
  shapes + happy path covered; mutation-verified in a prior round.
- **Tests pass**: 328 passed / 0 failed (focused lib suite), 15/15
  `explorer_ws_integration`, 3/3 adapters budget.

## Mutation probes (this round, on the merged handler)

1. Removed the `validate_repo_scope` enforcement from `generate_explorer_view` →
   `generate_explorer_view_rejects_foreign_repo_id` FAILED (200 vs 403). Restored.
2. Removed the `record_llm_call_usage` call →
   `generate_explorer_view_charges_llm_query_budget_record` FAILED (0 vs 1). Restored.
3. Post-restore: `generate_explorer_view*` suite 8/8 passed; `git status` clean.

Evidence: `/tmp/stage/review-evidence/probes.md`.

## Mechanical checks

`check-arch.sh`, `check-migration-versions.sh`, `check-abac-route-registry.sh`,
`check-mcp-write-tools.sh`, `check-unbounded-external-http.sh`,
`check-mem-port-contracts.sh`, `check-task-commit-attribution.sh` — all OK (exit 0).

## Non-blocking observations

- The task's shipped notes state budget charging is "enforced in all three handlers
  plus the `prompts/save` path"; `save_prompt` records no budget entry. The code is
  correct per spec (prompts/save makes no LLM call, so there is nothing to charge);
  the sentence overstates. Documentation nit only.
- MCP `handle_spec_assist` still collects its stream with
  `filter_map(|r| r.ok())`, so a mid-stream MCP failure surfaces as an
  invalid-JSON tool error rather than the underlying error. The MCP path is
  JSON-RPC (not the SSE contract of this task), pre-exists at the base, and the
  task contract does not cover it. Noted for a future parity pass.
