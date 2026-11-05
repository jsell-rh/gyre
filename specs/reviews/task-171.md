# Review — task-171 (LLM Endpoint Contract — SSE streaming, prompt templates, rate limiting)

Spec: `specs/system/ui-layout.md` §2 "LLM Endpoint Contract" (Role / Available Data / Output Format / Constraints).
Candidate under review: `3ff29841440800c0c23818f3c31600a7a04f4456` (base `d81b0fe4`). This round is the repair of the two findings from the review of `2cd03cf4` (commit `7fcbf272` plus docs/process commits `c161b28c`, `3ff29841`).
Verdict: **needs-revision** (1 new finding; both prior findings verified repaired).

## Round 2 — repair verification

Both findings from round 1 are independently confirmed repaired:

- **F1 (ABAC registry script false positive + invisible FAIL) — repaired.** `scripts/check-abac-route-registry.sh` now scopes both the coverage scan and the duplicate scan to the production region of `abac_middleware.rs` (everything before `#[cfg(test)]`), with a sanity guard that the stripped region still contains `RouteResourceMapping`. Independently verified: production-region duplicates are the identical 6 pre-existing paths at base and at candidate (whole-file count 22→27 exactly as the round-1 finding described); `FROZEN_DUPLICATE_COUNT=6` is therefore the true resolver-duplicate freeze, not a weakened baseline. The exit-code bug is fixed (the `FAIL` flag is now honored). Mutation probes (source reverted after each): a synthetic duplicate resolver entry → `FAIL: 7 duplicate path entries (baseline: 6)`, exit 1; deleting a resolver entry → `FAIL: 1 registered route(s) missing` printing the actual route, exit 1. Script on HEAD: OK, exit 0.
- **F2 (dead error-injection mocks / no SSE error-event coverage) — repaired.** `MockLlmAdapter::failing()` / `stream_error_after()` are wired into five handler tests (briefing/ask ×2, specs/assist ×2, explorer-views/generate connection failure), each asserting 200 + `text/event-stream`, exactly one `event: error`, zero `complete` events, and the failure cause in the payload. All 5 pass. Mutation probe (reviewer-performed, reverted): reintroducing the `filter_map(|r| r.ok())`-equivalent silent-drop in both `graph.rs` and `specs_assist.rs` fails both mid-stream tests (2/2 FAILED); restored → green. The mocks are no longer dead code (`cargo test -p gyre-adapters -- mock`: 8/8 pass).

Other verification (all green, logs in `/tmp/stage/review-evidence/`):

- Focused suite `cargo test -p gyre-server --lib -- llm rate_limit briefing assist explorer_view prompt sse budget`: **153 passed, 0 failed**.
- Guard scripts: `check-abac-route-registry`, `check-abac-exempt-handlers`, `check-api-auth`, `check-abac-context-parity`, `check-arch`, `check-unbounded-external-http`, `check-in-memory-state-stores`, `check-dead-message-kinds`, `check-mem-port-contracts`, `check-migration-versions`, `check-migration-sql-portability` — all OK, exit 0.
- Rate limiter (`llm_rate_limit.rs`): correct 10/60 s sliding window keyed on auth-derived principal (user_id, else agent:), 429 + `Retry-After`, 60 s inactivity eviction via background task; applied in all three handlers and the MCP spec-assist path.
- Budget charging: real Diesel-backed `BudgetCallRepository` (SQLite + Postgres + mem, port-contract guard green); migration `2026-10-09-000056_budget_prompt_sha` adds `prompt_template_sha`; `record_llm_budget_call` persists the audit record and increments workspace/tenant counters. The `000056` sequence reuse vs `2026-10-08-000056_spec_assertion_results` is legal under real Diesel version semantics (version = date+sequence, both distinct, both run) — both mechanical guards pass; noted only as an observation against the AGENTS.md "next unused sequence" shorthand.
- `rig_vertexai.rs`: all reqwest clients now latency-bounded (120 s adapter, 10 s OAuth, 5 s metadata); unbounded-HTTP exemption list shrunk 5→0 (a shrink, allowed).
- Scope handling in `explorer-views/generate`: workspace membership + tenant check, foreign `repo_id` → 403 (tested), hallucinated foreign repo_id in LLM output nulled with fallback (tested).

Findings:

- [-] **F1 (major, new): `POST /repos/:id/prompts/save` commits a caller-supplied `prompt_path` to the default branch with zero path validation — the no-review direct-commit escape hatch is not confined to `specs/prompts/`.** `save_prompt` (`crates/gyre-server/src/api/specs_assist.rs:814-836`) loads the repo and passes `req.prompt_path` straight to `git_ops.write_file` on the default branch. The spec's entire justification for skipping MR review, approval notifications, and the `trust:require-human-mr-review` gate (ui-layout.md line 181; the handler's own doc comment repeats it) is that `specs/prompts/` sits in spec-lifecycle's `ignored_paths` — but nothing constrains the path to that directory, so any caller passing the `generate` ABAC gate (workspace_role Developer+) can overwrite ANY tracked file (e.g. `specs/system/vision.md`, `scripts/check-arch.sh`, CI config) directly on the default branch with no MR, no approval, no gate. Reviewer probe (temporary test, reverted; `/tmp/stage/review-evidence/save-prompt-path-probe.log`): `prompt_path` values `specs/system/vision.md`, `scripts/check-arch.sh`, and `../../etc/passwd` each return **201 Created** with a commit SHA. Remedy: validate `prompt_path` in the handler — relative, canonical (no `..` components), and under `specs/prompts/` — rejecting everything else with 400, plus handler tests asserting rejection of out-of-directory and traversal paths.

Minor (not blocking, recorded for completeness):

- `briefing_ask` gates only on `require_workspace` (existence), unlike `generate_explorer_view`'s `check_workspace_membership` (tenant + membership). Pre-existing at the assigned base — not a regression of this diff — but the handler was otherwise reworked here; worth aligning in a follow-up.
- The `000056` migration sequence reuse across dates (see above) is mechanically legal but violates the AGENTS.md "next unused 6-digit sequence number" convention as literally written; use `000057` next.

Evidence: `/tmp/stage/review-evidence/` (script-dup-mutation.log, script-missing-mutation.log, sse-error-tests.log, midstream-mutation-probe.log, midstream-restore-check.log, task171-focused-suite.log, adapters-mock-tests.log, save-prompt-path-probe.log, save-prompt-restore-check.log, plus guard-script logs).
