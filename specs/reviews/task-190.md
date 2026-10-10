# Review — task-190 (Record real per-call LLM usage into budget counters and audit log)

Spec: `specs/system/platform-model.md` §5 Resource Governance → Budget Tracking.
Base: `8c2d177505852b3e39cd77f4f782fb355de245aa` — Candidate: `87532919b15eaa5f66bd47768346e66e0d4e9ae3`
(implementation commit `25feb84e5b870562c99b15814275c0f132c29cf2`, recovered from an interrupted
checkpoint per durable finding `6674a54d`; subsequent commits touch only task frontmatter).
Verdict: **approved**.

## Independent verification

All probes run against the candidate tree (full commands, sources, exit codes, and outputs in
`/tmp/stage/review-evidence/task-190-review.md`):

- **SQLite adapter round-trip** (`cargo test -p gyre-adapters budget_call`): 3/3 pass — every
  field round-trips through a real INSERT/SELECT against `budget_call_records` (diesel mapping
  verified against `schema.rs:620-635` and migration `2026-03-25-000024`), workspace/since/limit
  filtering newest-first, duplicate id rejected via PK.
- **Counter wiring** (`cargo test -p gyre-server --lib api::budget::tests`): 9/9 pass, including
  `record_llm_call_usage_increments_workspace_and_tenant_counters` (one call → workspace AND
  tenant `tokens_used_today`/`cost_today` incremented + one record retrievable) and
  `recorded_usage_makes_token_budget_limit_fire` (500/1000 → Ok; 1100/1000 → Err naming
  `max_tokens_per_day`).
- **Full HTTP path** (`cargo test -p gyre-server --lib api::spawn::tests::agent_usage_report...`):
  POST `/api/v1/agents/:id/usage` (600+400 tokens, $0.12) → exactly one `agent_run`
  `BudgetCallRecord` via `list_by_workspace`, workspace AND tenant counters at 1000/$0.12,
  `GET /api/v1/workspaces/:id/budget` reflects it.
- **Discrimination (kill) test**: temporarily replacing the `record_budget_usage` increment
  inside `record_llm_call_usage` with a no-op makes
  `recorded_usage_makes_token_budget_limit_fire` FAIL (panics "spawn must be rejected") — the
  tests genuinely depend on the production wiring, not on mirrored logic. Edit restored;
  `git status` clean, `git diff HEAD -- api/budget.rs` → 0 lines; both tests re-confirmed green.
- **Touched-module suites**: graph (13), specs_assist (21), explorer_views (8), mcp (71) — all
  pass; `cargo build --all` exit 0; `cargo test -p gyre-domain` 363/363.
- **Invariant checks**: check-arch, check-mcp-write-tools, check-mem-port-contracts,
  check-in-memory-state-stores, check-scope-literal-defaults, check-forged-scope-fields,
  check-abac-route-registry, check-abac-exempt-handlers, check-unwired-port-methods — all exit
  0. `git diff base..candidate -- scripts/` is empty (no exemption files touched, no frozen
  baselines widened).

## Verified real (no findings)

- **Port + adapters are real**: `BudgetCallRepository` in `gyre-ports` (no infrastructure deps);
  SQLite and Postgres implementations issue real diesel INSERT/SELECT on `budget_call_records`
  with tenant-scoped reads (`tenant_id.eq(&self.tenant_id)`, matching the sibling adapter
  pattern); mem adapter enforces the same duplicate-id contract in code
  (check-mem-port-contracts passes). Wired via `AppState.budget_calls` through the `store!`
  macro (PG > SQLite > mem) — the unwired-port-methods check passes with NO exemption entry,
  so the port is genuinely production-consumed.
- **Single recording entry point**: `budget::record_llm_call_usage` appends the
  `BudgetCallRecord` then delegates the counter increment to the pre-existing
  `record_budget_usage` (workspace + tenant). No increment logic duplicated at call sites; the
  pre-existing zero-caller defect of `record_budget_usage` is closed by construction.
- **Scope handling is safe**: every call site derives `tenant_id`/`workspace_id` from the
  workspace/repo row the handler already loaded (never caller-supplied form fields);
  `record_agent_usage` resolves the agent's own workspace row server-side and fails with
  Internal (not fabricated "default") if it is missing. No fabricated-scope or
  scope-literal-default patterns introduced (checks pass).
- **No hardcoded token counts**: agent path uses the reported `tokens_input`/`tokens_output`/
  `cost_usd` verbatim; llm_query paths split the pre-existing analytics estimate
  (prompt-estimate → input, remainder → output; briefing/ask uses prompt + produced text
  lengths) so input+output equals the estimate already charged to `CostEntry`. No invented
  values; `cost_usd: 0.0` on estimate-only paths matches the pre-existing analytics behavior
  (no cost was being charged there before either).
- **MCP parity/ABAC**: the four llm_query paths (specs/assist REST, MCP `spec_assist`,
  explorer-views/generate, briefing/ask) all record; the three REST routes are in the ABAC
  resolver (`abac_middleware.rs:284,417,428`); `spec_assist` performs no repository writes
  (recording goes through the shared helper), so it correctly stays out of the `needs_write`
  gate — confirmed by check-mcp-write-tools deriving gate membership from handler effects.
- **web/dist** changes are a rebuild artifact of unchanged `web/src` (verified: zero diff under
  `web/src`, `web/package*.json` between base and candidate), consistent with the deterministic
  build claim in the Shipped note.

## Notes (not findings)

- `list_by_workspace` has no production REST consumer yet — expected: the task contract
  specifies it "for later reporting/retention" (task-192 CLI, task-191 enforcement are the
  consumers). It is exercised by tests and the port is production-wired via `save`.
- `POST /api/v1/agents/:id/usage` runs without middleware ABAC — pre-existing at the base
  (frozen exemption `scripts/abac-route-registry-exemptions.txt:19`, unchanged by this
  candidate; check-abac-exempt-handlers passes). Not widened or introduced here.
- `check-task-commit-attribution.sh` fails at the base commit for task-210 (`a781ede2`); the
  candidate adds that commit to task-210's frontmatter, which is the correct repair direction.
- Coverage matrix row 29 stays `task-assigned` — status flips are PM-owned per the goal's
  process, not the implementer's; this review confirms the hollow findings it recorded
  ("record_budget_usage zero callers", "BudgetCallRecord never persisted") are now false at the
  candidate.

Approval basis: independent evidence (probes above) supports every acceptance criterion in the
task contract — port + 3 real adapters with read-back, agent-usage path appends a record and
increments workspace AND tenant counters visible via the budget GET, all four llm_query paths
record, `check_spawn_budget` token limits fire from real recorded usage, and the
discrimination tests fail when the wiring is removed.
