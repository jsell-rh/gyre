# Review — task-171 (LLM Endpoint Contract — SSE streaming, prompt templates, rate limiting)

Spec: `specs/system/ui-layout.md` §2 "LLM Endpoint Contract" (+ its Role / Available Data / Output Format / Constraints subsections).
Base `cba416b46`, candidate `6f391d83` (HEAD; 18 recorded product commits plus rebase merges and process commits, all attributed in the task frontmatter).
Verdict: **approved** — no blocking findings. Independent probe evidence under `/tmp/stage/review-evidence/task-171-r2/`.

## What the candidate ships (diff-verified)

- **SSE contract on all three LLM endpoints.** `explorer-views/generate`, `briefing/ask`, and `specs/assist` all emit `event: partial` (explanation/answer text only; `view_spec` and `diff` arrive only in `complete`), `event: complete` (final JSON), and — new this round — `event: error` for both connection failures and **mid-stream** chunk failures. The old `filter_map(|r| r.ok())` silently dropped stream errors and completed on truncated text; the new loop records the error and returns an error-only SSE stream, never a `complete` built from partial text. All three handlers use axum `Sse` with keep-alive.
- **Prompt templates from the repo git tree.** New `llm_helpers::resolve_prompt_template`: DB override → `specs/prompts/<function>.md` read from the workspace repos' default branch via the real `GitOpsPort` (with branch-head SHA provenance) → hardcoded fallback. `substitute_template` does exact-brace `{{var}}` substitution; unknown vars stay visible. The committed `specs/prompts/explorer-generate.md` now declares `{{view_spec_grammar}}`, grounded by `VIEW_SPEC_GRAMMAR` mirroring the grammar `parse_and_validate` enforces. `briefing-ask`/`specs-assist` templates carry `{{briefing_json}}/{{graph_summary}}/{{history}}` and `{{meta_spec_context}}` respectively, all substituted by the handlers (and by the MCP `specs/assist` tool for parity); the workspace meta-spec-set is rendered from the pinned repos' git trees.
- **Token limits.** `predict_json` gained a `max_tokens` parameter, threaded through `rig_vertexai` (real adapter applies it), the mock, and every caller. `max_tokens_for_function` resolves `GYRE_LLM_MAX_TOKENS_{GENERATE,ASK,ASSIST}` → 2000/4000/4000 defaults, pure (env lookup injected) so tests don't race on process env.
- **Rate limiting** keyed on `rate_limit_principal` (auth-context `user_id`, falling back to `agent:<agent_id>`), 10 req/min sliding window, 429 + `Retry-After`; eviction loop wired in `main.rs` (pre-existing, unchanged semantics).
- **Budget charging with provenance.** `BudgetCallRecord.prompt_template_sha` added end-to-end: domain field, SQLite + Postgres adapters, migration `000061` (next unused sequence; plain `ALTER TABLE ... ADD/DROP COLUMN` portable to both backends), mem adapter, `record_llm_call_usage`, and all four call sites (three endpoints + agent usage with `None`). `generate` now charges unconditionally against the already-loaded workspace instead of warn-and-skip.
- **ABAC.** `generate` action overrides were already mapped for all four routes; the candidate adds the `builtin-developer-generate-access` Allow (workspace_role Owner/Admin/Developer, tenant scope, priority 800) seeded via the production `seed_builtin_policies` path in `main.rs`, with a Viewer-denied negative test.
- **`prompts/save` hardening.** `validate_prompt_path` constrains the direct-to-default-branch review bypass to strictly-inside `specs/prompts/` using `Path::components` (rejects `..`, absolute, prefix, and wrong-directory forms) before anything touches git — closing the hole where the MR-review bypass could reach any tracked file.
- **Grounding scope safety.** Caller-supplied `repo_id` in `generate` is validated against the authorized workspace (403 on foreign repo) before it scopes the graph summary, template lookup, and budget record.

## Probes run (evidence: `/tmp/stage/review-evidence/task-171-r2/`)

- Full invariant battery: arch, ABAC registry, ABAC exempt handlers, template substitution, migration versions + SQL portability, MCP write tools, mem port contracts, dead message kinds, byte-slice, relative paths, fail-open refs, scope literals, fabricated scope defaults, lossy secrets, forwarded headers, in-memory stores, unbounded HTTP, inert enforcement, forged scope fields — **all exit 0**.
- `cargo fmt --check` → exit 0 (the prior verification round's changed-line rustfmt failures are repaired).
- `cargo test -p gyre-adapters llm::mock` → 11 passed (incl. the new `failing`/`stream_error_after` mock modes). `cargo test -p gyre-adapters --lib budget_call` → 3 passed (SHA round-trip, duplicate-id rejection, workspace scoping).
- `cargo check -p gyre-server --tests` → exit 0. `cargo clippy -p gyre-ports -p gyre-domain` and `-p gyre-server` → exit 0; no warning location intersects the candidate's changed lines (remaining warnings are pre-existing unchanged lines, incl. 259 `double_must_use` on port traits that never had the allow — `llm.rs` now matches the user.rs/tenant.rs/compute.rs convention).
- `check-task-commit-attribution.sh` → exit 0 against the committed task file (the working-tree copy is reset by the review harness to the assignment body; restored after probing).
- **Guard-script modification independently reproduced:** the ABAC registry script's new production-region scan yields exactly two duplicate literals (`/api/v1/tasks/:id` doc-comment example, `/api/v1/version` allowlist consts) — both non-resolver; the old whole-file count of 22 included test-router fixtures. The baseline changes (17→2 duplicates, 50→46 exemptions) tighten to reality, and the FAIL-exit repair makes previously invisible failures enforce. Not a weakening.

## Verifier must run (blocked here by the 4 GiB server-test build wrapper)

- `cargo test -p gyre-server --lib -- llm_rate_limit llm_helpers explorer_views specs_assist graph abac_middleware budget_call_tests`
- `cargo test -p gyre-server --test explorer_ws_integration`
- `cargo clippy --all-targets` (changed-line gate) + standard battery + web build/tests (no authored web changes in this candidate).

## Non-blocking observations (recorded, not defects)

- `specs/coverage/system/ui-layout.md` rows 8–12 remain `task-assigned`; flipping them to `implemented` belongs to the completion/controller stage after this approval — do not lose them.
- The `{{draft_content}}` backward-compat substitution was dropped from `specs/assist`; neither the committed template nor the hardcoded default uses it, and draft content still reaches the prompt as `{{spec_content}}` (test `assist_spec_draft_content_overrides_repo_content` covers the override). Harmless.
- `resolve_prompt_template` does not fall back to scanning sibling workspace repos when a `repo_hint` repo lacks the file — a semantics choice, not a spec violation (the hardcoded fallback applies).
- The `partial` event on `explorer-views/generate` is a single "Generating view..." placeholder rather than streamed explanation chunks, because that endpoint uses the structured `predict_json` path; `briefing/ask` and `specs/assist` stream real chunks. Consistent with the task's "endpoint stubs (actual LLM integration in later tasks)" framing and the acceptance criterion "endpoints registered and return SSE streams".

Test-inflation check: none found. The new tests drive the real router via `oneshot`, parse real SSE bodies, commit real templates into real git trees (`Git2OpsAdapter`) and assert on the system prompt the mock LLM actually received (`captured_system_prompts`), inject real connection/mid-stream failures, sign real JWTs against the real ABAC middleware with seeded production policies, and assert exact 429/`Retry-After`/400/403 outcomes — each fails if the corresponding production behavior is disabled.
