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
commits: ["ed457c45c87bc569a6aabcfd2e1b11d26b33abfa", "8c5468225eda8d28476bb8ff824a23c2c52de23b", "8da4333aaf3c17fd0d8e5068815efc647a16559f", "932ef894d0dfbfd7952d2a2c9babbe9857e13ebd", "93d0f4b995f15a0c00878228dfc080ba54b425fa", "daf00443522544ff1c56bed4ae092fadcd6a8f0b", "046663dbd9f4dc1f45fbbfb792cecdbf2b54f026", "8259c9413c9862d0a49bc6e920a63fadea24d3ba", "dd322e125c3fff0a964387467ee90d8059ea1ce3", "c2d61905b9eebd2bedee4664cf34f3b261d1f0c3", "cd206b555cbf5053060b92c862b3824241e2d011", "4489f0f44e7375d8ce2c0f5fe7e84f6a24636796", "33ead7c1309bfef123974a7135c189931c1d3b31", "f7bf82e9dd77b992d2138e95592b2cd6efc58bf1"]
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

All eight acceptance criteria are implemented in production code across the listed commits (13 implementation commits, test-fix commit `ed457c45`, merge reconciliations `e67f0012`/`d2c12b52`/`509e3b66`). This round is a contract repair: the production fix is preserved unchanged through the executor's merge of the newly assigned base `722179f0`.

### Behavior

- **SSE streaming**: all three LLM endpoints (`POST /workspaces/:id/explorer-views/generate`, `POST /workspaces/:id/briefing/ask`, `POST /repos/:id/specs/assist`) return `text/event-stream` via axum `Sse` with `partial`/`complete`/`error` events (`crates/gyre-server/src/api/{explorer_views,graph,specs_assist}.rs`, shared helpers in `src/llm_helpers.rs`).
- **Prompt templates**: `llm_helpers::resolve_prompt_template` resolves DB override → `specs/prompts/<function>.md` from the repo git tree (template git SHA carried into the cost entry) → hardcoded default; `substitute_template` performs `{{variable}}` substitution. `POST /repos/:id/prompts/save` commits templates directly to the default branch and returns the commit SHA (`specs_assist::save_prompt`).
- **Rate limiting**: `src/llm_rate_limit.rs` implements the in-memory sliding window (10 req/60 s keyed on `(principal, workspace)`, auth-context-derived principal, 429 with `Retry-After`, 60 s inactivity eviction); applied inside each handler after auth, distinct from the global limiter.
- **Model + token limits**: per-workspace `llm_model` with `GYRE_LLM_MODEL` default; per-endpoint max tokens from `GYRE_LLM_MAX_TOKENS_{GENERATE,ASK,ASSIST}` (2000/4000/4000) passed into `predict_json`.
- **Budget charging**: every LLM call records an `llm_query` cost entry with prompt-template git SHA for audit, enforced in all three handlers plus the `prompts/save` path.
- **ABAC**: resource mappings for all four routes present in `src/abac_middleware.rs`; cross-workspace repo scope rejected (`validate_repo_scope`), LLM-hallucinated foreign `repo_id` in view specs nulled with fallback.

### This round (contract repair, base merge 509e3b66)

- The executor restored the assigned task document and merged the newly assigned base `722179f0` (task-063). `git diff 7c8a80bc..509e3b66 --stat -- crates/` is empty — the merge brought only `specs/` and `web/` files, so the Rust server tree at HEAD is byte-identical to the prior-round candidate `7c8a80bc`, whose full lib suite passed 1266/0 and was independently reviewed. No production code changed this round.
- Restored `progress: ready-for-review` and re-added `ed457c45` to the `commits:` frontmatter (dropped by the document restore; required by `check-task-commit-attribution.sh`). The acceptance-criteria checkboxes are left in the assigned unchecked state — completion is recorded here, not by editing the protected contract text.

### Test evidence

- Mechanical checks re-run green on the merged HEAD (exit 0, logs in `/tmp/stage/review-evidence/`): `check-abac-route-registry.sh`, `check-arch.sh`, `check-task-commit-attribution.sh`, `check-in-memory-state-stores.sh`.
- Full lib suite on the byte-identical Rust tree (prior round, `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib`): **1266 passed, 0 failed**, including all task-171 modules (`api::explorer_views`, `api::specs_assist`, `api::graph`, `llm_rate_limit`, `llm_helpers`, `api::llm_prompts`, `api::llm_config`).
- Fresh focused run on the merged HEAD this round (`SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib -- api::explorer_views api::specs_assist api::graph llm_rate_limit llm_helpers api::llm_prompts api::llm_config`): **91 passed, 0 failed** (1175 filtered out), including the SSE-streaming, rate-limit (10 req/min + eviction), prompt-template (git-tree load, `{{...}}` substitution, DB override), `llm_query` budget-charging, and `prompts/save` commit-SHA tests. Log: `/tmp/stage/review-evidence/task171-focused-tests-r3.log`. The first launch of this run stalled on a CPU-starved host (load average ≈ 20, test-binary link progressing ~1 CPU-second per 10 minutes wall); after the sandbox freed up, the rerun compiled and linked in 4m40s — recorded as transient infrastructure contention, not a code defect.
