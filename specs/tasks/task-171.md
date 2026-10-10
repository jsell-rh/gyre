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
commits: ["daf00443522544ff1c56bed4ae092fadcd6a8f0b", "046663dbd9f4dc1f45fbbfb792cecdbf2b54f026", "8259c9413c9862d0a49bc6e920a63fadea24d3ba", "dd322e125c3fff0a964387467ee90d8059ea1ce3", "c2d61905b9eebd2bedee4664cf34f3b261d1f0c3", "cd206b555cbf5053060b92c862b3824241e2d011", "4489f0f44e7375d8ce2c0f5fe7e84f6a24636796", "33ead7c1309bfef123974a7135c189931c1d3b31", "f7bf82e9dd77b992d2138e95592b2cd6efc58bf1"]
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

All three LLM endpoints now implement the full ui-layout.md §2 endpoint
contract, recovered from an interrupted checkpoint and verified on
`aa38b723` (base `73a31e0b`, 25 files, +1965/−121):

- **SSE streaming** — `explorer-views/generate`, `briefing/ask`, and
  `specs/assist` stream `event: partial` → `event: complete` via axum
  `Sse` (15 s keep-alive). LLM connection failure and mid-stream chunk
  failure both surface as `event: error` (the old
  `filter_map(|r| r.ok())` silently dropped truncated responses);
  explorer-generate validates the LLM JSON against the view-spec grammar
  before `complete` and emits a list-layout `fallback` instead of an
  error when the spec is invalid.
- **Prompt templates** — `resolve_prompt_template` resolves DB override →
  `specs/prompts/<function>.md` on the repo default branch (git-tree read
  via `git_ops`, branch-head SHA as provenance) → hardcoded fallback.
  `substitute_template` handles `{{...}}` variables (unknown variables
  left visible, not dropped). `workspace_meta_spec_context` injects the
  workspace's pinned meta-spec-set; user questions/instructions travel as
  the user prompt only (injection containment). Committed templates:
  `explorer-generate.md`, `briefing-ask.md`, `specs-assist.md`.
  `prompts/save` commits directly to the default branch.
- **Rate limiting** — 10 req/60 s sliding window keyed on
  `rate_limit_principal(auth)` (immutable `user_id` for humans,
  `agent:<id>` for agent/system tokens), 429 + `Retry-After` via
  `ApiError::RateLimited`, auto-eviction after 60 s idle. Applied in all
  three REST handlers plus MCP `specs/assist` parity.
- **Budget charging** — new `BudgetCallRepository` port + SQLite/Postgres
  adapters + `MemBudgetCallRepository` (duplicate-id guarded, per
  check-mem-port-contracts) + migration `2026-10-09-000056_budget_prompt_sha`
  (portable `ALTER TABLE ... ADD COLUMN`). `record_llm_budget_call`
  persists an `llm_query` `BudgetCallRecord` (with prompt-template git
  SHA) and increments workspace + tenant counters on every user-initiated
  LLM call path.
- **ABAC** — `explorer_view`/`workspace` resource mappings with
  `generate` action for all three endpoints plus `prompts/save`
  (`abac_middleware.rs`); foreign-repo `repo_id` in explorer-generate is
  rejected 403.

**Test evidence** (focused probes on `aa38b723`, all exit 0; full log in
`/tmp/stage/review-evidence/task-171-focused-tests.md`):
`cargo check -p gyre-server -p gyre-adapters` clean;
`llm_rate_limit` 8/8; `llm_helpers` 9/9; `explorer_views::` 12/12;
`specs_assist::` 21/21; `briefing` 17/17; `api::budget` 9/9;
`mem::budget_call_tests` 1/1; `sqlite::budget_call` 2/2; `llm::` 25/25
(2 pre-existing ignored); `mcp` 72/72.

Sandbox restriction: TCP listeners unsupported (errno 95), so live
SSE-over-HTTP against a running server is deferred to host verification /
GitHub CI; SSE event sequences are exercised through axum `oneshot`
handler tests.
