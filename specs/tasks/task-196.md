---
title: "Ground Briefing Q&A in real briefing data with sources and history validation"
spec_ref: "human-system-interface.md §9 Briefing Q&A (§1295-1332)"
depends_on: [task-213]
progress: ready-for-review
coverage_sections:
  - "human-system-interface.md §47"
commits: ["3b90956c8d243b34b4bfbd329d3ce57b47819e86", "abcfff040406d5e320dfa970b725f0748178e333", "88b57180cf1397df7966381d3289b8d497c79c20", "05709c242509b89214876339b3c463ede3a31b60"]
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

## Shipped

The Briefing Q&A contract (HSI §9) is implemented in the mainline
(`crates/gyre-server/src/api/graph.rs`, integrated via 05709c24 after the
recovered interrupted checkpoints 88b57180/3b90956c and the formatting-only
abcfff04):

- **History cap (§1325):** `briefing_ask` rejects `history.len() > 20` with
  `ApiError::InvalidInput` → HTTP 400 after `require_workspace`, before the
  rate limiter; exactly 20 accepted. No truncation path remains (the `drain`
  block is gone).
- **Grounding (§1327 bullets 1+3):** `resolve_since` (shared helper:
  explicit param > last_seen_at from user_workspace_state > 24h fallback) is
  used by both `get_workspace_briefing` and `briefing_ask`; the real
  `assemble_briefing` output is JSON-serialized into `{{context}}` of the
  system prompt, with `req.history` replayed as `"{role}: {content}"` lines
  so follow-ups work. Server stays stateless. When the request carries
  `repo_id` (HSI §1.5 repo-scope Briefing), the briefing is narrowed by the
  same validated repo filter as the GET endpoint.
- **Response contract (§1325):** the terminal SSE `complete` event carries
  `{answer, sources}` — `answer` is the full concatenated stream text,
  `sources` is a de-duplicated array derived from briefing items'
  `spec_path` and completed agents' `{spec_ref, agent_id}`
  (`briefing_sources()`). `partial` events still stream incremental
  `{type, text}` chunks.
- **Frontend:** `InlineChat.svelte` reads `parsed.answer ?? parsed.text ??
  streamBuffer` on `complete` and notifies the caller via `onassistant`;
  `Briefing.svelte` tracks both user and assistant turns (client owns
  conversation state) and sends full history (capped client-side at 20) on
  follow-ups, plus `repo_id` when repo-scoped.
- Untouched as instructed: route registration (`api/mod.rs`), ABAC
  `generate` mapping (`abac_middleware.rs`), 10 req/60s rate limiter,
  `LlmUnavailable` → 503.

### Verification (requirements-rebase round, merged tree at ce1bb32f)

- `cargo test -p gyre-server --lib api::graph::tests::briefing` — 22/22
  pass (400 history-cap boundary, prompt grounding via PromptCaptureFactory
  asserting the seeded MR title + spec path + history replay in the captured
  system prompt, SSE `{answer, sources}` with answer == concatenated
  partials and non-empty sources for the seeded spec-linked MR, 503, rate
  limit, repo-scope guards). Evidence:
  `/tmp/stage/review-evidence/task-1....txt` (lib-briefing run).
- `cargo test -p gyre-server --test graph_integration briefing_ask` — 2/2
  pass (`test_briefing_ask_sse`, `test_briefing_ask_not_found`); this
  sandbox now permits loopback listeners, closing the limitation earlier
  rounds deferred to host verification. Evidence:
  `/tmp/stage/review-evidence/task-1....txt` (graph-integration run).
- `cd web && npx vitest run Briefing.test.js InlineChat.test.js
  DetailPanelChat.test.js` — 57/57 pass (complete-event `{answer, sources}`
  consumption, follow-up history accumulation, repo_id ask body, no
  regressions in shared InlineChat consumers). Evidence:
  `/tmp/stage/review-evidence/task-1....txt` (web-vitest run).
- `bash scripts/check-arch.sh` — passes (exit 0).
- `bash scripts/check-task-commit-attribution.sh` — OK exit 0 (merge and
  process commits touch no product surface; canonical `commits:` list
  unchanged).
- `git diff 078ec12a HEAD` — empty: the resolved merge tree is
  byte-identical to the assignment base; `web/dist` unchanged.

### Requirements-rebase round (finding 7314ba39, category=requirements_changed)

The executor merged the assignment base 078ec12a into the retained source
checkpoint branch (7bba9015), leaving three conflicts
(`api/graph.rs`, `Briefing.test.js`, `task-196.md`) plus one auto-merge
duplicate (`briefingChatAnswered` declared twice in `Briefing.svelte`).
All resolved to the base side: main had already integrated this task's
implementation (05709c24) and advanced well past the checkpoint's
merge-base, so the base's briefing code is a strict superset of the
checkpoint's task-196 work — identical 400 history cap, `resolve_since`
grounding, and `{answer, sources}` SSE contract, plus later repo-scope
features the checkpoint predates. The checkpoint's superseded variants
(pre-repo-filter `assemble_briefing` signature, inline concept search)
were dropped in favor of the integrated mainline. No product code was
authored this round; the assignment's contract is satisfied by the
verified mainline implementation above.
