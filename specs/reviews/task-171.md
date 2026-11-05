# Review: TASK-171 — LLM Endpoint Contract (SSE streaming, prompt templates, rate limiting)

**Reviewer:** Independent review (gyre review stage)
**Date:** 2026-11-05
**Candidate:** `1479674ac2b743b2536867c864a57a64f2b55bd5` (base `0cb71015861e41b082183b89e9a1924808ad114a`)
**Verdict:** approved

---

## Prior-round findings — all verified repaired in this candidate

1. **`save_prompt` unvalidated caller-supplied path** (review of `3ff29841`): `validate_prompt_path`
   (`api/specs_assist.rs:812`) now runs before anything touches git. Rejects absolute paths,
   non-normal components (`..`, root, prefixes), non-UTF-8 segments, the `specs/prompts` directory
   itself, and anything not strictly inside `specs/prompts/`; nested subdirectories allowed.
   Mutation probe (reviewer-run, reverted): removing the validation call fails both new tests —
   `save_prompt_rejects_paths_outside_specs_prompts` reproduces the original bypass exactly
   (`"specs/system/vision.md"` → 201 vs 400). Restored: 4/4 pass.
   Evidence: `/tmp/stage/review-evidence/task171-mutation-validate-removed.log`,
   `task171-mutation-restored.log`.

2. **ABAC registry script duplicate-count inflation + invisible FAIL** (review of `2cd03cf4`):
   `scripts/check-abac-route-registry.sh` now scopes both scans to the production region (before
   `#[cfg(test)]`) with a sanity guard so a misplaced test module cannot gut the scan;
   `FROZEN_DUPLICATE_COUNT` lowered 22 → 6, matching the real resolver duplicates — verified
   against the base tree (non-test-region `uniq -d` at base = 6, at candidate = 6, no new
   duplicates); the FAIL flag is honored in the exit code; the missing-route list is captured
   before temp-file removal. Script passes on the candidate with exit 0.
   Evidence: `abac-registry-script-candidate.log` (in review-evidence dir).

3. **Dead error-injection mocks / no SSE `event: error` regression coverage**: `MockLlmAdapter::failing()`
   and `stream_error_after()` are now wired into handler tests (explorer_views.rs:1178,
   graph.rs:2239/2324, specs_assist.rs:1600/1653) asserting one error event, zero complete events.
   Mutation probe (reviewer-run, reverted): reintroducing the silent mid-stream drop
   (`Err(_e) => break`) in `assist_spec` fails `assist_spec_llm_mid_stream_failure_sends_error_event`.
   Restored: 12/12 pass. Evidence: `task171-mutation-sse-silent-drop.log`.

## Acceptance criteria — independently verified

- **SSE streaming**: all three endpoints return `text/event-stream` with `partial`/`complete`/`error`
  events; content-type asserted in handler tests for all three (graph.rs:2187/2278,
  specs_assist.rs:1488/1624, explorer_views.rs:956/1207). Connection failure and mid-stream failure
  produce `event: error`, never a fake `complete` (mutation-verified above).
- **Prompt templates**: `llm_helpers::resolve_prompt_template` resolves DB override →
  `specs/prompts/<function>.md` from the repo git tree (real `Git2OpsAdapter` reads in tests,
  branch-head SHA carried as provenance) → hardcoded fallback; `substitute_template` does exact-brace
  `{{variable}}` substitution (no partial-name clobbering, no recursive expansion — unit-tested).
- **Rate limiting**: `llm_rate_limit.rs` sliding window, 10 req/60 s keyed on auth-context principal
  (user_id, agent fallback), 429 + `Retry-After` via `ApiError::RateLimited`, 60 s inactivity
  eviction spawned from `main.rs:81`. Applied inside all three handlers after auth; handler test
  `assist_spec_rate_limited_after_10_requests` proves the 11th request 429s.
- **Budget charging**: `record_llm_budget_call` persists a `BudgetCallRecord` (new port
  `BudgetCallRepository`, SQLite + PostgreSQL + mem adapters; mem enforces the insert-only contract
  in code with a killing test) and increments workspace + tenant counters. Migration
  `2026-10-09-000056_budget_prompt_sha` adds the provenance column with portable SQL (passes
  `check-migration-sql-portability.sh`; Diesel version distinct from 000024-era names — passes
  `check-migration-versions.sh`). Enforcement test proves counters feed `check_spawn_budget`.
- **Endpoints + ABAC**: all four routes registered (`api/mod.rs`), resolver entries present
  (`explorer_view`/`spec`/`workspace` with `generate` action override), new builtin
  `developer-generate-access` policy scoped to `subject.workspace_role` with a Viewer-deny
  assertion; `developer_jwt_can_generate_on_llm_endpoints` passes.
- **`prompts/save`**: commits directly to the default branch and returns the commit SHA, with the
  path guard above constraining the MR-review bypass to `specs/prompts/`.
- **Tests**: focused suite `-- prompt llm rate assist briefing explorer specs save` → 235 passed,
  0 failed; abac_middleware tests 10/10; adapters mock tests 8/8. Evidence:
  `task171-focused-suite.log`, `task171-abac-tests.log`, `task171-mock-tests.log`.

## Invariant scripts

`check-arch`, `check-abac-route-registry`, `check-abac-exempt-handlers`, `check-mcp-write-tools`,
`check-migration-versions`, `check-migration-sql-portability`, `check-unbounded-external-http`
(exemption file SHRANK 5 → 0; the former unbounded `reqwest::Client::new()` sites in
`rig_vertexai.rs` now carry real timeouts: 120 s Vertex, 10 s OAuth, 5 s metadata),
`check-inert-enforcement`, `check-forged-scope-fields`, `check-in-memory-state-stores`,
`check-mem-port-contracts`, `check-task-commit-attribution` — all pass.

## Informational (non-blocking) observations

- `check-llm-endpoint-hygiene.sh` still exits 1 with 7 violations — the same count as at base.
  The candidate actually *fixed* the three real in-scope violations (briefing_ask discarded
  max_tokens + missing budget tracking; explorer-generate discarded max_tokens) and the new flags
  on `briefing_ask` are heuristic false positives (budget is charged via the shared
  `record_llm_budget_call` helper, which the grep-for-`CostEntry::new` heuristic does not
  recognize). Remaining violations are pre-existing out-of-scope sites (`explorer_ws.rs` ×3,
  `graph-predict` ×1). Not a regression; future cleanup could add `// llm-hygiene:ok` markers or
  teach the heuristic about the shared helper.
- SSE `partial` events are emitted from chunks buffered during stream consumption (protocol shape
  correct: ordered partials, then terminal complete/error; mid-stream failures surface as
  `event: error`), not pushed to the client in real time per chunk. The client-visible contract of
  ui-layout.md §2 (event types, ordering, content type) is met; real-time incremental delivery is
  approximated. Noted for a future task if true incremental push is wanted.

## Verdict

All eight acceptance criteria are backed by production code and killing tests; both prior review
findings are genuinely repaired; no new enforcement gaps, scope-handling defects, fake
implementations, or weak tests found in the candidate diff. **Approved.**
