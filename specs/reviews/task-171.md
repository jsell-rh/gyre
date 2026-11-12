# Review — task-171 (LLM Endpoint Contract — SSE streaming, prompt templates, rate limiting)

Spec: `specs/system/ui-layout.md` §2 "LLM Endpoint Contract" (+ §Role/Available Data/Output Format/Constraints).
Candidate under review: `98539b65017e2dbfffcecb789fa95c40a6b73000` (base `3afece9d73e0d76baf53c9983dede637f15449e8`).
Verdict: **needs-revision** — one blocking finding (F1, test build broken); the rest of the candidate verified clean and real.

## Round N (this review; merge-repair + rebase round)

Probes run (clean worktree of the candidate SHA where noted; main checkout is
on the assigned branch at the candidate):

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → **exit 0** (6m cold).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` → **FAIL, E0050**
  (`crates/gyre-server/src/api/specs_assist.rs:2898` — see F1). Log:
  `/tmp/stage/review-evidence/task171-final/cargo-check-tests-gyre-server.log`.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-adapters --tests` → exit 0.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-domain --tests` → exit 0.
- `cargo fmt --check` → exit 0.
- All 91 `scripts/check-*.sh` run at BASE and at the CANDIDATE (candidate via
  a clean worktree for the attribution check); **zero exit-code regressions**
  (`/tmp/stage/review-evidence/task171-final/gates-{base,cand}.txt`,
  `diff-gates.sh`). The attribution check's apparent failure in the live
  checkout is an artifact of the review controller's unstaged reset of
  `specs/tasks/task-171.md` (progress→not-started, commits→[]); at the clean
  candidate commit it prints OK.
- Gate-script edits audited against the frozen-baseline rules (see below).

### F1 (blocking, code): gyre-server lib test target does not compile

The candidate widened `LlmPort::predict_json` to take
`max_tokens: Option<u32>` (`crates/gyre-ports/src/llm.rs`), and updated every
implementor except the test mock `RecordingLlmPort` in
`crates/gyre-server/src/api/specs_assist.rs` (~line 2898):

```
error[E0050]: method `predict_json` has 3 parameters but the declaration in trait `predict_json` has 4
   --> crates/gyre-server/src/api/specs_assist.rs:2899
```

Consequences: `cargo test -p gyre-server` (lib) fails before running a single
test — the entire task-171 suite (SSE, rate-limit, template, budget, ABAC
tests) is un-runnable at this commit, so the acceptance criterion "Tests
pass" is unmet and none of the shipped test claims can execute. The mock in
`graph.rs` and the `ScriptedLlmPort` in `tests/explorer_ws_integration.rs`
were both updated; only this one was missed. Fix: add
`_max_tokens: Option<u32>,` to `RecordingLlmPort::predict_json` (the
compiler's own suggestion). The shipped evidence only claims
`cargo check -p gyre-server`, which does not compile test code — that is how
the break was missed.

### Verified real (no findings)

- **SSE contract.** All three endpoints (+workspace `meta-specs/assist`,
  +MCP parity) return `axum::response::sse::Sse` with `event: partial`
  (explanation/answer text only), `event: complete` (structured
  view_spec/diff/answer as a unit), and `event: error`. LLM connection
  failure and **mid-stream chunk failure** both surface as `error` SSE
  events, never a truncated `complete` (`graph.rs` briefing_ask,
  `specs_assist.rs` assist_spec/assist_meta_spec, `explorer_views.rs`
  generate). Generate validates the LLM output against the view-spec grammar
  and repo ownership before `complete`; invalid output → null view_spec +
  fallback, not a 500.
- **Prompt templates.** `llm_helpers::resolve_prompt_template`: DB override →
  `specs/prompts/<key>.md` read from a real workspace repo git tree via
  `GitOpsPort::read_file` (branch-head SHA recorded as provenance) →
  hardcoded fallback. `substitute_template` does exact-`{{var}}` replacement;
  unknown placeholders stay visible. A grounded end-to-end test
  (`git_tree_template_injects_view_spec_grammar_into_system_prompt`) commits
  the real `specs/prompts/explorer-generate.md` bytes (`include_str!`) into a
  real git repo and asserts on the system prompt a mock LLM actually
  received — no mirrored substitution list. Meta-spec-set binding implemented
  (`workspace_meta_spec_context`) with honest reporting of unreadable pins.
- **Rate limiting.** `HashMap<(principal, workspace), VecDeque<Instant>>`
  behind a Mutex, 10/60s sliding window, after auth; `rate_limit_principal`
  keys human callers on immutable `user_id` (agent fallback
  `agent:<agent_id>`); 429 + `Retry-After` via `ApiError::RateLimited`;
  eviction after 60s inactivity. Unit tests cover window/eviction/independence;
  router-level tests drive 10-then-429 for all three endpoints.
- **Budget.** `record_llm_call_usage` writes an `llm_query` BudgetCallRecord
  with `prompt_template_sha` (new column, migration
  `2026-10-09-000061_budget_prompt_sha` — Diesel version unique, `ALTER
  TABLE ... ADD COLUMN` portable to both backends per the portability gate);
  SQLite/Postgres/mem adapters all carry the field; mem adapter enforces the
  insert-only duplicate-id port contract in code with a test. Workspace
  lookup failure skips-and-logs rather than fabricating a scope.
- **ABAC.** Resolver mappings exist for all four LLM routes
  (`explorer_view`/`workspace`/`spec` with `action_override: "generate"`);
  new builtin `developer-generate-access` Allow (workspace_role ∈
  Owner/Admin/Developer) with a real middleware test: Developer JWT → 200 on
  all four routes, Viewer workspace role → 403. `prompts/save` commits
  directly to the default branch with strict `validate_prompt_path`
  containment (only plain components, strictly inside `specs/prompts/`) —
  the review-gate bypass cannot be extended to other paths.
- **Gate-script edits audited (not weakened).**
  `check-abac-route-registry.sh`: the old whole-file grep counted test-module
  route literals as resolver duplicates (BASE: 17 whole-file vs **2**
  production-region; candidate: 22 vs 2 — the +5 is main's merged test
  fixtures, not new resolver entries). The rewrite scans only the
  pre-`#[cfg(test)]` region, guards that the region still contains
  `RouteResourceMapping` (a misplaced test module fails loudly), now honors
  `FAIL=1` (previously printed FAIL but exited 0 — fail-open), and prints the
  missing-route list (previously `cat`-ed an already-deleted temp file).
  Baselines shrank (50→46, 17→2). `check-template-substitution.sh` now
  recognizes `substitute_template` tuple form only inside functions that
  actually call the helper. Both scripts pass at the candidate; no exemption
  file grew.
- `mcp.rs` MCP parity (same template resolution, rate-limit principal,
  budget SHA), `signal_chain.rs`/`spawn.rs` call-site updates for the trait
  change, `mem.rs` duplicate-id guard.

### Observations (non-blocking)

- **Buffered SSE:** `briefing_ask` and the assist handlers drain the entire
  LLM stream, then emit the `partial` events (and `explorer_views/generate`
  emits one synthetic "Generating view..." partial). The wire format and
  event semantics are correct, but progressive client rendering is not
  real-time; the buffering exists to keep mid-stream errors from
  masquerading as `complete`. Generate's explanation is server-generated
  after `predict_json` returns. Acceptable under the §2 event contract; note
  for future streaming work.
- **Migration numbering:** `2026-10-09-000061` reuses 6-digit sequence 61
  with a different date prefix (matches the pre-existing repo pattern, e.g.
  four `000056`s); the enforced check is on the full Diesel version and
  passes.

### Verifier must run (after F1 fix)

- `cargo test -p gyre-server --lib` (full lib suite; task-171 tests in
  `api/explorer_views.rs`, `api/graph.rs`, `api/specs_assist.rs`,
  `api/budget.rs`, `llm_helpers.rs`, `llm_rate_limit.rs`,
  `abac_middleware.rs`, `mem.rs`).
- `cargo test -p gyre-server --test explorer_ws_integration`.
- `cargo clippy --all-targets` and the full mechanical gate suite in CI.

Evidence: `/tmp/stage/review-evidence/task171-final/` (gate logs, cargo
check logs, base/candidate comparison scripts and outputs).
