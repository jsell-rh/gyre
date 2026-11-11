# Review — task-171 (LLM Endpoint Contract — SSE streaming, prompt templates, rate limiting)

**Candidate:** `3db8c68a6cf78fc3a945b6918de647cd213af9b1` (branch `pipeline/task-171/e422d72e3520498188d5e497951a7f75-1`, base `cfeefbda975211a6ac24a17e5920be6f8b23008b`)
**Verdict:** APPROVED — both findings from review `9fc567ff` are repaired with real code and a discriminating grounding test. Evidence: `/tmp/stage/review-evidence/` (summary.md).

## Scope of this round

Since the previously reviewed checkpoint `4d6d29e58` (which already contained rebase-repair merge `12835877a` onto the assigned base), the only product-surface change is repair commit `c5eb3911b` plus two process commits that touch `specs/tasks/task-171.md` only. The eight acceptance criteria were verified in the prior round's full diff review (SSE `partial`/`complete`/`error` incl. mid-stream failures in all three endpoints; DB-override → git-tree → fallback template resolution with git-SHA provenance into `BudgetCallRecord.prompt_template_sha`; 10 req/min sliding-window rate limit keyed on auth principal with 429 + `Retry-After`; per-endpoint token caps; `llm_query` budget charging; ABAC `RouteResourceMapping`s with `generate` action overrides; `prompts/save` direct-to-default-branch commits with `validate_prompt_path` confinement).

## Finding repairs verified

1. **`view_spec_grammar` dead substitution (9fc567ff F1) — FIXED.** `specs/prompts/explorer-generate.md` now declares `{{view_spec_grammar}}` in its Output Format section; the handler's substitution tuple (`explorer_views.rs:596-603`) now lands in the git-tree path. Declared-vs-substituted variable sets match exactly for all three committed templates (explorer-generate, briefing-ask, specs-assist).
2. **Grounding test added — sound and discriminating.** `git_tree_template_injects_view_spec_grammar_into_system_prompt` commits the exact template bytes (`include_str!`) into a real workspace repo's git tree, runs the real `POST /workspaces/:id/explorer-views/generate` handler end-to-end, and asserts via `MockLlmAdapter::captured_system_prompts` (new, capture-before-failure in all three port methods; `MockLlmPortFactory::for_model` returns the same capturing instance) that the system prompt contains the full `VIEW_SPEC_GRAMMAR` schema with no unsubstituted `{{...}}`. Discrimination: the hardcoded fallback is prose without the schema string and `test_state` has no DB template overrides, so the grammar assertion can only pass through the git-tree path with real substitution — a dropped placeholder, a dropped handler tuple, or a silently failing git read each fail the test. `substitute_template` leaves unknown placeholders verbatim, so the `{{`-leak assertion has teeth.
3. **briefing/ask docs payload (9fc567ff F2) — FIXED.** `docs/api-reference.md` now says `event: complete` carries `{answer, sources}`, matching the handler (`graph.rs:1885`) and `InlineChat.svelte`'s `parsed.answer ?? parsed.text`.

## Probes (candidate tree, this round)

- All 21 mechanical check scripts exit 0 (arch, ABAC registry + exempt handlers, migration versions + portability, template substitution, and the full invariant battery; see `summary.md` table).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib --tests` → exit 0 (cold, 7m36s) — the new test and mock changes compile.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib -- llm budget migration` → 43 passed, 0 failed — the modified mock and merged budget/migration adapters pass at runtime.
- Merge-resolution spot checks: migration collision renumbered to `000061_budget_prompt_sha` (next unused sequence); frozen-count ABAC exemptions re-baseline passes.

## Tests the independent verifier must run

(gyre-server test execution exceeds the review sandbox's build budget; type-checked here via `cargo check --lib --tests`.)

```
SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib -- git_tree_template_injects_view_spec_grammar
SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib -- explorer_views
SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib -- prompt llm rate assist briefing specs save builtin developer jwt
SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib -- budget_call signal_chain
SKIP_WEB_BUILD=1 cargo test -p gyre-server --test explorer_ws_integration
cargo test -p gyre-adapters
cargo test -p gyre-domain
```

## Non-blocking observations (carried from prior round, unchanged)

- All three endpoints buffer the LLM stream server-side before emitting SSE events, so `partial` events arrive together rather than incrementally (pre-existing base design; the SSE event contract itself is met).
- Coverage rows for ui-layout.md are promoted by the spec-fidelity auditor per process, not by this task.
