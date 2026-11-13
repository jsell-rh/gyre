# Review — task-187 (Conversational Exploration — AI Q&A grounded in knowledge graph)

Spec: `specs/system/system-explorer.md` §8 "Conversational Exploration".
Candidate: `4d9e23a9` (base `23d7361f`); commits `7737e532` (implementation), `fa8568c5` (style repair), `d363ce7d` (round-1 entity-ref repair).
Verdict: **needs-revision** — one blocking finding (F1: a shipped server unit test fails at runtime); everything else verified clean.

## Round 2 (this review)

### Probes run (evidence under `/tmp/stage/review-evidence/`)

- `cd web && npm ci && npx vitest run src/__tests__/ConversationalExplorer.test.js` → **6/6 passed** (`probe2` logs).
- `npx vitest run src/__tests__/ExplorerDefaultViews.test.js ExplorerView.test.js GeneratedViewChat.test.js` → **11/11 passed** (2 files; ExplorerView has no standalone suite at that path).
- Standalone rustc probe of the exact production `validate_entity_refs` + `short_sha` (extracted verbatim from the candidate working tree via sed) against the exact test bodies from the same file:
  - `validate_entity_refs_strips_hallucinations_and_keeps_valid_tokens` → **all assertions pass** on candidate code (the round-1 defect — unknown-kind token `[entity:unknown:x]` reduced to bare id — is genuinely fixed by the dedicated match arm; hallucinated ids are still stripped, valid tokens kept verbatim, labels resolved).
  - `short_sha_never_panics_on_short_or_unicode_input` → **FAILED, exit 101** (`probe2/verbatim_test`, `probe2/short_sha_failure.log`).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → clean (7m19s).
- `bash scripts/check-byte-slice-truncation.sh` → exit 0.
- `bash scripts/check-abac-route-registry.sh` → exit 0.
- Diff inspection of all 15 changed files, base→candidate.

### Verified working (no findings)

- **Grounding is real, not structural.** `build_ask_grounding` reads live `graph_store` nodes (spec linkage, `created_sha`/`last_modified_sha`/`last_modified_by`, recency-ranked, capped 400/300/20/10-repos), edges, and timeline deltas; the prompt test (`ask_explorer_grounded_prompt_reaches_llm`) asserts the provenance strings reach the captured system prompt fully substituted.
- **Citation enforcement is real.** `validate_entity_refs` resolves every `[entity:kind:id]` token against the same graph the prompt was built from; unresolvable ids are stripped to plain text and excluded from `entity_refs`. Verbatim probe passes the full unit test on candidate code. Unknown-kind tokens are preserved literally and the frontend regex only links node|spec|commit|agent, so no ungrounded clickable reference can render.
- **Scope/auth handling.** `check_workspace_membership` before anything; per-user/workspace rate limit identical to the generate endpoint; caller-supplied `repo_id` scope-validated via `validate_repo_scope` (404/Forbidden on foreign repos) before it can leak a foreign graph into the prompt; prediction repo resolved only within the workspace. ABAC route registered before `/:view_id` (first-match resolver) mapping to `explorer_view`/`generate` — registry gate passes.
- **Prompt-injection containment.** Question and client-owned history travel in the user prompt only; the template's `{{question}}` slot is blanked; history roles validated, capped at 20, rejected beyond.
- **Prediction sharing is a genuine extraction.** `run_structural_prediction` in `graph.rs` is the same code path `predict_graph` uses (same prompt construction, same note slot moved to `system_note`, same response shaping); the spec-edit flow now calls the shared `predictionOverlays` mapping — no divergent copy.
- **View-spec chain.** "Show me X" specs pass through `parse_and_validate` + `validate_repo_ownership` + `enforce_flow_trace_backing`, dropped to null on failure — same chain as the generate endpoint.
- **Frontend.** `ConversationalExplorer.svelte` implements chat Q&A with server-validated refs rendered as clickable per-type links (node → detail + camera focus, spec → inline editor, commit → git remote/copy, agent → highlight modified nodes), view-spec application, ghost-overlay predictions, node highlighting, follow-up history replay capped at 20; reachable from the control-bar view selector ("Ask the graph…") and as the third chat-mode tab. History is component state only. Tests exercise real callbacks (they fail if dispatch is removed) and pass 6/6. Locale keys complete. Cost/budget recording and template-SHA audit match sibling LLM endpoints.

### Findings

- [**F1 (blocking): the shipped server test `short_sha_never_panics_on_short_or_unicode_input` fails at runtime — the suite the task itself instructs the verifier to run is red.**]
  `crates/gyre-server/src/api/explorer_views.rs`: production `short_sha` (line 1017) is `sha.chars().take(7).collect()`, so `short_sha("déadbeef")` returns `"déadbee"` (7 chars). The test (line 3571) asserts `assert_eq!(short_sha("déadbeef"), "déadbe")` — 6 characters, which is exactly the first **7 bytes** of the input (d + 2-byte é + a d b e). The expectation was written for byte-truncation semantics — the very behavior `scripts/check-byte-slice-truncation.sh` forbids and this function exists to avoid. The production code is correct; the test expectation is wrong.

  Reproduce: extract both verbatim (as in `/tmp/stage/review-evidence/probe2/verbatim_test.rs`, rustc `--test`, run) → `left: "déadbee"`, `right: "déadbe"`, exit 101. In the verifier: `cargo test -p gyre-server --lib api::explorer_views` fails on this test.

  This is the same failure class as round 1 (review a8fa631b): the server tests were compile-checked but never executed (the 4 GiB sandbox wrapper blocks gyre-server test builds), and the round-2 repair probe covered only `validate_entity_refs` — the `short_sha` test in the same module was never run. The acceptance criterion "Tests pass" is not met.

  **Repair:** one-line test fix — `assert_eq!(short_sha("déadbeef"), "déadbee");` (char semantics, 7 chars) — committed on the task branch and recorded in `commits:` frontmatter; then the full focused suite must actually pass somewhere it can run (verifier), not just compile.

### Verifier must run (unchanged from task notes, plus note F1)

    cargo test -p gyre-server --lib api::explorer_views   # currently FAILS on short_sha test (F1)
    cargo test -p gyre-server --lib api::graph::tests
    cd web && npx vitest run src/__tests__/ConversationalExplorer.test.js

Test-inflation check: none found — the new frontend tests drive real mounted-component behavior and callback dispatch; the server tests drive the real router with a mock LLM factory and assert on captured prompts; the one broken assertion is a wrong expectation, not a self-confirming test.

Scope check: no out-of-scope edits — `predict_graph` behavior preserved by the extraction (same prompts, same response shape); `ExplorerView.svelte` changes are additive wiring; docs/locales updates match the new surface.
