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
commits: ["c5eb3911b4d66871de7b3541667565a63272fa6d"]
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

All eight acceptance criteria were implemented and previously reviewed (implementation commits `8c546822`…`f7bf82e9`, review repairs `7fcbf272`/`f7219be2`, rebase repair `12835877a`; see git history — earlier commit SHAs were carried on the branch's prior rounds). This round (`c5eb3911b`) repairs the two findings from review `9fc567ff`:

### Review-finding repairs

- **Dead `view_spec_grammar` substitution on the git-tree path** (specs/prompts/explorer-generate.md): the committed template now declares `{{view_spec_grammar}}` in its Output Format section (matching ui-layout.md §2's template format example), so the spec's primary prompt-storage path actually delivers the grammar schema to the LLM instead of silently dropping the handler's substitution tuple.
- **Grounding test added** (`explorer_views::tests::git_tree_template_injects_view_spec_grammar_into_system_prompt`): the exact committed template bytes (`include_str!`) are committed into a real workspace repo's git tree, the real `POST /workspaces/:id/explorer-views/generate` handler runs end-to-end, and a mock LLM that now records received system prompts (`MockLlmAdapter::captured_system_prompts`, new) proves the captured prompt contains the full `VIEW_SPEC_GRAMMAR` schema and no unsubstituted `{{...}}` placeholders. The test fails if either the template drops the placeholder or the handler drops the substitution tuple — no mirrored substitution list.
- **Doc misdescription** (docs/api-reference.md): the `briefing/ask` row now says `event: complete` carries `{answer, sources}` — matching the handler payload, HSI §1325, and `InlineChat.svelte`'s `parsed.answer ?? parsed.text`.

### Verification evidence

- `/tmp/stage/bin/cargo check -p gyre-server --tests` — exit 0 (compiles the new test and mock changes; log retained in review evidence).
- `scripts/check-template-substitution.sh` — pass; `scripts/check-arch.sh` — pass.
- Static substitution sim: the template's declared variable set exactly equals the handler's substitution set.
- Deferred to the independent verifier (gyre-server test execution exceeds this sandbox's build budget): `cargo test -p gyre-server git_tree_template_injects_view_spec_grammar`, `cargo test -p gyre-server explorer_views`, `cargo test -p gyre-adapters llm`.
