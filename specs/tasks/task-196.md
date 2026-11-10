---
title: "Ground Briefing Q&A in real briefing data with sources and history validation"
spec_ref: "human-system-interface.md §9 Briefing Q&A (§1295-1332)"
depends_on: [task-213]
progress: complete
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

Implementation recovered from interrupted assignment (checkpoint 88b57180),
rustfmt-repaired in abcfff04 (formatting-only, five hunks inside task-196's
own changed lines; `rustfmt --edition 2021`, gate re-run clean), re-verified
fresh in each round:

- **History cap (§1325):** `briefing_ask` rejects `history.len() > 20` with
  `ApiError::InvalidInput` → HTTP 400 after `require_workspace`, before the rate
  limiter; exactly 20 accepted. No truncation path remains (the `drain` block is
  gone).
- **Grounding (§1327 bullets 1+3):** `resolve_since` (new shared helper:
  last_seen_at → 24h fallback) is used by both `get_workspace_briefing` and
  `briefing_ask`; the real `assemble_briefing` output is JSON-serialized into
  `{{context}}` of the system prompt, with `req.history` replayed as
  `"{role}: {content}"` lines so follow-ups work. Server stays stateless.
- **Response contract (§1325):** the terminal SSE `complete` event carries
  `{answer, sources}` — `answer` is the full concatenated stream text, `sources`
  is a de-duplicated array derived from briefing items' `spec_path` and
  completed agents' `{spec_ref, agent_id}` (`briefing_sources()`). `partial`
  events still stream incremental `{type, text}` chunks.
- **Frontend:** `InlineChat.svelte` reads `parsed.answer ?? parsed.text ??
  streamBuffer` on `complete` and notifies the caller via new `onassistant`;
  `Briefing.svelte` tracks both user and assistant turns (client owns
  conversation state) and sends full history (capped client-side at 20) on
  follow-ups.
- Untouched as instructed: route registration (`api/mod.rs`), ABAC `generate`
  mapping (`abac_middleware.rs`), 10 req/60s rate limiter, `LlmUnavailable` →
  503.
- Dropped the accidental `web/dist` rebuild the interrupted checkpoint had
  captured (build.rs rebuild during cargo test) — task branches don't ship dist
  rebuilds (task-210 round 12 precedent) and the regenerated bundle carried a
  `git diff --check` trailing-whitespace failure. `web/dist` is byte-identical
  to main again.

### Verification (product source at 88b57180, formatting-only delta in
abcfff04; re-run fresh this contract-repair round on the restored tree)

- `cargo test -p gyre-server --lib api::graph::tests::briefing` — 15/15 pass
  (400 cap incl. 20-accepted boundary, prompt grounding via PromptCaptureFactory
  asserting the seeded MR title + spec path + history replay in the captured
  system prompt, SSE `{answer, sources}` shape with answer == concatenated
  partials, non-empty sources for the seeded spec-linked MR, 503, rate limit).
  Evidence: `/tmp/stage/review-evidence/task-196-contract-repair-briefing-tests.txt`
  (fresh run this round; earlier rounds' logs:
  `task-196-server-briefing-tests.txt`, `task-196-tests-repair-round.txt`).
- `cd web && npx vitest run Briefing.test.js InlineChat.test.js
  DetailPanelChat.test.js` — 49/49 pass (complete-event `{answer, sources}`
  consumption, follow-up history accumulation, no regressions in shared
  InlineChat consumers). Evidence:
  `/tmp/stage/review-evidence/task-196-contract-repair-frontend-tests.txt`.
- `bash scripts/check-arch.sh` — passes.
- `python3 scripts/check-rustfmt-diff.py 73a31e0b` — "changed lines clean",
  exit 0.
- `bash scripts/check-task-commit-attribution.sh` — OK exit 0 with the
  canonical `commits:` list (abcfff04, 88b57180; re-derived by
  `dev-attribution.py`, matching HEAD e6d79ec8).
- `git diff --check 73a31e0b` (working tree) — clean; `git diff 73a31e0b HEAD
  -- web/dist` empty.
- No `MUTANT` markers in `crates/` or `web/src/`.
- Sandbox limitation: loopback listeners are unsupported here
  (`capabilities.json`: tcp_listener_probe errno 95), so
  `tests/graph_integration.rs::test_briefing_ask_sse` and
  `test_briefing_ask_not_found` cannot run in this sandbox. Both are
  payload-agnostic (assert 200/SSE content-type/`partial`+`complete` presence
  and 404) and compatible with the new payload; host verification must run
  `cargo test -p gyre-server --test graph_integration`.

### Contract-repair round (finding 27dbc148, category=contract)

Audit: the assigned contract (Spec Excerpt, Why-open finding, Implementation
Plan, Acceptance Criteria, Agent Instructions) is byte-identical to the
decomposition commit — verified by hashing `scripts/dev-contract.py`
`requirement_parts` prose (identical) and frontmatter (differs only in the
lifecycle-managed `progress`/`commits` fields plus the assignment-issued
`depends_on: [task-213]`). The finding's cause: the prior round recorded its
repair narrative under `## Repair round (baseline finding d9546b22)` — an
unknown heading, which the contract hash treats as normative prose, changing
the requirement generation. Repaired by removing that section and recording
this round under the canonical `## Shipped` operational heading only (this
subsection is nested under it). Product files untouched this round:
`git diff e6d79ec8 -- crates/ web/src/` empty; the only source-side delta on
the branch remains 88b57180 + the formatting-only abcfff04 (verified hunk by
hunk — whitespace/layout only). `commits:` retains the attribution-canonical
list `["abcfff04...", "88b57180..."]` (re-derived via `dev-attribution.py`);
the restore, not an exemption, keeps
`bash scripts/check-task-commit-attribution.sh` at exit 0.

### Checkpoint round (finding e4852943, category=checkpoint)

Recovered-assignment finding resolved at the merged HEAD (base 653a696f
task-222 merged in at 53606576; product tree byte-identical to the prior
verified tree e6d79ec8 — `git diff e6d79ec8 HEAD -- crates/ web/src/` is
empty, and `git diff 3b90956c HEAD -- .../graph.rs` is empty). No product
code changed this round; fresh verification on the merged tree:

- `cargo test -p gyre-server --lib api::graph::tests::briefing` — 15/15 pass
  at HEAD (cold build, 8m41s; includes the 400 history-cap boundary, prompt
  grounding via PromptCaptureFactory, SSE `{answer, sources}` with
  answer == concatenated partials and non-empty sources for the seeded
  spec-linked MR, 503, rate limit). Evidence:
  `/tmp/stage/review-evidence/task-196-checkpoint-round-briefing-tests.txt`.
- `cd web && npm ci` (locked) then `npx vitest run Briefing.test.js
  InlineChat.test.js DetailPanelChat.test.js` — 49/49 pass at HEAD.
  Evidence: `/tmp/stage/review-evidence/task-196-checkpoint-round-frontend-tests.txt`.
- `bash scripts/check-arch.sh` — passes (exit 0).
- `python3 scripts/check-rustfmt-diff.py 653a696f` — "changed lines clean",
  exit 0.
- `bash scripts/check-task-commit-attribution.sh` — OK exit 0 with the
  canonical `commits:` list (abcfff04, 88b57180).
- `git diff --check 653a696f` clean; `web/dist` byte-identical to base
  restored after build.rs rebuilt it during the test run (task branches
  don't ship dist rebuilds); no `MUTANT` markers in `crates/` or `web/src/`.
- Sandbox limitation unchanged: loopback listeners unsupported
  (`capabilities.json` tcp_listener_probe errno 95), so
  `tests/graph_integration.rs::test_briefing_ask_sse` and
  `test_briefing_ask_not_found` still require host verification
  (`cargo test -p gyre-server --test graph_integration`); both are
  payload-agnostic and compatible with the new `{answer, sources}` payload.
