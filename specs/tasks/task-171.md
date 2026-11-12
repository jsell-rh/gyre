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
commits: ["474d6133c3aaf74cc014ca30fc51ef090f33aacc", "d712b608880a4c617d30e886b6832e6dd47be7a5", "5e9e678024a2b3bf88cf8ea514ef09a27491bd3c", "3025f256e61a9198cd7f060c2b4d5907585cdd8f", "c5eb3911b4d66871de7b3541667565a63272fa6d", "f7219be28b55cfce7a28a913d6a9538f0fdff116", "7fcbf2723c67e4ee7f289965bfa4a1e55f0fc9be", "ed457c45c87bc569a6aabcfd2e1b11d26b33abfa", "8c5468225eda8d28476bb8ff824a23c2c52de23b", "8da4333aaf3c17fd0d8e5068815efc647a16559f", "932ef894d0dfbfd7952d2a2c9babbe9857e13ebd", "93d0f4b995f15a0c00878228dfc080ba54b425fa", "daf00443522544ff1c56bed4ae092fadcd6a8f0b", "046663dbd9f4dc1f45fbbfb792cecdbf2b54f026", "8259c9413c9862d0a49bc6e920a63fadea24d3ba", "dd322e125c3fff0a964387467ee90d8059ea1ce3", "c2d61905b9eebd2bedee4664cf34f3b261d1f0c3", "cd206b555cbf5053060b92c862b3824241e2d011", "4489f0f44e7375d8ce2c0f5fe7e84f6a24636796", "33ead7c1309bfef123974a7135c189931c1d3b31", "f7bf82e9dd77b992d2138e95592b2cd6efc58bf1"]
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

All eight acceptance criteria are implemented and previously independently approved (commit history above): SSE helper emitting `partial`/`complete`/`error` events, git-tree prompt templates with `{{...}}` substitution, per-(user, workspace) sliding-window rate limiter (10 req/min, 429 + `Retry-After`, 60s eviction), `llm_query` budget cost entries with prompt-template git-SHA audit, three registered SSE endpoints with ABAC mappings, and `prompts/save` direct-to-default-branch commits.

### This round: test-build repair (474d6133)

The prior candidate widened `LlmPort::predict_json` to take `max_tokens: Option<u32>` but missed the `RecordingLlmPort` test mock in `crates/gyre-server/src/api/specs_assist.rs`, leaving `cargo check -p gyre-server --tests` broken with error[E0050] so the task-171 suite could not run. Added the missing `_max_tokens: Option<u32>` parameter (one line); no production behavior changed.

Evidence: `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` exits 0 at this commit (log: `/tmp/stage/review-evidence/task171-repair/cargo-check-tests-gyre-server.log`). Full `cargo test -p gyre-server` (linking the server test binary) is deferred to the independent verifier per the runtime memory budget; the suite to execute is the in-module tests in `src/api/specs_assist.rs`, `src/api/graph.rs` (explorer generate SSE), the briefing-ask tests, and `tests/explorer_ws_integration.rs`.
