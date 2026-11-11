# Review — task-171 (LLM Endpoint Contract — SSE streaming, prompt templates, rate limiting)

**Candidate:** `5dc7c5b9221b5443f927cb7ff92a4f9342d3a3c3` (branch `pipeline/task-171/9fc567ff60ee4aae8ffac92accb7bc27-1`, base `cfeefbda975211a6ac24a17e5920be6f8b23008b`, includes rebase-repair merge `12835877a`)
**Verdict:** NOT APPROVED — 2 findings (1 code/spec-fidelity, 1 docs). Evidence: `/tmp/stage/review-evidence/`.

## What was verified (independently, this round)

- **Diff review** of all 25 changed files: SSE error-event handling (connection + mid-stream failure) in all three endpoints; prompt-template resolution (DB override → repo git tree → hardcoded fallback) with git-SHA provenance into `BudgetCallRecord.prompt_template_sha` (migration `2026-10-09-000061`, next unused sequence); rate-limit principal keyed on auth `user_id` (agents on `agent_id`); per-endpoint token caps via `GYRE_LLM_MAX_TOKENS_{GENERATE,ASK,ASSIST}` (2000/4000/4000) threaded through the 4-param `predict_json`; `validate_prompt_path` confinement of the `prompts/save` review-gate bypass to `specs/prompts/`; `validate_repo_scope` cross-workspace repo_id rejection in `explorer-views/generate`; ABAC `RouteResourceMapping` entries with `action_override: "generate"` for all four routes plus the `developer-generate-access` builtin Allow (Owner/Admin/Developer workspace roles, Viewer denied — tested).
- **Mechanical checks** (all exit 0 on the candidate tree): check-arch, check-abac-route-registry, check-migration-versions, check-migration-sql-portability, check-template-substitution, check-mem-port-contracts, check-mcp-write-tools, check-in-memory-state-stores, check-forged-scope-fields, check-unbounded-external-http, check-dead-message-kinds, check-byte-slice-truncation, check-relative-path-defaults, check-fail-open-ref-resolution, check-fabricated-scope-defaults, check-lossy-secret-conversion, check-scope-literal-defaults, check-inert-enforcement, check-forwarded-header-trust, check-abac-exempt-handlers.
- **Mutation probe of the modified checker**: deleting the `("spec_content", ...)` tuple from `specs_assist.rs` makes `check-template-substitution.sh` FAIL (exit 1); restoring it passes (exit 0). The tuple-form extension did not weaken enforcement.
- **Compile + focused tests**: `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib --tests` → exit 0; `cargo check -p gyre-server --bins` → exit 0; `cargo test -p gyre-adapters --lib -- llm budget migration` → 43 passed; `cargo test -p gyre-domain` → 463+8 passed.
- **Commit attribution**: `check-task-commit-attribution.sh` exits 0 against the candidate HEAD (the working-tree failure observed during review was caused only by the stage machinery's revert of `specs/tasks/task-171.md` to `not-started`; verified in a clean worktree at the candidate SHA).

## Findings

1. **[code/spec-fidelity] `view_spec_grammar` substitution is dead in every shipped path** — `explorer_views.rs:602` substitutes `("view_spec_grammar", VIEW_SPEC_GRAMMAR)` but no template declares `{{view_spec_grammar}}`: the committed `specs/prompts/explorer-generate.md` (the git-tree path the spec makes primary) shows only the response envelope in Output Format, and `PROMPT_EXPLORER_GENERATE` enumerates the grammar inline without the placeholder. In the git-tree path the LLM never receives the view spec grammar schema, contradicting ui-layout.md §2 line 158 ("The server sends the LLM: the question, the list of available node types and counts in the workspace, and the view spec grammar schema") and the spec's template-format example (lines 199-201). No test asserts grammar injection. Fix: add `{{view_spec_grammar}}` to the committed template's Output Format and assert it in a grounding test. Evidence: `f1-view-spec-grammar-dead-substitution.txt`.
2. **[docs] briefing/ask complete-event payload misdocumented** — `docs/api-reference.md:198` (row rewritten by this task) says `event: complete` carries `{text}`; the handler sends `{"type":"complete","answer":...,"sources":...}` (graph.rs ~1880, HSI §1325; frontend reads `parsed.answer ?? parsed.text`). `text` is the partial-event field. Evidence: `f2-briefing-complete-payload-docs.txt`.

## Non-blocking observations

- All three endpoints buffer the LLM stream server-side before emitting SSE events, so `partial` events arrive together rather than incrementally (pre-existing base design, preserved; the SSE contract — POST → `text/event-stream` with `partial`/`complete`/`error` — is met).
- The old `{{draft_content}}` backward-compat replacement was dropped; a legacy DB-override template declaring it would now render the literal placeholder verbatim (visible, not silent). No shipped template uses it.
- Coverage rows for ui-layout.md §8-12 remain `task-assigned` — consistent with the process (the spec-fidelity auditor promotes rows, as it did for task-152/205).

## Tests the independent verifier must run

(gyre-server test compilation exceeds this sandbox's 4 GiB budget; type-checked here via `cargo check --lib --tests`.)

```
SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib -- prompt llm rate assist briefing explorer specs save builtin developer jwt
SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib -- budget_call signal_chain
SKIP_WEB_BUILD=1 cargo test -p gyre-server --test explorer_ws_integration
cargo test -p gyre-adapters
cargo test -p gyre-domain
```
