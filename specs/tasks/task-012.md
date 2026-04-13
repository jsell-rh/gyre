---
title: "Specs Assist Real LLM Integration"
spec_ref: "ui-layout.md §3 (LLM-Assisted Spec Editing), human-system-interface.md §3 (Explorer)"
depends_on: 
  - task-011
progress: complete
review: specs/reviews/task-012.md
coverage_sections: []
commits: 
  - 2bc7f950
  - 6cde0c8e
---

## Spec Excerpt

From `ui-layout.md` §3:

> `POST /api/v1/repos/:repo_id/specs/assist` — request: `{spec_path, instruction, draft_content?: string}`, response: `{diff: [{op: "add"|"remove"|"replace", path: string, content: string}], explanation}`.

> The LLM reads the current spec + knowledge graph context.

> **Streaming:** All responses stream via Server-Sent Events (SSE). Events: `event: partial` (incremental text chunks for `explanation`), `event: complete` (final complete JSON response).

## Current State

The `specs/assist` handler (`crates/gyre-server/src/api/specs_assist.rs`) makes **real LLM calls** via `LlmPort::stream_complete`, but response parsing and context enrichment are incomplete:
- Real streaming LLM call through `LlmPort` (not simulated) ✓
- SSE streaming with `partial` and `complete` events ✓
- Rate limiting (10 req/60s per user/workspace) ✓
- Prompt template loading and variable substitution ✓
- 503 when LLM unavailable ✓
- **Missing:** Response format is `{text: full_text}`, not `{diff, explanation}` as spec requires
- **Missing:** No knowledge graph context included in prompt (spec says "reads current spec + knowledge graph context")
- **Missing:** No `event: error` for invalid LLM output
- **Missing:** No explicit handling of new spec creation (nonexistent path + draft_content)
- **Stale comment** at file top still says "stubbed LLM" — should be updated

## Implementation Plan

1. **Load spec content from the repo:**
   - Read the current spec at `spec_path` from the repo's default branch via `GitOpsPort`
   - If `draft_content` is provided, use it instead of the committed spec

2. **Build the LLM prompt:**
   - Load the prompt template from `llm_defaults.rs` (or from git `specs/prompts/specs-assist.md` if it exists in the repo)
   - Substitute `{{spec_path}}`, `{{draft_content}}`, `{{instruction}}` variables
   - Include knowledge graph context: query graph nodes governed by this spec for structural context

3. **Call `LlmPort::stream_complete`:**
   - Use `state.llm` factory with the workspace's configured model
   - Stream the response as SSE events (`partial` for explanation, `complete` for final JSON)
   - Parse the LLM's response into the `{diff, explanation}` format
   - Validate the diff ops (`add`/`remove`/`replace`, valid `path` strings)

4. **Handle edge cases:**
   - `spec_path` does not exist + no `draft_content` → 404
   - `spec_path` does not exist + `draft_content` provided → new spec creation (only `add` ops)
   - LLM returns invalid JSON → send `event: error` with explanation
   - LLM unavailable (`state.llm` is None) → return 503

5. **Add tests:**
   - Mock LLM returning valid diff → verify SSE events
   - Mock LLM returning invalid JSON → verify error event
   - Verify rate limiting (existing test may cover this)
   - Verify `draft_content` override behavior

## Acceptance Criteria

- [ ] `POST /api/v1/repos/:repo_id/specs/assist` calls through `LlmPort` (not simulated)
- [ ] Response streams as SSE: `partial` events for explanation, `complete` event with `{diff, explanation}`
- [ ] Prompt includes spec content and knowledge graph context
- [ ] `draft_content` overrides committed spec content when provided
- [ ] New spec creation (nonexistent path + draft_content) works
- [ ] Invalid LLM output produces an `event: error` response
- [ ] Tests verify integration with mock LLM

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-server/src/api/specs_assist.rs` for the current handler
3. Read `crates/gyre-ports/src/llm.rs` for the LlmPort trait
4. Read `crates/gyre-server/src/llm_defaults.rs` for prompt templates
5. Read `crates/gyre-server/src/llm_helpers.rs` for model resolution
6. Read `crates/gyre-server/src/api/explorer_views.rs` for a working example of LLM + SSE streaming
7. Replace the stub with a real LLM call, following the pattern in `explorer_views.rs`
8. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `2bc7f950` feat(git-ops): add read_file method to GitOpsPort (TASK-012)
- `6cde0c8e` feat(specs-assist): real LLM integration with {diff, explanation} response (TASK-012)
