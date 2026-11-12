# Review — task-171 (LLM Endpoint Contract — SSE streaming, prompt templates, rate limiting)

Spec: `specs/system/ui-layout.md` §2 "LLM Endpoint Contract" (sections 8–12: LLM Endpoint Contract, Role, Available Data, Output Format, Constraints).
Base: `e402e1ed` · Candidate: `8bbdb9dd` (round: repair of the round-1 compile-break finding).
Verdict: **approved** — the round-1 blocking finding is fixed and independently verified; no new findings.

## Round 2 (this review) — repair verification

The prior round's sole finding: the candidate widened `LlmPort::predict_json` to take
`max_tokens: Option<u32>` but missed the `RecordingLlmPort` test mock in
`crates/gyre-server/src/api/specs_assist.rs`, leaving `cargo check -p gyre-server --tests`
broken (error[E0050]) so the task-171 suite could not run. Fix commit `474d6133` adds the
missing `_max_tokens: Option<u32>` parameter (one line, test-only; no production change).

Probes run at `8bbdb9dd` (evidence: `/tmp/stage/review-evidence/task171-r2/`):

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` → **exit 0** (`cargo-check-tests-gyre-server.log`) —
  the exact check that failed in round 1. The test target now type-checks, so
  `cargo test -p gyre-server` can compile and run in the verifier.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib -- llm::mock budget_call` → **14/14 passed**
  (`cargo-test-gyre-adapters.log`), including `save_then_list_round_trips_every_field`, which
  asserts `prompt_template_sha` persists through the real SQLite adapter (migration 000061),
  and the new `failing`/`stream_error_after` mock constructors' tests.
- `cargo test -p gyre-domain -p gyre-common` → **606 passed** (`cargo-test-domain-common.log`).
- Mechanical guards: `check-arch`, `check-abac-exempt-handlers`, `check-migration-versions`,
  `check-migration-sql-portability`, `check-forged-scope-fields`, `check-in-memory-state-stores`,
  `check-mem-port-contracts`, `check-byte-slice-truncation`, `check-relative-path-defaults`,
  `check-unbounded-external-http`, `check-lossy-secret-conversion`, `check-scope-literal-defaults`,
  `check-fabricated-scope-defaults`, `check-inert-enforcement`, `check-dead-message-kinds`,
  `check-mcp-write-tools`, `check-forwarded-header-trust`, `check-fail-open-ref-resolution` →
  all OK; `check-template-substitution` → OK.
- `check-task-commit-attribution.sh` → FAIL **against the stage's working-tree snapshot only**
  (the harness reverts `specs/tasks/task-171.md` to its pre-task `commits: []` text for review
  comparison — matches `/tmp/stage/review-before.json`). Re-run with the candidate's committed
  task file checked out → **OK**
  (`script-check-task-commit-attribution-head.log`, exit 0). Not a candidate defect; the working
  tree was restored to the stage snapshot afterwards.

Diff review of `e402e1ed..8bbdb9dd` (28 files, +2461/−298), re-verifying the round-1 approvals
still hold at the merged/rebased candidate:

- **SSE contract**: all three endpoints (`explorer-views/generate`, `briefing/ask`,
  `specs/assist`, plus `meta-specs/assist` and the MCP `spec_assist` tool) stream
  `partial`/`complete`/`error` via axum `Sse` with keep-alive. Connection failures return a
  200 SSE stream with a single `error` event; mid-stream chunk failures surface as `error`
  (the old `filter_map(|r| r.ok())` silent-drop is gone). Tests drive the real router and
  parse the actual SSE wire format, asserting error events never co-occur with `complete`.
- **Prompt templates**: `resolve_prompt_template` resolves DB override → repo git tree
  (`specs/prompts/<fn>.md` on the default branch, real `Git2OpsAdapter` reads) → hardcoded
  fallback, carrying the branch-head SHA for the cost entry. `substitute_template` does exact
  `{{var}}` replacement (non-recursive, no partial-name clobbering — all unit-tested). The
  committed `specs/prompts/*.md` templates declare exactly the variables the handlers inject;
  `check-template-substitution.sh` enforces the wiring mechanically. The grammar-grounding
  test commits the real template bytes into a real bare repo and asserts the captured system
  prompt contains `VIEW_SPEC_GRAMMAR` and no literal `{{` leakage.
- **Rate limiting**: 10 req/min per (user, workspace) sliding window (`VecDeque<Instant>`),
  keyed on `rate_limit_principal` (auth-context `user_id`, else `agent:{agent_id}` — not the
  collision-prone display name), enforced after membership check; 429 + `Retry-After`;
  eviction loop wired in `main.rs` (`spawn_llm_rate_limiter_cleanup`). Rate-limit handler
  test drives 11 real requests through the router.
- **Budget**: `llm_query` cost entries + per-call `BudgetCallRecord` with
  `prompt_template_sha` (domain struct, SQLite + Postgres adapters, portable migration
  000061, mem adapter, all in sync — `check-mem-port-contracts` green). Tenant scope is
  derived from the looked-up workspace; unresolvable workspace → skip + warn, no fabricated
  "default" scope.
- **Model/token limits**: workspace `llm_model` → `GYRE_LLM_MODEL` → hardcoded default;
  `GYRE_LLM_MAX_TOKENS_{GENERATE,ASK,ASSIST}` (2000/4000/4000) threaded through the widened
  `predict_json`/`stream_complete`; non-capped callers pass `None`.
- **ABAC**: `RouteResourceMapping` entries for all four LLM routes (`explorer_view`/`spec`/
  `workspace` with `generate` action override), a `builtin-developer-generate-access` Allow
  policy keyed on `subject.workspace_role` (Owner/Admin/Developer; Viewer denied — tested
  both ways with real JWTs), and the modified `check-abac-route-registry.sh` is a net
  tightening: production-region-scoped duplicate scan (with a sanity guard so a misplaced
  `#[cfg(test)]` cannot gut it), frozen counts lowered (exemptions 50→46 matching the actual
  file, duplicates 17→2 with both residual hits verified as a doc-comment example and
  gate-agent allowlist consts, not real resolver duplicates), and the previously-ignored
  `FAIL=1` exit code now honored.
- **Scope safety**: caller-supplied `repo_id` on generate is validated against the workspace
  (403 on foreign repo — tested); `prompt_path` on `prompts/save` is component-validated to
  be strictly inside `specs/prompts/` (traversal, absolute, `..`, wrong-dir, and empty forms
  all rejected with 400 — tested), confining the MR-review bypass the spec grants to
  `specs/prompts/` only.
- **MCP parity**: `handle_spec_assist` mirrors the REST handler's template resolution,
  meta-spec binding, and rate-limit principal.

Tests the independent verifier must run (large server test binary, deferred per the 4 GiB
budget — compilation itself is proven green by the `cargo check --tests` probe above):

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server` — in-module suites: `api/explorer_views.rs`
  (SSE generate: valid/invalid spec, error events, rate limit, budget record, grammar
  grounding, foreign repo_id), `api/graph.rs` (briefing ask: connection + mid-stream
  failure error events), `api/specs_assist.rs` (assist error events, prompt-path guard,
  prompts/save), `api/budget.rs` (llm_query record with prompt SHA), `abac_middleware.rs`
  (`developer_jwt_can_generate_on_llm_endpoints`), `llm_helpers.rs` (substitution, token
  limits, git-tree template resolution), `llm_rate_limit.rs`, `mem.rs` (budget call tests),
  plus `tests/explorer_ws_integration.rs`.

Test-inflation check: none. New tests drive the real router with real git state and
error-injection mock constructors; the grammar test asserts on the mock-captured system
prompt (not mirrored substitution lists); negative paths (foreign repo 403, bad prompt_path
400, Viewer 403, truncated stream) are all exercised.

Scope check: no out-of-scope production edits; `signal_chain.rs`/`mcp.rs`/`spawn.rs`/
`rig_vertexai.rs` changes are signature-propagation and parity only, with unchanged
behavior for non-capped callers.

## Round 1 (recorded for continuity; superseded by the repair above)

- Finding F1 (blocking): `RecordingLlmPort::predict_json` mock in `api/specs_assist.rs`
  (~line 2898) not updated for the widened trait → test build broken. Fixed in `474d6133`,
  verified this round.
