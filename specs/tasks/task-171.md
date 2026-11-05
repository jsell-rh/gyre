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
commits: ["8c5468225eda8d28476bb8ff824a23c2c52de23b", "8da4333aaf3c17fd0d8e5068815efc647a16559f", "932ef894d0dfbfd7952d2a2c9babbe9857e13ebd", "93d0f4b995f15a0c00878228dfc080ba54b425fa", "daf00443522544ff1c56bed4ae092fadcd6a8f0b", "046663dbd9f4dc1f45fbbfb792cecdbf2b54f026", "8259c9413c9862d0a49bc6e920a63fadea24d3ba", "dd322e125c3fff0a964387467ee90d8059ea1ce3", "c2d61905b9eebd2bedee4664cf34f3b261d1f0c3", "cd206b555cbf5053060b92c862b3824241e2d011", "4489f0f44e7375d8ce2c0f5fe7e84f6a24636796", "33ead7c1309bfef123974a7135c189931c1d3b31", "f7bf82e9dd77b992d2138e95592b2cd6efc58bf1", "ed457c45c87bc569a6aabcfd2e1b11d26b33abfa", "d2c12b525bfbba5a4f9f8089c855facee0a98895"]
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

All eight acceptance criteria are implemented in production code across the listed commits (13 implementation commits, repair commit `ed457c45`, merge reconciliation `d2c12b52`).

### Behavior

- **SSE streaming**: all three LLM endpoints (`POST /workspaces/:id/explorer-views/generate`, `POST /workspaces/:id/briefing/ask`, `POST /repos/:id/specs/assist`) return `text/event-stream` via axum `Sse` with `partial`/`complete`/`error` events (`crates/gyre-server/src/api/{explorer_views,graph,specs_assist}.rs`, shared helpers in `src/llm_helpers.rs`).
- **Prompt templates**: `llm_helpers::resolve_prompt_template` resolves DB override → `specs/prompts/<function>.md` from the repo git tree (template git SHA carried into the cost entry) → hardcoded default; `substitute_template` performs `{{variable}}` substitution. `POST /repos/:id/prompts/save` commits templates directly to the default branch and returns the commit SHA (`specs_assist::save_prompt`).
- **Rate limiting**: `src/llm_rate_limit.rs` implements the in-memory sliding window (10 req/60 s keyed on `(principal, workspace)`, auth-context-derived principal, 429 with `Retry-After`, 60 s inactivity eviction); applied inside each handler after auth, distinct from the global limiter.
- **Model + token limits**: per-workspace `llm_model` with `GYRE_LLM_MODEL` default; per-endpoint max tokens from `GYRE_LLM_MAX_TOKENS_{GENERATE,ASK,ASSIST}` (2000/4000/4000) passed into `predict_json`.
- **Budget charging**: every LLM call records an `llm_query` cost entry with prompt-template git SHA for audit, enforced in all three handlers plus the `prompts/save` path.
- **ABAC**: resource mappings for all four routes present in `src/abac_middleware.rs` (`scripts/check-abac-route-registry.sh` passes); cross-workspace repo scope rejected (`validate_repo_scope`), LLM-hallucinated foreign `repo_id` in view specs nulled with fallback.

### Repair in earlier rounds

- `ed457c45` fixed the test setup of 3 task-170-era tests (`generate_forwards_valid_llm_view_spec`, `generate_rejects_invalid_llm_view_spec_with_fallback`, `generate_rejects_llm_spec_with_foreign_repo_id`) that never created their target workspaces and only passed because the pre-task-171 handler skipped the workspace lookup; no production code or assertion strength changed.
- `e67f0012` resolved the prior round's merge of base `e608ac78` (mem.rs test-module union: `budget_call_tests` + `secret_contract_tests`/`trace_payload_tests`, each module byte-identical to its parent).

### Repair in this round (merge with assigned base 2f215b1e, commit d2c12b52)

- The checkout arrived mid-merge of the assigned base `2f215b1e` (task-180) with four conflicted files, resolved as a union:
  - `crates/gyre-server/src/lib.rs`: kept this branch's `budget_calls` port store and took the base's port-backed `SearchPort` store in place of the bare `Arc<MemSearchAdapter>` (satisfies `check-in-memory-state-stores.sh`; all `.search` call sites go through the trait).
  - `docs/api-reference.md`: kept the base's two spec-assertion endpoint rows plus this branch's detailed `prompts/save` row.
  - `web/dist/`: took the base's rebuilt artifacts wholesale — this branch made no `web/src` changes since the previous merge point, so the base's build is the only one consistent with the merged sources; the branch's stale assets (including a previously committed index.html containing literal conflict markers) were removed.
- Restored this task document to `ready-for-review` after the executor reset it to the assigned contract, re-adding `ed457c45` to the `commits:` list (dropped by the restore, flagged by `check-task-commit-attribution.sh`).

### Test evidence

- Full suite on the merged tree (`SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib`): **1266 passed, 0 failed** — including all task-171 modules (`api::explorer_views`, `api::specs_assist`, `api::graph`, `llm_rate_limit`, `llm_helpers`, `api::llm_prompts`, `api::llm_config`) and both sides of the merged mem test modules (`budget_call_tests`, `secret_contract_tests`, `trace_payload_tests`). Log: `/tmp/stage/review-evidence/task171-full-lib-tests.log`.
- Focused run (`api::explorer_views api::specs_assist api::graph llm_rate_limit llm_helpers api::llm_prompts api::llm_config mem::`): **102 passed, 0 failed**. Log: `/tmp/stage/review-evidence/task171-focused-tests.log`.
- Mechanical checks green on the merged tree: `check-arch.sh`, `check-abac-route-registry.sh`, `check-in-memory-state-stores.sh`, `check-migration-versions.sh`, `check-mem-port-contracts.sh`, `check-mcp-write-tools.sh`, `check-inert-enforcement.sh`, `check-task-commit-attribution.sh` (all exit 0; logs under `/tmp/stage/review-evidence/`).
