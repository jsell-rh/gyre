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

## Agent Instructions

Read `ui-layout.md` §2 "LLM Endpoint Contract" for the full contract. Check existing endpoint registration in `crates/gyre-server/src/api/mod.rs`. The `explorer-views/generate` endpoint already has a route registered — verify at `mod.rs`. Use the existing budget and ABAC infrastructure. For SSE, use axum's `Sse` response type. The rate limiter should be a simple `HashMap<(UserId, WorkspaceId), VecDeque<Instant>>` behind a `Mutex`, not a complex middleware.

## Shipped

**LLM Endpoint Contract (ui-layout.md §2) fully implemented across the branch's
checkpoint series (commits 932ef894..4489f0f4 in the `commits:` frontmatter, plus
landing SHAs d88e0a2a/1d1b5b90/1708ea7d/2da8f584 for the original SSE wiring).
This round recovered an interrupted assignment (exit 130, kill-check pending),
merged base 06d70009 (touched only `specs/tasks/` — zero source conflicts with
the LLM endpoint code), and completed verification.**

**Shipped behavior (all in production code, no stubs):**
- **SSE streaming** (`api/graph.rs` briefing_ask, `api/explorer_views.rs`
  generate_explorer_view, `api/specs_assist.rs` assist_spec): axum `Sse`
  responses emitting `event: partial` (incremental text), `event: complete`
  (final JSON), `event: error` (LLM connection failure — HTTP 200 + error
  event, not a hanging stream).
- **Prompt templates** (`llm_helpers.rs::resolve_prompt_template`): DB
  workspace/tenant override → repo git tree `specs/prompts/<function>.md` on
  the default branch (branch-head SHA carried as provenance) → hardcoded
  fallback. `substitute_template` does `{{var}}` substitution; unknown
  variables stay visible (no silent drops). `prompts/save` commits directly
  to the repo's default branch and returns the commit SHA.
- **Rate limiting** (`llm_rate_limit.rs`): per-(user, workspace) sliding
  window, 10 req/60 s, `HashMap<(String, String), VecDeque<Instant>>` behind
  tokio Mutex in AppState, runs after auth in each of the three handlers
  (keyed on immutable `user_id`, agents on `agent_id`), 429 + `Retry-After`
  header, background eviction via `spawn_llm_rate_limiter_cleanup`.
- **Budget charging**: all three handlers call
  `budget::record_llm_budget_call` with `usage_type: "llm_query"`, estimated
  input/output tokens, model name, and prompt-template git SHA; skips+logs
  when the workspace cannot be resolved (no fabricated scope).
- **Model selection + token limits** (`llm_helpers.rs::resolve_llm_model`):
  LLM config override → workspace `llm_model` → `GYRE_LLM_MODEL` env →
  default; `GYRE_LLM_MAX_TOKENS_{GENERATE=2000,ASK=4000,ASSIST=4000}` env
  overrides with spec'd defaults.
- **ABAC** (`abac_middleware.rs`): all four routes have `RouteResourceMapping`
  entries with `action_override: Some("generate")` (briefing/ask → workspace,
  explorer-views/generate → explorer_view, specs/assist + prompts/save →
  spec); new builtin `developer-generate-access` Allow policy (priority 800,
  workspace_role ∈ {Owner, Admin, Developer}) so non-Admin roles can use LLM
  endpoints; Viewer still denied.

**Test evidence (all at current HEAD aa596de6):**
- Focused lib tests: **87 passed, 0 failed**
  (`api::graph::tests::briefing*`, `predict*`, `abac_middleware::tests`,
  `api::explorer_views::tests`, `api::specs_assist::tests`,
  `llm_rate_limit::tests`, `llm_helpers::tests`) — includes SSE partial→
  complete flows with mock LLM, rate-limit-after-10 with 429+Retry-After
  assertion, prompt template load from real git tree with 40-hex SHA,
  `{{var}}` substitution edge cases, `llm_query` budget record charging,
  save_prompt commit SHA, and ABAC regression. Evidence:
  `/tmp/stage/review-evidence/task-171-focused-tests.txt`.
- **Kill-check (regression test kills the bug)**: with the
  `developer-generate-access` policy temporarily disabled
  (`enabled: false` + never-matching condition),
  `developer_jwt_can_generate_on_llm_endpoints` FAILS with 403 ≠ 200 on
  briefing/ask; restored, it passes. The interrupted assignment's pending
  check is complete. Evidence:
  `/tmp/stage/review-evidence/task-171-abac-killcheck-negativeside.txt` and
  `task-171-abac-killcheck-restored.txt`.
- Repo invariant checks at HEAD: `check-task-commit-attribution` exit 0,
  `check-abac-route-registry` exit 0, `check-mem-port-contracts` exit 0,
  `check-byte-slice-truncation` exit 0, `check-in-memory-state-stores` exit 0.

**Transport restriction (recorded, not a code defect):**
`cargo test --test graph_integration -- briefing` cannot run in this sandbox:
the harness binds a real TCP listener and `accept()` fails with errno 95
(`Operation not supported` — same as `/tmp/stage/capabilities.json`
`tcp_listener_probe`), so the serve task panics and reqwest sees
IncompleteMessage. Reproduced directly with a raw socket probe (client
receives b'', accept fails). Required host verification: rerun that
integration test where accept() works, plus GitHub CI. The in-process lib
tests exercise the full router+middleware via `tower::oneshot` — every layer
except the socket. Evidence:
`/tmp/stage/review-evidence/task-171-integration-briefing.txt`.
