---
title: "Ground Briefing Q&A in real briefing data with sources and history validation"
spec_ref: "human-system-interface.md §9 Briefing Q&A (§1295-1332)"
depends_on: []
progress: not-started
coverage_sections:
  - "human-system-interface.md §47"
commits: ["e71e96ee05c9283a72b89a244f2858d7133f505b", "11bafc3f850078eafec986d225f91bde1df98d66", "d5bc160911dc99a4d9920859cbe44349f55b0dec"]
---

## Spec Excerpt

From `specs/system/human-system-interface.md` §9 "Briefing Q&A" (§1295-1332):

> Below the briefing, an inline chat grounded in the briefing data.
>
> **Q&A endpoint:** `POST /api/v1/workspaces/:workspace_id/briefing/ask` — request:
> `{question: "...", history?: [{role: "user"|"assistant", content: "..."}]}`, response:
> `{answer: "...", sources: [{spec_path, agent_id, ...}]}`. The `history` array is capped
> at 20 entries (older entries are dropped by the client); **the server rejects requests with
> more than 20 history entries (400)**. The optional `history` array enables follow-up
> questions... The server is stateless; the client owns the conversation state. ABAC:
> `RouteResourceMapping` with `resource_type: "workspace"`, `workspace_param: "workspace_id"`,
> `action_override: "generate"`. Requires workspace membership.
>
> The LLM answering this chat has read-only access to:
> - The briefing data (specs, tasks, MRs, completion summaries, deltas)
> - The knowledge graph (for structural context)
> - The agent completion summaries (for decision reasoning)

## Why this section is open (auditor finding)

The endpoint `briefing_ask` (`crates/gyre-server/src/api/graph.rs:1149`) is **hollow**:

1. **Not grounded.** Line 1196 hardcodes `{{context}}` → `""`. The LLM never sees any
   briefing data — it cannot answer questions about the workspace. The section's core
   requirement (§1297, §1327) is unmet.
2. **No `sources`.** The response streams SSE events carrying only `{text}`. Spec §1325
   requires the response payload to be `{answer, sources: [{spec_path, agent_id, ...}]}`.
   There is no `sources` field anywhere.
3. **History cap wrong.** Lines 1173-1178 silently `drain` history entries over 20. Spec
   §1325 requires the server to **reject** requests with more than 20 entries with HTTP 400.

Everything else (route registration at `api/mod.rs:892`, ABAC `generate` mapping, the
10 req/60s rate limit, `LlmUnavailable` 503, prompt-template resolution) already exists and
is correct — do not touch it.

## Implementation Plan

All work in `crates/gyre-server/src/api/graph.rs` unless noted.

1. **Reject oversized history (400).** Replace the `drain` block (lines 1173-1178) with a
   validation check performed *before* any LLM/rate-limit work is meaningful (place it right
   after `require_workspace`): if `req.history` has `len() > 20`, return
   `Err(ApiError::InvalidInput(...))` (maps to 400). Do not truncate.

2. **Ground the prompt in real briefing data.** Before building `system_prompt`:
   - Resolve `since` using the *exact same* logic as `get_workspace_briefing`
     (graph.rs:1127-1141): explicit param is not available here, so use
     `caller.user_id` → `state.user_workspace_state.get_last_seen(uid, &id)` →
     24h fallback (`now_secs().saturating_sub(24 * 3600)`). Factor this into a small helper
     (e.g. `resolve_since(&state, caller.user_id.as_deref(), &id).await`) and call it from
     both `get_workspace_briefing` and `briefing_ask` so the two stay in sync.
   - Call `assemble_briefing(&state, &id, since).await?` to get the real `BriefingResponse`.
   - Serialize it to a compact JSON string (`serde_json::to_string`) and substitute that for
     `{{context}}` (replacing the `.replace("{{context}}", "")` at line 1196). The briefing
     data (completed, in_progress, cross_workspace, exceptions, metrics, completed_agents)
     is now the LLM's grounding context, satisfying §1327 bullet 1 and 3.
   - Also fold `req.history` into the prompt so follow-ups work: append each history entry
     as `"{role}: {content}"` lines into the context or user prompt (client owns state; server
     is stateless — just include what was sent).

3. **Return `answer` + `sources` per §1325.** Derive `sources` from the assembled briefing:
   collect a de-duplicated list of `{spec_path, agent_id}` objects from briefing items that
   carry a `spec_path` (BriefingItem.spec_path) and from `completed_agents` (agent_id +
   their spec_ref). Define a small `#[derive(Serialize)] struct BriefingSource { spec_path:
   Option<String>, agent_id: Option<String> }` (extra fields allowed per `...` in spec).
   - The streaming transport (SSE) may be retained, but the terminal `complete` event's data
     payload MUST be the spec response object `{answer: <full_text>, sources: [...]}` (rename
     the current `{text: full_text}`). `partial` events may continue to stream incremental
     `{text: chunk}` for UX. This makes the observable response contract conform to §1325.

4. **Update the frontend consumer** (`web/src/lib/api.js` + the Briefing component/test that
   reads the SSE `complete` event): read `answer` and `sources` from the `complete` event
   instead of `text`. Keep `partial` streaming behavior. Display `sources` if the component
   already has a slot; otherwise at minimum stop depending on `complete.text`.

## Acceptance Criteria

- `POST /api/v1/workspaces/:id/briefing/ask` with `history` of length 21 returns HTTP 400
  (no LLM call made).
- The system prompt sent to the LLM contains the JSON-serialized assembled briefing (verify
  via a mock LLM factory that captures the prompt: assert it contains a known field/value
  from the seeded briefing, e.g. a completed MR title). A test that seeds a workspace with an
  MR, asks a question, and asserts the captured prompt includes that MR's data — and that
  would fail if `{{context}}` were empty — is the hard test that closes this section.
- The `complete` SSE event payload deserializes to `{answer, sources}`; `answer` equals the
  concatenated stream text; `sources` is a JSON array (may be empty when no spec_path/agent
  data exists, non-empty when the seeded briefing has spec-linked items).
- Existing behavior preserved: `LlmUnavailable` → 503, rate limit → 429 after 10 req,
  ABAC `generate` enforced, workspace membership required.
- `cargo test -p gyre-server` passes; `cd web && npm test` passes.
- `bash scripts/check-arch.sh` passes (no new hexagonal violations — all work in
  `gyre-server` + `web`).

## Agent Instructions

- Do NOT change the route, ABAC mapping, rate limiter, or LLM-availability handling — they
  are already correct.
- The spec is the contract: the observable response MUST be `{answer, sources}` (§1325). Do
  not invent a different schema. If retaining SSE, the `complete` event carries that object.
- Update, do not delete, the existing tests `briefing_ask_with_mock_llm_streams_sse_events`
  and the frontend `Briefing.test.js` complete-event assertion to match the new
  `{answer, sources}` payload. Replace the SSE-shape tests' expectations rather than
  leaving mirrored/self-confirming assertions.
- Skip formatters/linters/full-suite runs beyond the two test commands above; the loop
  handles global validation.
