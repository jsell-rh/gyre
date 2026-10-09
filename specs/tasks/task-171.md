---
title: "LLM Endpoint Contract — SSE streaming, prompt templates, rate limiting"
spec_ref: "ui-layout.md §2 LLM Endpoint Contract"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "ui-layout.md §LLM Endpoint Contract"
  - "ui-layout.md §Role"
  - "ui-layout.md §Available Data"
  - "ui-layout.md §Output Format"
  - "ui-layout.md §Constraints"
commits: ["c2d61905b9eebd2bedee4664cf34f3b261d1f0c3", "cd206b555cbf5053060b92c862b3824241e2d011", "4489f0f44e7375d8ce2c0f5fe7e84f6a24636796", "33ead7c1309bfef123974a7135c189931c1d3b31", "f7bf82e9dd77b992d2138e95592b2cd6efc58bf1", "dd322e125c3fff0a964387467ee90d8059ea1ce3"]
---

## Spec Excerpt

All three LLM endpoints share these behaviors (ui-layout.md §2):

1. **Streaming via SSE**: POST → `Content-Type: text/event-stream`. Events: `partial` (incremental text), `complete` (final JSON), `error`.
2. **Prompt storage**: Templates in `specs/prompts/` (versioned in git, parameterized with `{{...}}` variables).
3. **Model selection**: Per-workspace `llm_model` field, defaulting to `GYRE_LLM_MODEL` env var.
4. **Token limits**: Configurable per endpoint via env vars (`GYRE_LLM_MAX_TOKENS_GENERATE` = 2000, `_ASK` = 4000, `_ASSIST` = 4000).
5. **Rate limiting**: 10 req/min per user per workspace, in-memory sliding window after auth, 429 with `Retry-After`.
6. **Budget charging**: LLM calls charged as `llm_query` cost entries to workspace budget.

Endpoints:
- `POST /api/v1/workspaces/:workspace_id/explorer-views/generate` — view generation
- `POST /api/v1/workspaces/:workspace_id/briefing/ask` — briefing Q&A
- `POST /api/v1/repos/:repo_id/specs/assist` — spec editing assistance

Prompt template format uses `{{variable}}` substitution. Templates are committed to `specs/prompts/` and edited via `POST /api/v1/repos/:repo_id/prompts/save`.

## Implementation Plan

1. **SSE streaming infrastructure** (`gyre-server`):
   - Create a shared `SseStream` helper that wraps LLM API calls and produces `event: partial`, `event: complete`, `event: error` SSE events
   - Implement the streaming response pattern for axum handlers

2. **Prompt template system**:
   - Load templates from `specs/prompts/` in the repo's git tree
   - Variable substitution engine for `{{workspace_name}}`, `{{node_type_summary}}`, etc.
   - `POST /api/v1/repos/:repo_id/prompts/save` endpoint for direct-to-default-branch commits

3. **Per-user rate limiter**:
   - In-memory sliding window counter keyed by `(user_id, workspace_id)`
   - Runs after auth middleware (not the global rate limiter)
   - Returns 429 with `Retry-After` header
   - Auto-eviction after 60s of inactivity

4. **Budget integration**:
   - Record `llm_query` cost entries via existing budget tracking system
   - Attach prompt template git SHA for audit

5. **Endpoint stubs** (actual LLM integration in later tasks):
   - Wire the three endpoints with SSE streaming
   - ABAC: `explorer_view` resource type with `generate` action override
   - Workspace membership required

6. **Tests**:
   - SSE streaming unit tests (partial → complete flow)
   - Rate limiter tests (sliding window, eviction)
   - Prompt template variable substitution tests

## Acceptance Criteria

- [x] SSE streaming helper produces correct event format
- [x] Prompt templates loaded from repo git tree with `{{...}}` substitution
- [x] Rate limiter enforces 10 req/min per (user, workspace) with 429 + Retry-After
- [x] Budget charging records `llm_query` cost entries
- [x] Three LLM endpoints registered and return SSE streams
- [x] ABAC resource mappings configured for all three endpoints
- [x] `prompts/save` endpoint commits directly to default branch
- [x] Tests pass

## Agent Instructions

Read `ui-layout.md` §2 "LLM Endpoint Contract" for the full contract. Check existing endpoint registration in `crates/gyre-server/src/api/mod.rs`. The `explorer-views/generate` endpoint already has a route registered — verify at `mod.rs`. Use the existing budget and ABAC infrastructure. For SSE, use axum's `Sse` response type. The rate limiter should be a simple `HashMap<(UserId, WorkspaceId), VecDeque<Instant>>` behind a `Mutex`, not a complex middleware.

## Shipped

All six shared behaviors of ui-layout.md §2 are real production code:

- **SSE streaming**: `explorer-views/generate` (predict_json + grammar validation → null view_spec + list fallback on invalid), `briefing/ask`, and `specs/assist` all return axum `Sse` streams emitting `event: partial` (incremental text/explanation only — structured `view_spec`/`diff` never stream incrementally), `event: complete` (final JSON), and `event: error` (invalid LLM JSON / invalid diff ops). `specs/assist` streams the explanation progressively in chunks while `diff` arrives only in `complete`.
- **Prompt templates**: `llm_helpers::resolve_prompt_template` resolves DB workspace override → `specs/prompts/<function>.md` read from the repo's git tree (branch-head SHA captured as provenance) → hardcoded fallback. `substitute_template` performs `{{var}}` substitution, leaving unknown placeholders verbatim. Committed templates: `specs/prompts/{explorer-generate,briefing-ask,specs-assist}.md`. `POST /repos/:id/prompts/save` commits directly to the default branch (no MR) and returns `{commit_sha}`.
- **Model selection**: `resolve_llm_model` — LLM config override → workspace `llm_model` → `GYRE_LLM_MODEL` → default. Per-endpoint token limits via `GYRE_LLM_MAX_TOKENS_{GENERATE,ASK,ASSIST}` (2000/4000/4000 defaults), threaded into every LLM call including MCP `specs/assist`.
- **Rate limiting**: `llm_rate_limit.rs` — `HashMap<(principal, workspace), VecDeque<Instant>>` sliding window, 10 req/60 s, applied after auth in all three REST handlers plus MCP `specs/assist`; 429 with `Retry-After` via `ApiError::RateLimited`; background eviction (`spawn_llm_rate_limiter_cleanup`).
- **Budget charging**: every endpoint records an `llm_query` `CostEntry` and a `BudgetCallRecord` with prompt-template git SHA (new `budget_call_records` store + SQLite/PG migration 000056 + mem adapter), incrementing workspace + tenant counters via `record_llm_budget_call`.
- **ABAC**: `explorer_view`/`generate` for explorer-generate, `workspace`/`generate` for briefing/ask, `spec`/`generate` for specs/assist and prompts/save; explorer-generate additionally enforces per-handler workspace membership + tenant match and rejects foreign-repo `repo_id` with 403.

This assignment (continuation of interrupted checkpoint `ae1eec91`) repaired the test harness: the generate tests' shared helper created the workspace in `tenant-1` while the test token authenticates as the system agent in tenant `default` (403), and the SSE/rate-limit tests targeted never-created workspaces that the grounding workspace lookup correctly 404s. Fixed by creating workspaces in the caller's tenant and registering real workspaces for `ws-4`/`ws-rl` — no handler changes.

Focused test evidence (saved at `/tmp/stage/review-evidence/task-171-focused-tests.txt`): `api::explorer_views::tests` 12/12, `api::graph::tests::briefing_ask` 3/3, `api::specs_assist` 21/21, `llm`-filtered 36/36 (llm_helpers, llm_rate_limit, llm_config, llm_prompts), `api::budget` 9/9, gyre-adapters `budget` 2/2. Mechanical checks from the checkpoint run: arch lint, ABAC route registry, migration versions + SQL portability, mem port contracts, in-memory store lint — all pass.
