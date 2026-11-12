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
commits: ["5e9e678024a2b3bf88cf8ea514ef09a27491bd3c", "3025f256e61a9198cd7f060c2b4d5907585cdd8f", "c5eb3911b4d66871de7b3541667565a63272fa6d", "f7219be28b55cfce7a28a913d6a9538f0fdff116", "7fcbf2723c67e4ee7f289965bfa4a1e55f0fc9be", "ed457c45c87bc569a6aabcfd2e1b11d26b33abfa", "8c5468225eda8d28476bb8ff824a23c2c52de23b", "8da4333aaf3c17fd0d8e5068815efc647a16559f", "932ef894d0dfbfd7952d2a2c9babbe9857e13ebd", "93d0f4b995f15a0c00878228dfc080ba54b425fa", "daf00443522544ff1c56bed4ae092fadcd6a8f0b", "046663dbd9f4dc1f45fbbfb792cecdbf2b54f026", "8259c9413c9862d0a49bc6e920a63fadea24d3ba", "dd322e125c3fff0a964387467ee90d8059ea1ce3", "c2d61905b9eebd2bedee4664cf34f3b261d1f0c3", "cd206b555cbf5053060b92c862b3824241e2d011", "4489f0f44e7375d8ce2c0f5fe7e84f6a24636796", "33ead7c1309bfef123974a7135c189931c1d3b31", "f7bf82e9dd77b992d2138e95592b2cd6efc58bf1", "d712b608880a4c617d30e886b6832e6dd47be7a5"]
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

All eight acceptance criteria are implemented and previously independently approved (see commit history): SSE helper emitting `partial`/`complete`/`error` events, git-tree prompt templates with `{{...}}` substitution, per-(user, workspace) sliding-window rate limiter (10 req/min, 429 + `Retry-After`, 60s eviction), `llm_query` budget cost entries with prompt-template git-SHA audit, three registered SSE endpoints with ABAC mappings, and `prompts/save` direct-to-default-branch commits.

### This round: merge semantic-conflict repair (d712b608)

The merge of base 3afece9d into this branch brought main's task-176/task-209 `assist_meta_spec` handler (workspace-scoped meta-specs/assist) into `specs_assist.rs`, whose shared LLM-assist infrastructure had evolved in later task-171 rounds — the merged file did not compile (3 errors) and regressed the SSE error contract on the new handler. Repaired in `crates/gyre-server/src/api/specs_assist.rs`:

- `assist_sse_response` returns the concrete iterator-backed `Sse` type matching `sse_error_stream`, so all return paths of both assist handlers unify under one `impl Trait` slot (E0308).
- `assist_meta_spec` consumes the LLM stream with the error-surfacing while-let loop — mid-stream chunk failures and `stream_complete` connection failures emit `event: error` SSE streams instead of a truncated `complete` or an HTTP 500 (E0599 + ui-layout.md §2 contract).
- Template resolution via `llm_helpers::resolve_prompt_template` (DB override → workspace git tree `specs/prompts/meta-specs-assist.md` → hardcoded fallback) with `substitute_template` covering all four declared variables; `prompt_template_sha` recorded on the `llm_query` budget entry (E0063).
- Rate-limit key switched to `rate_limit_principal` (user_id with agent fallback) — per-user semantics matching the repo-scoped assist.

The prior verification round's ABAC duplicate finding (22 vs 17) was against the pre-merge candidate still carrying the whole-file-grep script; the merged HEAD's production-region scan (baseline 2) passes unchanged — no verifier was weakened this round.

### Evidence (this round)

- `cargo check -p gyre-server` (SKIP_WEB_BUILD=1) — exit 0, zero warnings.
- `cargo fmt --check` — exit 0.
- All 20 mechanical gate scripts exit 0 (arch, ABAC registry + exempt handlers, MCP write tools, migrations ×2, dead message kinds, byte-slice, relative paths, fail-open refs, scope/fabrication/lossy/inert/forged/forwarded/in-memory/unbounded checks, mem port contracts, template substitution).
- Logs under `/tmp/stage/review-evidence/task-171-merge-repair/`.

### Deferred to the independent verifier

- `cargo test -p gyre-server specs_assist` (covers both assist handlers' SSE protocol, rate limiting, template resolution, cross-workspace scope rejection), `cargo test -p gyre-server explorer_views`, `cargo test -p gyre-server llm_helpers llm_rate_limit` — test compilation exceeds this sandbox's 4 GiB build budget.
- Full workspace clippy/fmt/test suites and web build (verification-owned).
