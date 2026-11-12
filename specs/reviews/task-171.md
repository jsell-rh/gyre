# Review — task-171 (LLM Endpoint Contract — SSE streaming, prompt templates, rate limiting)

Spec: `specs/system/ui-layout.md` §2 "LLM Endpoint Contract" (coverage rows 8-12:
LLM Endpoint Contract, Role, Available Data, Output Format, Constraints).
Candidate under review: `c10b348f770c09c69135d6b972dcb366e5a4d647` (base
`ceefc14afc9f5f30f0494a6a983ae864569e1d44`). This round is the rebase repair
after the published PR (head `c06422789`) conflicted with current main; the
single conflict (`crates/gyre-server/src/mem.rs` gyre-ports import union) was
resolved in merge `24a1371ab`. No prior approval was carried forward — the full
base→candidate diff (~2,400 lines across 26 files) was re-inspected.

Verdict: **approved** — no blocking findings. Independent probes below; server
test-binary execution deferred to the verifier per the 4 GiB build-budget
protocol (exact commands recorded).

## Probes run (evidence in `/tmp/stage/review-evidence/task-171/`)

- `cargo check -p gyre-server` → **exit 0**, zero warnings (only build-script
  notes; build.rs also ran `npm ci` + `npm run build` for the web bundle
  successfully during the check).
- `cargo fmt --check -p gyre-server -p gyre-adapters -p gyre-ports -p gyre-domain`
  → exit 0.
- `cargo clippy -p gyre-ports -p gyre-domain` → the residual `double_must_use`
  warnings are all pre-existing on unchanged files (verified via
  `--message-format=json`: none in `llm.rs`, so the candidate's
  `#[allow(clippy::double_must_use)]` on `LlmPort` is effective); CI's
  diff-scoped clippy gate (`scripts/check-clippy-diff.py`) covers changed lines.
- `cargo test -p gyre-adapters mock` → **11/11 passed**, including the new
  error-injection constructors `MockLlmAdapter::failing` (every call errors)
  and `stream_error_after` (partial chunks then a stream error) that back the
  SSE `event: error` tests.
- `cargo test -p gyre-adapters budget_call` → **3/3 passed**: full-field
  round-trip including `prompt_template_sha`, duplicate-id rejection,
  workspace/since/limit filtering newest-first.
- Mechanical gates, all **exit 0**: `check-arch.sh`, `check-abac-route-registry.sh`,
  `check-abac-exempt-handlers.sh`, `check-migration-versions.sh`,
  `check-migration-sql-portability.sh`, `check-mem-port-contracts.sh`,
  `check-inert-enforcement.sh`, `check-mcp-write-tools.sh`,
  `check-template-substitution.sh`, and `check-task-commit-attribution.sh`
  (the attribution gate initially FAILed only because the stage harness reset
  the working-tree `task-171.md` to `progress: not-started, commits: []`;
  against the committed candidate tree it exits 0 — probe run with the
  committed file checked out, working tree restored afterward).

## Acceptance criteria — code-level verification

- **SSE event format**: all three endpoints emit `partial`/`complete`/`error`
  events with `Content-Type: text/event-stream` and 15 s keep-alive.
  `briefing/ask` and `specs/assist` stream real `stream_complete` chunks as
  `partial` events; both connection failure and **mid-stream** failure now
  surface as `event: error` (the old `filter_map(|r| r.ok())` silent-drop is
  gone, with regression tests on both endpoints using the new mock
  error-injection constructors). `explorer-views/generate` uses non-streaming
  `predict_json` and emits one synthetic `partial` before `complete` —
  consistent with the task plan's "endpoint stubs (actual LLM integration in
  later tasks)"; LLM failure there is an `error` event, never a 500.
- **Prompt templates**: `resolve_prompt_template` resolves DB override →
  `specs/prompts/<fn>.md` from the repo's real git tree (via `GitOpsPort`
  `read_file`, with the default-branch head SHA as provenance) → hardcoded
  fallback. `substitute_template` does exact-brace `{{var}}` replacement;
  unknown variables stay visible. The committed
  `specs/prompts/explorer-generate.md` now declares `{{view_spec_grammar}}`
  (repair `c5eb3911b`), and an end-to-end test commits the exact template
  bytes (`include_str!`) into a real bare git repo and asserts via the mock's
  captured system prompt that the grammar reached the LLM with no `{{` leakage.
- **Rate limiting**: sliding-window `HashMap<(principal, workspace),
  VecDeque<Instant>>` behind a tokio `Mutex`, 10 req/60 s, 429 +
  `Retry-After` (integration test drives 10 successes then a 429). Principal
  keyed on auth-context `user_id` (agents on `agent:{agent_id}`) — repair of
  the old display-name keying. Background eviction (`evict_stale_entries`)
  spawned from `main.rs`. Applied on all three endpoints plus MCP parity and
  `predict_graph`.
- **Budget charging**: `llm_query` `BudgetCallRecord`s via
  `record_llm_call_usage`, carrying `prompt_template_sha` (new migration
  `2026-10-09-000061_budget_prompt_sha`, portable SQL, schema +
  SQLite/Postgres adapters + mem adapter with in-code duplicate-id guard).
- **Endpoints + ABAC**: all three routes plus `prompts/save` registered;
  `RouteResourceMapping` entries with `action_override: "generate"`
  (`explorer_view` / `workspace` / `spec` resources); new builtin
  `developer-generate-access` Allow (priority 800, workspace_role
  Owner/Admin/Developer) so `default-deny` doesn't 403 every LLM call — test
  proves Developer-JWT allowed on all four paths and Viewer denied.
- **`prompts/save`**: commits directly to the default branch and returns
  `{commit_sha}`; `validate_prompt_path` constrains the review-gate bypass to
  relative paths strictly inside `specs/prompts/` (rejects traversal,
  absolute, `..`, non-`specs/prompts` targets) with an exhaustive bad-path
  test table.
- **Scope safety**: caller-supplied `repo_id` on `explorer-views/generate` is
  validated against the path workspace (403 on foreign repo, test provided);
  `briefing/ask` keeps its per-handler `check_repo_abac` guard; the user's
  question/instruction travel only in the user prompt, with template
  placeholders blanked (injection containment).

## Deferred to the independent verifier (test compilation exceeds this
sandbox's 4 GiB budget — the verifier must run and pass all of these)

- `cargo test -p gyre-server explorer_views`
- `cargo test -p gyre-server specs_assist`
- `cargo test -p gyre-server briefing_ask`
- `cargo test -p gyre-server llm_helpers`
- `cargo test -p gyre-server llm_rate_limit`
- `cargo test -p gyre-server budget_call` (mem adapter duplicate-id test)
- `cargo test -p gyre-server developer_jwt_can_generate`
- `cargo test -p gyre-server --test explorer_ws_integration`
- Full workspace: clippy (diff-scoped gate), fmt, `cargo test --all`, web
  build + `cd web && npm test`.

## Non-blocking observations (recorded, no action required this round)

1. `mcp.rs handle_spec_assist` still collects its stream with
   `filter_map(|r| r.ok())`, so a mid-stream drop yields a truncated text that
   then fails JSON parsing and returns a tool error (visible failure, not a
   fake success). The REST handlers were hardened; MCP parity for mid-stream
   errors is a future nit — ui-layout.md §2's SSE contract governs the REST
   surface.
2. Meta-spec-set binding (`{{meta_spec_context}}`) is injected into
   `specs-assist` (REST + MCP). The committed `explorer-generate` and
   `briefing-ask` templates declare no such variable, so nothing is dropped
   today; a custom template adding it on those endpoints would see the
   literal placeholder (unknown variables stay visible by design).
3. `explorer-views/generate` emits one synthetic `partial` (predict_json is
   non-streaming) — acceptable under the task's "endpoint stubs, actual LLM
   integration in later tasks" plan.
4. `specs/coverage/system/ui-layout.md` rows 8-12 still read `task-assigned`;
   coverage-matrix updates are the audit stage's responsibility, not this
   candidate's.

Generated-output note: no `web/dist/` changes are in this candidate; the
verifier-owned bundle rebuild is expected post-integration.
