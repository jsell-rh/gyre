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

All eight acceptance criteria are implemented in production code (implementation commits `8c546822`…`f7bf82e9`, review repairs `7fcbf272`/`f7219be2`, test repair `ed457c45`, and the rebase-repair merge commits listed above). This round (`12835877a`) was an assigned rebase repair onto the new base `cfeefbda`: the branch was merged with the base carrying the spec-lifecycle-configs / gate-validation-type / commit-signatures / task-spec-ref / notification-entity-source migration wave, the signal-chain LLM decomposition/coordination machinery, task-152's briefing narrative rework, and gate-agent route allowlists.

### This round's resolutions

- **`llm/rig_vertexai.rs`** (content conflict): kept the base's latency-bound client refactor (named constants `HTTP_TIMEOUT_SECS=180` / `TOKEN_HTTP_TIMEOUT_SECS=15`) and task-171's 4-param `predict_json` (`max_tokens`) — the base's 3-param impl no longer satisfies the merged `LlmPort` trait. `scripts/unbounded-external-http-exemptions.txt` keeps the base's comment (its constants match the resolved file).
- **`scripts/check-abac-route-registry.sh`** (content conflict): kept task-171's production-region scan, FAIL-flag honor, and missing-paths capture; frozen counts re-baselined to the merged tree — `FROZEN_EXEMPTION_COUNT=46` (both sides shrank the exemption file) and `FROZEN_DUPLICATE_COUNT=2` (both hits are false positives: the `/// Axum route pattern` doc-comment example and the base's `REVIEWER_ALLOWED_ROUTES`/`VALIDATOR_ALLOWED_ROUTES` consts; there are no real duplicate resolver entries). The check passes on the merged tree.
- **`docs/api-reference.md`** (content conflict): kept the base's `settings/spec-lifecycle` row and task-152 briefing description; kept task-171's richer `specs/assist` and `briefing/ask` rows (rate limiting, `llm_query` budget charging, `history` body field).
- **Migration version collision**: base's `2026-10-09-000059_gate_validation_type` collided with task-171's `2026-10-09-000059_budget_prompt_sha` (Diesel runs one migration per version — the prompt-sha column would be silently missing on fresh databases). The base keeps 000059; `budget_prompt_sha` renumbered to `2026-10-09-000061` (next unused sequence). `check-migration-versions.sh` passes.
- **Semantic conflicts fixed on the merged tree**: `signal_chain.rs` (new from base) now passes `max_tokens=None` to the 3-arg `predict_json` (signal-chain calls are not one of the three token-capped ui-layout.md §2 endpoints — behavior unchanged); `api/budget.rs` test initializer gained `prompt_template_sha: None`.
- **CI repair (pre-existing on the candidate, found by this round's mechanical sweep)**: `check-template-substitution.sh` false-positived on every consumer using the shared `llm_helpers::substitute_template(&tpl, &[("var", value), ...])` form (11 violations on the merged tree, 18 on the pre-merge candidate — the checker only recognized inline `.replace("{{var}}", ...)` chains). The checker now accepts the tuple form, scoped to functions that actually call `substitute_template` — same enforcement (every declared `{{variable}}` must be substituted), both forms recognized. Mutation-verified: removing the `("spec_content", ...)` tuple from `specs_assist.rs` makes the check FAIL (1 violation), restoring it passes.

### Behavior (unchanged from the previously reviewed contract)

- **SSE streaming**: all three LLM endpoints (`POST /workspaces/:id/explorer-views/generate`, `POST /workspaces/:id/briefing/ask`, `POST /repos/:id/specs/assist`) return `text/event-stream` via axum `Sse` with `partial`/`complete`/`error` events (`crates/gyre-server/src/api/{explorer_views,graph,specs_assist}.rs`, shared helpers in `src/llm_helpers.rs`).
- **Prompt templates**: `llm_helpers::resolve_prompt_template` resolves DB override → `specs/prompts/<function>.md` from the repo git tree (template git SHA carried into the cost entry) → hardcoded default; `substitute_template` performs `{{variable}}` substitution. `POST /repos/:id/prompts/save` commits templates directly to the default branch; `validate_prompt_path` confines writes to `specs/prompts/` (repair `f7219be2`).
- **Rate limiting**: `src/llm_rate_limit.rs` — in-memory sliding window, 10 req/60s keyed on `(principal, workspace)`, 429 with `Retry-After`, 60s inactivity eviction; applied inside each handler after auth.
- **Model + token limits**: per-workspace `llm_model` with `GYRE_LLM_MODEL` default; per-endpoint max tokens from `GYRE_LLM_MAX_TOKENS_{GENERATE,ASK,ASSIST}` (2000/4000/4000).
- **Budget charging**: every LLM call records an `llm_query` cost entry carrying the prompt-template git SHA (`LlmCallUsage` struct, `prompt_template_sha` column — migration `000061`).
- **ABAC**: resource mappings for all four routes in `src/abac_middleware.rs`; cross-workspace repo scope rejected (`validate_repo_scope`).

### Test evidence

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib --tests` → clean (exit 0); `cargo check -p gyre-adapters --tests -p gyre-cli -p gyre-domain` → clean.
- `cargo test -p gyre-adapters budget` → 3 passed; `migration` → 7 passed (duplicate-version guard included); `llm` → 33 passed; `cargo test -p gyre-domain` → 463 passed.
- Mechanical checks on the merged tree: `check-arch.sh`, `check-abac-route-registry.sh`, `check-migration-versions.sh`, `check-migration-sql-portability.sh`, `check-mcp-write-tools.sh`, `check-mem-port-contracts.sh`, `check-dead-message-kinds.sh`, `check-byte-slice-truncation.sh`, `check-relative-path-defaults.sh`, `check-fail-open-ref-resolution.sh`, `check-fabricated-scope-defaults.sh`, `check-lossy-secret-conversion.sh`, `check-scope-literal-defaults.sh`, `check-inert-enforcement.sh`, `check-forwarded-header-trust.sh`, `check-in-memory-state-stores.sh`, `check-unbounded-external-http.sh`, `check-forged-scope-fields.sh`, `check-template-substitution.sh`, `check-task-commit-attribution.sh` → all exit 0. Logs: `/tmp/stage/review-evidence/task171-rebase4-*.log`.
- **Server tests for the independent verifier** (gyre-server test compilation exceeds this sandbox's 4 GiB build budget and is blocked by the build wrapper): `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib -- prompt llm rate assist briefing explorer specs save builtin developer jwt` (354 passed on the pre-merge candidate), `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib -- budget_call` (2 passed pre-merge), `SKIP_WEB_BUILD=1 cargo test -p gyre-server --test explorer_ws_integration` (15 passed pre-merge), plus `cargo test -p gyre-server --lib -- signal_chain` for the base's new signal-chain path touched by the `max_tokens=None` resolution.
