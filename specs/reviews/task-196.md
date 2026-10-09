# Review — task-196 (Briefing Q&A: grounding, sources, history cap)

Spec: `specs/system/human-system-interface.md` §9 "Briefing Q&A" (§1295-1332) — HSI §47 coverage row.
Base: `8c2d1775` — Candidate: `3d4e7d8a` (HEAD; `88b57180` checkpoint + `b0657d9a` dist-restore + `3d4e7d8a` task-record).
Verdict: **approved**.

## Round 1 (2026-10-09)

Tree: clean checkout at the candidate; product source byte-identical to the recovered checkpoint `88b57180` (the only intervening changes are the `web/dist` restoration to base state — verified `git diff 8c2d1775 3d4e7d8a -- web/dist` is 0 lines — and the task-record commit touching only `specs/tasks/task-196.md`). Untouched as instructed: route registration, ABAC `generate` mapping, rate limiter, `LlmUnavailable` → 503 (all confirmed by reading the diff — no changes to `api/mod.rs` or `abac_middleware.rs`).

Test runs (evidence under `/tmp/stage/review-evidence/`):

- `cargo test -p gyre-server --lib api::graph::tests::briefing` → **15 passed, 0 failed** (`task-196-server-briefing-tests.txt`).
- `cd web && npm ci` (169 packages), then `npx vitest run` Briefing/InlineChat/DetailPanelChat → **49 passed, 0 failed** (`task-196-frontend-tests.txt`).
- `bash scripts/check-arch.sh` → pass.
- `git diff --check 8c2d1775 3d4e7d8a` → clean.
- Mutation probes (all reverted; final tree clean, HEAD = candidate): **4/4 killed** (`task-196-mutation-probes.txt`).

Contract verification:

- **History cap (§1325):** `briefing_ask` rejects `history.len() > 20` with `ApiError::InvalidInput` (400) after `require_workspace`, before the rate limiter; the base's silent `drain` truncation is gone (`Json(mut req)` was also correctly dropped — the request is no longer mutated). Test asserts both boundaries: exactly 20 accepted (200), 21 rejected (400) with the cap named in the error.
- **Grounding (§1327 bullets 1+3):** new shared helper `resolve_since` (last_seen_at → 24h fallback) used by both `get_workspace_briefing` and `briefing_ask` — behavior-identical to the base's inline logic (verified by reading the base's `since` resolution and `assemble_briefing`'s window filters `updated_at >= since`; the grounding test seeds `mr.updated_at = now_secs()` so it lands in the 24h window deterministically). The real `assemble_briefing` output is JSON-serialized into the `{{context}}` slot of the system prompt; `req.history` is replayed as `"{role}: {content}"` lines. The grounding test uses a `PromptCaptureFactory` LLM that captures the exact system prompt and asserts the seeded MR title, its spec path, the `Briefing data:` block, and both history lines — hard assertions that fail if `{{context}}` is empty (proved by MUTANT 1).
- **Response contract (§1325):** the terminal SSE `complete` event carries `{answer, sources}` — `answer` is the concatenated stream text (test asserts equality with the concatenated `partial` payloads), `sources` is a de-duplicated array from `briefing_sources()` over items' `spec_path` and `completed_agents`' `{spec_ref, agent_id}`, entries with neither field skipped. Test asserts non-empty sources containing the seeded spec path (proved by MUTANT 4). `partial` events still stream incremental `{type, text}` chunks.
- **Frontend:** `InlineChat.svelte` reads `parsed.answer ?? parsed.text ?? streamBuffer` on `complete` (backwards-compatible with other `{text}` producers) and notifies the caller via `onassistant`; `Briefing.svelte` tracks user + assistant turns and sends client-capped (`slice(-20)`) history on follow-ups — server stays stateless. `Briefing.test.js` asserts the rendered answer comes from `answer` (not the partial buffer) and that a follow-up carries both turns in `history` (proved by MUTANT 3). InlineChat's other consumers (`MetaSpecs`, `WorkspaceHome`, `DetailPanel`) don't pass `onassistant` — optional prop, no behavioral change for them; their suites pass.
- **`sources` non-goal noted:** the server derives sources from the assembled briefing rather than tracking which parts the LLM actually cited. The spec's `sources: [{spec_path, agent_id, ...}]` does not demand citation tracking, and the task plan explicitly prescribed this derivation — not a defect.

Mutation results (all against the candidate's tests, all source edits reverted):

- MUTANT 1 — `{{context}}` → `""`: `briefing_ask_prompt_is_grounded_in_real_briefing_data` FAILED. Grounding is test-enforced.
- MUTANT 2 — 400 rejection reverted to base's truncation: `briefing_ask_rejects_history_over_20_entries_with_400` FAILED. The cap is test-enforced.
- MUTANT 3 — frontend `complete` handler ignores `answer` (old `parsed.text ?? streamBuffer`): `Briefing.test.js` Q&A test FAILED. The frontend contract is test-enforced.
- MUTANT 4 — `briefing_sources` hollowed to `Vec::new()`: grounding test's sources assertions FAILED. The sources contract is test-enforced.

Sandbox limitation (infrastructure, not a code defect): loopback TCP listeners are unsupported here (`/tmp/stage/capabilities.json`: `tcp_listener_probe` errno 95), so `cargo test -p gyre-server --test graph_integration` cannot run. I verified by inspection that the two integration tests touching this endpoint are payload-agnostic (`test_briefing_ask_sse` asserts 200/SSE content-type/`partial`+`complete` presence; `test_briefing_ask_not_found` asserts 404) and compatible with the new payload — the mocked `MockLlmPortFactory::echo()` streams chunks, so `partial` events exist and the `complete` event is always emitted. **Host verification must run `cargo test -p gyre-server --test graph_integration`.**

Attribution note: `scripts/check-task-commit-attribution.sh` fails at the candidate, but on `a781ede2` (task-210, an ancestor of the base) — the same failure exists at `8c2d1775`, so it is pre-existing and out of scope for this review. The candidate's own branch commits (`88b57180` product surface, `b0657d9a`/`3d4e7d8a` process/docs) present no attribution gap: the task-labeled product commit is listed in task-196's `commits:` frontmatter.
