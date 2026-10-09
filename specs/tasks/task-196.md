---
title: "Ground Briefing Q&A in real briefing data with sources and history validation"
spec_ref: "human-system-interface.md §9 Briefing Q&A (§1295-1332)"
depends_on: []
progress: complete
coverage_sections:
  - "human-system-interface.md §47"
commits: ["fb19bdc44b06840994de7bcc8b42e46dc76c6557", "62ed0595e07339870bc71f0bf7da7bd2a7aae42c", "875e2a1117854605168cb558a9d6dcfcbb9d0964", "40a77abf3d05ec0b9e8173091626797f31e52d3a", "17c356a8c1d2b1b746e1a1ec27848efca823612f", "5645f939ecee5f083ef83ef19e769db8fff528c7"]
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

## Verification (implementation round 2026-10-09)

- `cargo test -p gyre-server --lib briefing_ask` — 5/5 pass (400 cap, 503, rate
  limit, SSE shape, prompt grounding with captured system prompt).
- `web` vitest: `Briefing.test.js` 24/24, `InlineChat.test.js` +
  `WorkspaceHomeSections.test.js` 43/43 pass.
- `bash scripts/check-arch.sh` passes.
- Route registration, ABAC `generate` mapping, rate limiter, and
  LlmUnavailable handling untouched (verified via commit diffs).
- Sandbox limitation: `tests/graph_integration.rs::test_briefing_ask_sse` and
  `test_briefing_ask_not_found` cannot run here — this sandbox disallows
  loopback listeners (both fail at the shared request helper with
  `hyper IncompleteMessage`, including the pre-existing 404 path untouched by
  this task). Controller should run `cargo test -p gyre-server --test
  graph_integration` on host.

## Repair (review round 2026-10-09, attempt 12)

**Finding:** commit `62ed059` had reintroduced a silent-truncation mutant at
`graph.rs:1216-1223` (`history.drain(..excess)` with comment "MUTANT: silently
truncate instead of rejecting") in place of the spec-required 400 rejection.
The hard test `briefing_ask_rejects_history_over_20_entries_with_400` fails
against it (21 entries returned 200).

**Repair:** replaced the drain block with the HSI §1325 rejection
(`ApiError::InvalidInput` → 400) before the rate limiter; no truncation.
Verified no other `MUTANT` markers remain in `crates/` or `web/src/`.

**Checks (this sandbox):**
- `cargo test -p gyre-server --lib briefing_ask` — 5/5 pass, including the
  400-cap test that kills the mutant.
- vitest: `Briefing.test.js` + `InlineChat.test.js` — 40/40 pass.
- Sandbox limitation unchanged: `tests/graph_integration.rs` requires loopback
  listeners; controller to run `cargo test -p gyre-server --test
  graph_integration` on host.

## Review

### Review changed source code

- crates/gyre-server/src/api/graph.rs


## Review (round 2026-10-09, independent)

Comparison base 3214c982 → HEAD c3174c1. Diff touches only `graph.rs`,
`task-196.md`, and the three web files — route registration (`api/mod.rs`),
ABAC `generate` mapping (`abac_middleware.rs:417`), rate limiter, and
`LlmUnavailable` handling are byte-identical to the base, as instructed.

### Verified behavior

1. **History cap (§1325).** `graph.rs:1219-1223` rejects `history.len() > 20`
   with `ApiError::InvalidInput` → HTTP 400 (`error.rs:56`), placed after
   `require_workspace` and before the rate limiter. No truncation path
   remains. Boundary case covered: exactly 20 accepted, 21 rejected.
2. **Grounding (§1327 bullets 1+3).** `graph.rs:1245-1246` resolves `since`
   via the shared `resolve_since` (last_seen_at → 24h fallback — same logic
   as `get_workspace_briefing`, now factored into one helper used by both) and
   calls the real `assemble_briefing`; the JSON-serialized briefing replaces
   `{{context}}` in the system prompt (§1283), with history replayed as
   `"{role}: {content}"` lines so follow-ups work.
3. **Response contract (§1325).** The terminal `complete` SSE event carries
   `{answer, sources}` where `answer` is the full concatenated stream text and
   `sources` is a de-duplicated array derived from briefing items' spec_paths
   and completed agents (agent_id + spec_ref). Frontend `InlineChat.svelte`
   reads `parsed.answer ?? parsed.text ?? streamBuffer` and `Briefing.svelte`
   now tracks both user and assistant turns, sending full history on
   follow-ups (client owns conversation state per §1325).

### Mutation probes (isolated worktree, private target dir; both restored,
worktree verified identical to HEAD afterward)

- **Truncation mutant** (replaced rejection with `history.drain(..excess)`):
  `briefing_ask_rejects_history_over_20_entries_with_400` FAILED (21 entries
  → 200, expected 400). Test kills the exact bug class from review attempt 12.
- **Hollow-context mutant** (restored `.replace("{{context}}", "")`):
  `briefing_ask_prompt_is_grounded_in_real_briefing_data` FAILED ("system
  prompt must contain the seeded MR title"; prompt shows empty `Context:`).
  Test kills the original hollowness finding.

The SSE-shape test uses `MockLlmPortFactory::echo()` which chunks the real
user_prompt into 3 stream chunks, so `answer == concatenated partials` is
checked against real streamed content, not a mirrored constant.

### Checks run in this sandbox

- `cargo test -p gyre-server --lib briefing_ask` — 5/5 pass (HEAD).
- `cargo test -p gyre-server --lib briefing` — 19/19 pass (no regressions in
  neighboring briefing/MCP tests).
- vitest `Briefing.test.js` — 24/24 pass; `InlineChat.test.js` — 16/16 pass.
- `scripts/check-arch.sh`, `check-abac-route-registry.sh`,
  `check-mcp-write-tools.sh`, `check-inert-enforcement.sh`,
  `check-dead-message-kinds.sh` — all pass.
- No `MUTANT` markers in `crates/` or `web/src/`.
- Evidence: `/tmp/stage/review-evidence/task196-lib-tests.log`.
- Not run here (loopback listeners disallowed): `--test graph_integration`
  (`test_briefing_ask_sse`, `test_briefing_ask_not_found` — pre-existing
  tests, still assert 200/SSE/404, compatible with the new payload). Host
  gate should run them.

### Findings

None material. One scope note, not a gap for this task: §1327 bullet 2 gives
the LLM read access to "the knowledge graph (for structural context)". The
task plan (auditor finding + implementation plan step 2) scoped grounding to
the assembled briefing (bullets 1+3), and the briefing JSON (entity types,
spec paths, MR/task items) does provide structural context. KG-node
injection into the prompt remains uncovered by task-196's plan; if the
controller wants literal KG data in the Q&A context, that is a follow-up
task, not a revision of this one — the section's hollowness finding (empty
`{{context}}`, no `sources`, truncation) is fully resolved.

Minor, non-blocking observations (no change requested):
- `partial`/`complete` payloads gained a `type` discriminator field; the spec
  allows extra fields (`sources: [..., ...]`), and the SSE envelope was
  already an extension of the §1325 JSON response (transport), so this
  conforms.
- `briefing_sources` collects spec_path from briefing items and
  `{spec_ref, agent_id}` from completed agents; exceptions items carry
  entity_ref as spec_path — all real derivations, non-empty for the seeded
  spec-linked MR in the grounding test.

**Verdict: complete.** The repaired code implements the spec: grounded
prompt, `{answer, sources}` response contract, 400 rejection for oversized
history, with regression tests that kill both reintroduced bug classes.

## Shipped

- `POST /workspaces/:id/briefing/ask` rejects `history` > 20 entries with
  HTTP 400 before the rate limiter (no silent truncation); exactly 20 passes.
- The Q&A system prompt is grounded in the real assembled briefing
  (specs/tasks/MRs/completion summaries via `assemble_briefing`, same
  `since` resolution as the briefing endpoint) with conversation history
  replayed for follow-ups.
- The SSE `complete` event now carries the spec §1325 response object
  `{answer, sources}` — sources de-duplicated from briefing spec_paths and
  completed agents; `partial` events keep streaming chunks.
- Frontend: `InlineChat` commits `answer` (not the partial buffer) from the
  complete event and notifies its caller; `Briefing` tracks full user+assistant
  history client-side and sends it as `history` on follow-up asks.
