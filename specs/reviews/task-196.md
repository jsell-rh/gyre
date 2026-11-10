# Review — task-196 (Ground Briefing Q&A in real briefing data with sources and history validation)

Spec: `specs/system/human-system-interface.md` §9 "Briefing Q&A" (§1295-1332).
Candidate: `aa2b863a` (base `078ec12a`).
Verdict: **approved** — no findings.

## Candidate structure

`git diff 078ec12a aa2b863a` touches only `specs/tasks/task-196.md` (progress/commits
frontmatter and Shipped-section bookkeeping). The implementation itself lives in the
base mainline, integrated via the task-attributed commit `05709c24`
("feat(task-196): Ground Briefing Q&A in real briefing data with sources and history
validation"), which is an ancestor of the candidate and the first entry of the task's
`commits:` frontmatter. This is the resolution of durable finding `7314ba39`
(requirements_changed): the executor merged the assignment base into the retained
checkpoint (7bba9015) and resolved all conflicts to the base side, since main had
already integrated this task's work. `bash scripts/check-task-commit-attribution.sh`
passes (OK, exit 0).

## Code inspection (`crates/gyre-server/src/api/graph.rs`)

- **History cap (§1325):** `briefing_ask` rejects `history.len() > 20` with
  `ApiError::InvalidInput` → 400 immediately after `require_workspace`, before the rate
  limiter. No `drain`/truncation path remains.
- **Grounding (§1327):** shared helper `resolve_since` (explicit param >
  `user_workspace_state.get_last_seen` > 24h fallback) is called by both
  `get_workspace_briefing` and `briefing_ask`; the real `assemble_briefing` output is
  JSON-serialized and substituted into `{{context}}`; `req.history` is replayed as
  `"{role}: {content}"` lines (server stateless). Request-body `repo_id` is guarded by
  per-handler repo ABAC + workspace-membership validation (403/404), same filter as GET.
- **Response contract (§1325):** the terminal SSE `complete` event carries
  `{type, answer, sources}` — `answer` is the full concatenated stream text, `sources`
  is the order-preserving de-duplicated output of `briefing_sources()` (briefing items'
  `spec_path` + completed agents' `{spec_ref, agent_id}`; entries with neither field
  skipped). `partial` events still stream `{type, text}` chunks.
- **Untouched as instructed:** route registration (`api/mod.rs:988`), ABAC
  `RouteResourceMapping` workspace/`generate` (`abac_middleware.rs:456-460`), 10 req/60s
  rate limiter, `LlmUnavailable` → 503 — all verified present and covered by passing tests.

## Probes (fresh this round; logs under /tmp/stage/review-evidence/)

- `cargo test -p gyre-server --lib api::graph::tests::briefing` → **22/22 pass**, exit 0
  (`lib-briefing-tests.txt`; includes compile, 13m cold).
- `cargo test -p gyre-server --test graph_integration briefing_ask` → **2/2 pass**
  (`test_briefing_ask_sse`, `test_briefing_ask_not_found`), exit 0
  (`graph-integration-briefing.txt`).
- `cd web && npm ci && npx vitest run Briefing.test.js InlineChat.test.js
  DetailPanelChat.test.js` → **57/57 pass**, exit 0 (`web-vitest-briefing.txt`).
- `bash scripts/check-task-commit-attribution.sh` → OK, exit 0
  (`task-commit-attribution.txt`).
- `grep -rn MUTANT crates/ web/src/` → no matches.
- Arch: candidate is byte-identical to base for every product file, so no new
  hexagonal violations are possible by construction.

## Test quality (no self-confirmation)

- `briefing_ask_prompt_is_grounded_in_real_briefing_data` wires a real `LlmPort`
  implementation (`PromptCapturePort`) that records the exact `system_prompt`
  production code passes to `stream_complete`, seeds a workspace with a merged MR
  (title + spec_ref) through the domain stores, and asserts the captured prompt
  contains the MR title, the spec path, the `Briefing data:` block, and the replayed
  history lines. Every asserted string can only originate from the `{{context}}`
  substitution — the test fails if the context were empty. It also asserts the
  `complete` event's `sources` is non-empty and includes the seeded spec path.
- `briefing_ask_rejects_history_over_20_entries_with_400` checks both boundaries:
  exactly 20 accepted (200) and 21 rejected (400) with an error message naming the cap.
- `briefing_ask_with_mock_llm_streams_sse_events` parses the real SSE wire format and
  asserts `answer` equals the concatenated `partial` text and `sources` is a JSON array.
- Frontend `Briefing.test.js` uses distinct partial text (`"partial stream buffer"`)
  vs the complete answer (`"final answer text"`), asserts the answer renders and the
  partial buffer does not — discriminating against a regression to reading
  `complete.text`/the stream buffer — and asserts follow-up requests carry both the
  user turn and the committed assistant answer in `history` (client owns state,
  capped client-side at 20 via `slice(-20)` in `Briefing.svelte`).

All acceptance criteria verified against the candidate tree.
