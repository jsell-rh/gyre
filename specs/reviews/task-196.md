# Review — task-196 (Ground Briefing Q&A in real briefing data with sources and history validation)

Spec: `specs/system/human-system-interface.md` §9 Briefing Q&A (§1295–1332): `POST .../briefing/ask` response `{answer, sources: [{spec_path, agent_id, ...}]}`, server rejects `history` > 20 entries with 400, LLM grounded in the briefing data, server stateless (client owns conversation state).

Candidate `7bba9015` against assigned base `653a696f` (worktree inspected at candidate; product diff = `3b90956c` (merged recovery of `88b57180` + rustfmt `abcfff04` + base) + task-file bookkeeping; `web/dist` byte-identical to base). Verdict: **approved**.

## What was verified

- **400 history cap (§1325).** The base's silent `drain` truncation is gone; `briefing_ask` (graph.rs:1218–1222) returns `ApiError::InvalidInput` → HTTP 400 for `history.len() > 20`, checked after `require_workspace` and before the rate limiter, so invalid requests consume no LLM budget. Test `briefing_ask_rejects_history_over_20_entries_with_400` asserts both the 20-accepted boundary and 21-rejected, plus error text naming the cap.
- **Grounding (§1327 bullets 1+3).** New shared helper `resolve_since` (last_seen_at → 24h fallback) is used by both `get_workspace_briefing` and `briefing_ask`, so the Q&A window matches the REST briefing; the real `assemble_briefing` output is JSON-serialized into `{{context}}` (replacing the base's hardcoded `""` at graph.rs:1282), and `req.history` is replayed as `"{role}: {content}"` lines. Server remains stateless.
- **Response contract (§1325).** The terminal SSE `complete` event carries `{type: "complete", answer, sources}` (graph.rs:1304–1308); `answer` equals the concatenated `partial` text; `sources` is a de-duplicated array from `briefing_sources()` (items' `spec_path` + completed agents' `{spec_ref, agent_id}`). `partial` events stream incremental `{type, text}` chunks. Note: the base server emitted no `type` field while the base client dispatched on `parsed.type` — the added `type` fields fix a dead client path rather than breaking one. No other consumer reads the briefing-ask `complete` payload (CLI has no briefing-ask call; `DetailPanel.svelte` parses a different endpoint's SSE).
- **Frontend.** `InlineChat.svelte` reads `parsed.answer ?? parsed.text ?? streamBuffer` and notifies via new optional `onassistant` (other consumers — MetaSpecs, WorkspaceHome, DetailPanel — use string-returning `onmessage`, unaffected). `Briefing.svelte` tracks both user and assistant turns and resends full history capped client-side at 20. `Briefing.test.js` asserts the final message comes from `answer` (not the partial buffer) and that the follow-up request carries both turns.
- **Untouched as instructed:** route registration (`api/mod.rs:918`), ABAC `generate` mapping (`abac_middleware.rs:417`), rate limiter, `LlmUnavailable` → 503 — all byte-identical to base (empty diff on those files). `get_workspace_briefing` behavior is preserved (explicit `?since=` still wins; same fallback chain, now via the shared helper).

## Evidence (this review's own runs; `/tmp/stage/review-evidence/`)

- `cargo test -p gyre-server --lib api::graph::tests::briefing` → **15/15 passed** (cold build; includes the 400-cap boundary, prompt grounding via `PromptCaptureFactory`, SSE `{answer, sources}` shape, 503, rate limit). `task-196-review-briefing-tests.txt`.
- `cd web && npm ci` (locked) then `npx vitest run Briefing.test.js InlineChat.test.js DetailPanelChat.test.js` → **49/49 passed**. `task-196-review-frontend-tests.txt`.
- **Mutation probes** (each restored; worktree clean after each) — tests are anchored to production behavior, not stubs. `task-196-review-mutation-probes.txt`:
  - Grounding disabled (`.replace("{{context}}", "")`, the base's hollow form) → grounding test FAILS ("system prompt must contain the seeded MR title ... Context: " empty).
  - Cap rejection disabled (`if false && ...`) → cap test FAILS (left: 200, right: 400).
  - `sources` emptied (hardcoded `[]`) → grounding test FAILS ("seeded spec-linked MR must yield at least one source, got: []").
- `bash scripts/check-arch.sh` → OK. `python3 scripts/check-rustfmt-diff.py 653a696f` → "changed lines clean". `bash scripts/check-task-commit-attribution.sh` → OK with the canonical `commits:` list (3b90956c, abcfff04, 88b57180 — exactly the branch's product-surface commits; `b0657d9a` touches only `web/dist`). `git diff --check 653a696f` clean. All `scripts/*-exemptions.txt` unchanged vs base. `abcfff04` verified whitespace-only against `88b57180` (hunk-by-hunk).

## Sandbox limitation (host verification items)

Loopback TCP listeners are unsupported here (`capabilities.json`: tcp_listener_probe errno 95), so `tests/graph_integration.rs::test_briefing_ask_sse` / `test_briefing_ask_not_found` (each spawns `axum::serve` on a bound listener) could not run. Inspected against the new payload: both are payload-agnostic (200 + SSE content-type + `partial`/`complete` presence; 404 for missing workspace) and compatible. Host/CI verification: `cargo test -p gyre-server --test graph_integration`.

Findings: none.
