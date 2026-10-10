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
commits: ["932ef894d0dfbfd7952d2a2c9babbe9857e13ebd", "93d0f4b995f15a0c00878228dfc080ba54b425fa", "daf00443522544ff1c56bed4ae092fadcd6a8f0b", "046663dbd9f4dc1f45fbbfb792cecdbf2b54f026", "8259c9413c9862d0a49bc6e920a63fadea24d3ba", "dd322e125c3fff0a964387467ee90d8059ea1ce3", "c2d61905b9eebd2bedee4664cf34f3b261d1f0c3", "cd206b555cbf5053060b92c862b3824241e2d011", "4489f0f44e7375d8ce2c0f5fe7e84f6a24636796", "33ead7c1309bfef123974a7135c189931c1d3b31", "f7bf82e9dd77b992d2138e95592b2cd6efc58bf1"]
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

## Shipped

**Status: ready-for-review.** All contract behaviors from ui-layout.md §2 "LLM Endpoint Contract" are implemented in production code and verified with focused tests at HEAD `1d7dc036abb8a18c35bea26f1d4345565fbdcf7e`.

- **SSE streaming** — all three LLM endpoints (`POST /workspaces/:id/explorer-views/generate` in `api/explorer_views.rs`, `POST /workspaces/:id/briefing/ask` in `api/graph.rs`, `POST /repos/:id/specs/assist` in `api/specs_assist.rs`) return `axum::response::sse::Sse` responses. `event: partial` streams incremental explanation/answer text (structured `view_spec`/`diff` arrays arrive only in `event: complete`, per spec), `event: error` on LLM connection failure or mid-stream chunk failure (a truncated stream is never surfaced as `complete`). Tests: `generate_explorer_view_streams_sse` (content-type `text/event-stream`), `generate_explorer_view_valid_spec_completes_with_view_spec` / `..._invalid_spec_emits_null_view_spec_and_fallback` (kills both inverse bugs of the grammar-validation gate), `briefing_ask_with_mock_llm_streams_sse_events`, `assist_spec_with_valid_diff_response_streams_sse`, plus error-event tests for invalid JSON/missing fields/invalid diff ops and mid-stream failure.
- **Prompt templates** — `specs/prompts/{explorer-generate,briefing-ask,specs-assist}.md` committed with `{{...}}` variables; `llm_helpers::resolve_prompt_template` loads from the repo's git tree (default branch) with the branch-head SHA carried for audit, DB override → git tree → hardcoded fallback precedence; `llm_helpers::substitute_template` does exact-brace variable substitution (unknown variables left verbatim, no recursive expansion). Templates are bound to the workspace meta-spec-set via `workspace_meta_spec_context`. Tests: `prompt_template_loads_from_repo_git_tree_with_sha` (asserts a real 40-hex git SHA), `db_override_beats_git_tree`, `prompt_template_falls_back_to_hardcoded_when_absent`, 4 substitution tests.
- **Model selection & token limits** — `resolve_llm_model` implements workspace `llm_model` → `GYRE_LLM_MODEL` → default, and `max_tokens_for_function` implements `GYRE_LLM_MAX_TOKENS_{GENERATE,ASK,ASSIST}` → hardcoded 2000/4000/4000 (pure env-lookup injection so tests don't mutate process env). Tests: `max_tokens_defaults_match_spec`, `max_tokens_env_override_wins`.
- **Rate limiting** — `llm_rate_limit.rs`: in-memory `HashMap<(principal, workspace), VecDeque<Instant>>` sliding window, 10 req/60 s, checked in every handler after auth (before the LLM call so rejected requests consume nothing), `ApiError::RateLimited` → HTTP 429 with parsed `Retry-After` header; background eviction via `spawn_llm_rate_limiter_cleanup`. Principal keys on `user_id` for humans (agent_id fallback for agent/system tokens) so same-display-name humans don't share a budget. Tests: 8 unit tests (limit rejection, per-user/per-workspace independence, eviction, retry-after bounds) + router-level 429 tests for all three endpoints (`*_rate_limited_after_10_requests` asserting the 11th request returns `Retry-After`).
- **Budget charging** — every successful call appends an `llm_query` `BudgetCallRecord` with prompt-template git SHA via `api/budget::record_llm_budget_call` and increments workspace + tenant counters. Tests: `generate_explorer_view_charges_llm_query_budget_record` (asserts `usage_type == "llm_query"` and token count > 0 in the persisted record), `record_llm_budget_call_increments_counters_and_persists_record`.
- **ABAC** — `RouteResourceMapping` entries: `explorer_view`/`generate` (explorer-views/generate), `workspace`/`generate` (briefing/ask), `spec`/`generate` (specs/assist, prompts/save); the `generate` action is covered by a builtin Allow so Developer-role JWTs aren't default-denied. Tests: `developer_jwt_can_generate_on_llm_endpoints`, `readonly_jwt_denied_on_write`; `check-abac-route-registry.sh` passes.
- **`POST /repos/:id/prompts/save`** — commits directly to the default branch (no feature branch, no MR, no approval), returns `{commit_sha}`. Tests: `save_prompt_returns_commit_sha`, `save_prompt_not_found_returns_404`.
- **Hardening side-effect** (kept, not new scope): task-171 also added `.timeout(...)` to the five previously-exempted unbounded reqwest clients in `crates/gyre-adapters/src/llm/rig_vertexai.rs` (120s Vertex, 10s/5s/2s token exchanges and metadata), shrinking `scripts/unbounded-external-http-exemptions.txt` from 5 entries to 0 and lowering `FROZEN_EXEMPTION_COUNT` accordingly — a strictly stronger gate, no verifier weakening.

**Test evidence** (recorded under `/tmp/stage/review-evidence/`, all at HEAD `1d7dc036`): `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` filtered — `llm_rate_limit` 8/8, `llm_helpers` 9/9, `specs_assist` 21/21, `briefing_ask` 5/5, `explorer_views` 12/12, `abac_middleware` 10/10, `budget` 12/12, `mcp` 79/79. Mechanical checks: `check-arch.sh`, `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh`, `check-mcp-write-tools.sh`, `check-unbounded-external-http.sh`, `check-task-commit-attribution.sh` all exit 0 (`task-171-mechanical-checks.txt`).

**Transport restriction:** this sandbox cannot accept TCP (`accept(): [Errno 95] Operation not supported`, recorded in `/tmp/stage/capabilities.json`), so no live-server HTTP probe was run; the SSE wire format is verified through the in-process router tests above (`tower::ServiceExt::oneshot` drives the full axum pipeline, asserting content-type headers and SSE event payloads). Exact-head GitHub checks remain mandatory after independent review.

## Agent Instructions

Read `ui-layout.md` §2 "LLM Endpoint Contract" for the full contract. Check existing endpoint registration in `crates/gyre-server/src/api/mod.rs`. The `explorer-views/generate` endpoint already has a route registered — verify at `mod.rs`. Use the existing budget and ABAC infrastructure. For SSE, use axum's `Sse` response type. The rate limiter should be a simple `HashMap<(UserId, WorkspaceId), VecDeque<Instant>>` behind a `Mutex`, not a complex middleware.
