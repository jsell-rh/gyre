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
commits: ["f7219be28b55cfce7a28a913d6a9538f0fdff116", "7fcbf2723c67e4ee7f289965bfa4a1e55f0fc9be", "ed457c45c87bc569a6aabcfd2e1b11d26b33abfa", "8c5468225eda8d28476bb8ff824a23c2c52de23b", "8da4333aaf3c17fd0d8e5068815efc647a16559f", "932ef894d0dfbfd7952d2a2c9babbe9857e13ebd", "93d0f4b995f15a0c00878228dfc080ba54b425fa", "daf00443522544ff1c56bed4ae092fadcd6a8f0b", "046663dbd9f4dc1f45fbbfb792cecdbf2b54f026", "8259c9413c9862d0a49bc6e920a63fadea24d3ba", "dd322e125c3fff0a964387467ee90d8059ea1ce3", "c2d61905b9eebd2bedee4664cf34f3b261d1f0c3", "cd206b555cbf5053060b92c862b3824241e2d011", "4489f0f44e7375d8ce2c0f5fe7e84f6a24636796", "33ead7c1309bfef123974a7135c189931c1d3b31", "f7bf82e9dd77b992d2138e95592b2cd6efc58bf1"]
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

All eight acceptance criteria are implemented in production code across the listed commits (13 implementation commits, test-fix commit `ed457c45`, merge reconciliations, review repairs `7fcbf272` and `f7219be2`). The latest round (`81607e62d`) was an assigned rebase repair: the branch was merged with the new base `fefde2946` (task-089 HSI Explorer Generated Views), which had independently built the same LLM endpoint surface from the HSI side. The 13 conflicted files were resolved so both feature sets survive:

- **budget_call port + adapters** (`gyre-ports`, sqlite/postgres/mem): keep task-171's `prompt_template_sha` column (migration `000056_budget_prompt_sha`, schema, domain record, round-trip assertions) and adopt the base's tenant-scoped `list_by_workspace` filters; the mem adapter keeps the in-code duplicate-id port-contract guard. Both sides' sqlite tests merged into one suite (tenant-scoped `setup()` via `new_for_tenant`, SHA provenance asserted).
- **`api/budget.rs`**: unified on the base's `LlmCallUsage` struct API (the auto-merged `spawn.rs` agent-usage path already calls it), extended with `prompt_template_sha: Option<String>`; the candidate's long-parameter `record_llm_budget_call` is folded into it, so every caller (explorer generate, briefing/ask, specs/assist REST + MCP, agent usage reports) funnels through one entry point that carries template provenance.
- **`explorer_views/generate`**: the merged handler layers the task-171 contract (auth-principal rate limit, `validate_repo_scope`, git-tree prompt template with SHA + `{{...}}` substitution grounded in workspace graph summary, model + per-endpoint max tokens, SSE `event: error` on LLM failure including mid-stream) on top of the base's task-089 refinement flow (`previous_spec` grammar validation, MR-trace-discovery user prompt via `build_generate_user_prompt`, `generated_view_explanation`, flow-layout trace backing).
- **`graph.rs` briefing/ask**: base's repo-scope ABAC guard (`check_repo_abac`) and repo-filtered briefing assembly kept; budget recording unified on `record_llm_call_usage` with template SHA (plus the base's `CostEntry` analytics record).
- **`specs_assist.rs` + `mcp.rs` spec_assist**: budget calls unified the same way (struct API + template SHA, repo_id attribution kept).
- **`llm/mock.rs`**: struct/constructors now carry both sides' fields — failure injection (`failing`, `stream_error_after`) for task-171's SSE error tests and `scripted_tool_responses` for task-089's multi-turn refinement tests.
- **`scripts/check-abac-route-registry.sh`**: production-region duplicate scan (task-171 repair) kept; `FROZEN_DUPLICATE_COUNT` lowered to the merged resolver's real duplicate count (1).

Test repairs forced by the merged handler semantics (not behavior changes): the base's flow-layout tests now seed their workspace via a `seed_workspace` helper (the generate handler grounds the prompt in the real workspace and 404s unknown ones — the base's handler never looked the workspace up); `explorer_ws_integration`'s `ScriptedLlmPort` implements the 3-arg `predict_json` (max_tokens); `abac_middleware`'s task-171 test expects the seeding result instead of discarding it (matching the base's pattern); `generate_rejects_invalid_previous_spec` passes the workspace id to the 2-arg `app_with_llm_response`.

### Behavior

- **SSE streaming**: all three LLM endpoints (`POST /workspaces/:id/explorer-views/generate`, `POST /workspaces/:id/briefing/ask`, `POST /repos/:id/specs/assist`) return `text/event-stream` via axum `Sse` with `partial`/`complete`/`error` events (`crates/gyre-server/src/api/{explorer_views,graph,specs_assist}.rs`, shared helpers in `src/llm_helpers.rs`).
- **Prompt templates**: `llm_helpers::resolve_prompt_template` resolves DB override → `specs/prompts/<function>.md` from the repo git tree (template git SHA carried into the cost entry) → hardcoded default; `substitute_template` performs `{{variable}}` substitution. `POST /repos/:id/prompts/save` commits templates directly to the default branch and returns the commit SHA (`specs_assist::save_prompt`); the caller-supplied `prompt_path` is validated by `validate_prompt_path` — relative, plain components only (no `..`/root/prefix), strictly inside `specs/prompts/` — so the MR-review bypass cannot reach any other tracked file.
- **Rate limiting**: `src/llm_rate_limit.rs` implements the in-memory sliding window (10 req/60 s keyed on `(principal, workspace)`, auth-context-derived principal, 429 with `Retry-After`, 60 s inactivity eviction); applied inside each handler after auth, distinct from the global limiter.
- **Model + token limits**: per-workspace `llm_model` with `GYRE_LLM_MODEL` default; per-endpoint max tokens from `GYRE_LLM_MAX_TOKENS_{GENERATE,ASK,ASSIST}` (2000/4000/4000) passed into `predict_json`.
- **Budget charging**: every LLM call records an `llm_query` cost entry with prompt-template git SHA for audit, enforced in all three handlers plus the `prompts/save` path.
- **ABAC**: resource mappings for all four routes present in `src/abac_middleware.rs`; cross-workspace repo scope rejected (`validate_repo_scope`), LLM-hallucinated foreign `repo_id` in view specs nulled with fallback.

### Review repairs (prior rounds)

- **`save_prompt` path validation** (commit `f7219be2`, finding from the review of candidate `3ff29841`): `prompt_path` originally went straight to `git_ops.write_file` on the default branch, so the review-gate bypass justified only by spec-lifecycle's `specs/prompts/` `ignored_paths` entry applied to every tracked file — a Developer+ caller could overwrite `specs/system/vision.md`, `scripts/check-arch.sh`, or CI config with no MR, no approval, no notification. `validate_prompt_path` now runs before anything touches git: absolute paths, non-normal components (`..`, root, prefixes), non-UTF-8 segments, the `specs/prompts` directory itself, and anything outside `specs/prompts/` are rejected with 400; backslash-separated inputs collapse to one component on Unix and fail the directory check. Nested subdirectories under `specs/prompts/` remain allowed. Two handler tests cover 12 rejection shapes plus traversal-in-suffix and the nested-subdir happy path; mutation-verified as killing tests (validation removed → both fail, first with `"specs/system/vision.md" → 201 vs 400`).
- **ABAC registry scan scope + SSE error events** (commit `7fcbf272`): scoped the ABAC registry script's scans to the production region of `abac_middleware.rs` (fixing the duplicate-count false positive and the invisible FAIL exit code) and wired the error-injection mocks into five SSE `event: error` handler tests.

### Test evidence

- Merge commit `81607e62d` (this round, rebase repair of the `fefde2946` base): `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib -- prompt llm rate assist briefing explorer specs save builtin developer jwt` → **327 passed, 0 failed** (1179 filtered out, exit 0), covering the merged surface: task-171 contract tests (SSE `event: error` including mid-stream, per-endpoint rate limiting, prompt templates with SHA, budget charging with provenance, `prompts/save` path guards, `developer_jwt_can_generate_on_llm_endpoints`) and the task-089 flow tests (`generate_flow_layout_*`, `generate_rejects_invalid_previous_spec`, `generate_forwards_previous_spec_to_llm`). `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters budget` → 3 passed; `cargo test -p gyre-server --lib budget_call` → 2 passed; `cargo test -p gyre-server --test explorer_ws_integration` → 15 passed. `scripts/check-arch.sh`, `check-mem-port-contracts.sh`, `check-migration-versions.sh`, `check-mcp-write-tools.sh`, `check-abac-route-registry.sh`, `check-task-commit-attribution.sh` → all OK. `cargo check --workspace --lib --tests` clean. The conflict resolutions are rustfmt-clean (remaining `cargo fmt --check` diffs in `llm_helpers.rs`/`explorer_views.rs`/`specs_assist.rs` pre-exist identically on the pre-merge candidate tip, verified by formatting both parents). Logs: `/tmp/stage/review-evidence/task171-rebase-repair-focused-tests.log`, `task171-rebase-repair-failures.log` (pre-fix failures), `task171-checkpoint-focused-tests.log`.
- Merge commit `c32837bc2` (prior round, rebase repair of the `7da7bbff` base): the same focused suite → **291 passed, 0 failed** (1067 filtered out), including both conflict-resolved tests (`developer_jwt_can_generate_on_llm_endpoints`, `builtin_policy_seeding_fails_closed_on_store_error`).
- Checkpoint merge `f6d870f1` (prior round, clean tree, no code changes): focused suite → **239 passed, 0 failed**.
- Prior rounds: focused `save_prompt` tests 4/4 with mutation probe (validation removed → both new tests FAILED; reverted; 4/4 again); broader focused suite on the pre-merge HEAD 235 passed, 0 failed.
