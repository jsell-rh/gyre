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
commits: ["7fcbf2723c67e4ee7f289965bfa4a1e55f0fc9be", "ed457c45c87bc569a6aabcfd2e1b11d26b33abfa", "8c5468225eda8d28476bb8ff824a23c2c52de23b", "8da4333aaf3c17fd0d8e5068815efc647a16559f", "932ef894d0dfbfd7952d2a2c9babbe9857e13ebd", "93d0f4b995f15a0c00878228dfc080ba54b425fa", "daf00443522544ff1c56bed4ae092fadcd6a8f0b", "046663dbd9f4dc1f45fbbfb792cecdbf2b54f026", "8259c9413c9862d0a49bc6e920a63fadea24d3ba", "dd322e125c3fff0a964387467ee90d8059ea1ce3", "c2d61905b9eebd2bedee4664cf34f3b261d1f0c3", "cd206b555cbf5053060b92c862b3824241e2d011", "4489f0f44e7375d8ce2c0f5fe7e84f6a24636796", "33ead7c1309bfef123974a7135c189931c1d3b31", "f7bf82e9dd77b992d2138e95592b2cd6efc58bf1"]
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

- [ ] SSE streaming helper produces correct event format
- [ ] Prompt templates loaded from repo git tree with `{{...}}` substitution
- [ ] Rate limiter enforces 10 req/min per (user, workspace) with 429 + Retry-After
- [ ] Budget charging records `llm_query` cost entries
- [ ] Three LLM endpoints registered and return SSE streams
- [ ] ABAC resource mappings configured for all three endpoints
- [ ] `prompts/save` endpoint commits directly to default branch
- [ ] Tests pass

## Agent Instructions

Read `ui-layout.md` §2 "LLM Endpoint Contract" for the full contract. Check existing endpoint registration in `crates/gyre-server/src/api/mod.rs`. The `explorer-views/generate` endpoint already has a route registered — verify at `mod.rs`. Use the existing budget and ABAC infrastructure. For SSE, use axum's `Sse` response type. The rate limiter should be a simple `HashMap<(UserId, WorkspaceId), VecDeque<Instant>>` behind a `Mutex`, not a complex middleware.

## Shipped

All eight acceptance criteria are implemented in production code across the listed commits (13 implementation commits, test-fix commit `ed457c45`, merge reconciliations `e67f0012`/`d2c12b52`/`509e3b66`, review repair `7fcbf272`). This round repaired the two findings from the independent review of candidate `2cd03cf4`; the endpoint behavior below is unchanged.

### Behavior

- **SSE streaming**: all three LLM endpoints (`POST /workspaces/:id/explorer-views/generate`, `POST /workspaces/:id/briefing/ask`, `POST /repos/:id/specs/assist`) return `text/event-stream` via axum `Sse` with `partial`/`complete`/`error` events (`crates/gyre-server/src/api/{explorer_views,graph,specs_assist}.rs`, shared helpers in `src/llm_helpers.rs`).
- **Prompt templates**: `llm_helpers::resolve_prompt_template` resolves DB override → `specs/prompts/<function>.md` from the repo git tree (template git SHA carried into the cost entry) → hardcoded default; `substitute_template` performs `{{variable}}` substitution. `POST /repos/:id/prompts/save` commits templates directly to the default branch and returns the commit SHA (`specs_assist::save_prompt`).
- **Rate limiting**: `src/llm_rate_limit.rs` implements the in-memory sliding window (10 req/60 s keyed on `(principal, workspace)`, auth-context-derived principal, 429 with `Retry-After`, 60 s inactivity eviction); applied inside each handler after auth, distinct from the global limiter.
- **Model + token limits**: per-workspace `llm_model` with `GYRE_LLM_MODEL` default; per-endpoint max tokens from `GYRE_LLM_MAX_TOKENS_{GENERATE,ASK,ASSIST}` (2000/4000/4000) passed into `predict_json`.
- **Budget charging**: every LLM call records an `llm_query` cost entry with prompt-template git SHA for audit, enforced in all three handlers plus the `prompts/save` path.
- **ABAC**: resource mappings for all four routes present in `src/abac_middleware.rs`; cross-workspace repo scope rejected (`validate_repo_scope`), LLM-hallucinated foreign `repo_id` in view specs nulled with fallback.

### This round (review repair, commit `7fcbf272`)

Both findings from the review of candidate `2cd03cf4` are repaired:

- **ABAC registry check false positive + invisible FAIL** (`scripts/check-abac-route-registry.sh`): the script's whole-file grep counted the `developer_jwt_can_generate_on_llm_endpoints` test's route fixtures as resolver duplicates, inflating the frozen duplicate count 22 → 27 on every run. The test itself is unchanged — its route patterns must exactly match the resolver's (`resolve()` is an exact string match) for the `generate` action override to be exercised, so the reviewer's alternative remedy was applied: both the coverage scan and the duplicate scan now read only the production region of `abac_middleware.rs` (everything before `#[cfg(test)]`), guarded by a sanity check that the stripped region still contains `RouteResourceMapping`. `FROZEN_DUPLICATE_COUNT` is lowered to the real resolver duplicate count (6). The script's pre-existing exit-code bug (FAIL=1 set but exit 0 — the guard was invisible to pre-commit/CI) is fixed: the flag is now honored. The missing-routes branch also captures its list before the temp files are deleted, so it prints the actual routes instead of nothing.
- **Dead error-injection mocks / missing SSE error-event coverage** (`api/{graph,specs_assist,explorer_views}.rs`): `MockLlmAdapter::failing()` and `stream_error_after()` are now wired into five handler tests — `briefing/ask` and `specs/assist` for both LLM connection failure and mid-stream failure, `explorer-views/generate` for connection failure (it uses `predict_json`, which has no mid-stream mode). Each asserts 200 + `text/event-stream`, exactly one `event: error`, zero `complete` events, and the failure cause in the error payload. Mutation-verified as killing tests: reintroducing the old `filter_map(|r| r.ok())` silent-drop in both handlers fails both mid-stream tests.

### Test evidence

- `./scripts/check-abac-route-registry.sh` on HEAD: OK, exit 0; guard proven live — a synthetic duplicate resolver entry trips `FAIL: 7 duplicate path entries (baseline: 6)` with exit 1, reverted after the probe. Sibling checks re-run green: `check-abac-exempt-handlers.sh`, `check-api-auth.sh`, `check-abac-context-parity.sh`.
- New tests: 5/5 passed (`SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib --` the five new test names). Mutation probe (silent-drop reintroduced in `graph.rs` + `specs_assist.rs`): both mid-stream tests FAILED, reverted, 5/5 pass again.
- Focused task-171 suite on HEAD: **96 passed, 0 failed** (1181 filtered out) — covers SSE streaming, rate limiting, prompt templates, budget charging, `prompts/save`, plus the five new error-event tests. `gyre-adapters` mock tests: 8/8 passed; no dead-code warnings.
- Logs retained in `/tmp/stage/review-evidence/` (task171-review-repair-*).
