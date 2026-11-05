# Review: TASK-190 — Record Real Per-Call LLM Usage Into Budget Counters and Audit Log

**Reviewer:** Independent review (pi runtime)
**Candidate:** `a08b212f145b92fa133590d4b2605bb69387bd0b` (base `110180fb`)
**Verdict:** `approved`

---

## Scope Reviewed

Full diff base→candidate (16 files, +972/−25): new `BudgetCallRepository`
port, SQLite/Postgres/in-memory adapters, `AppState.budget_calls` wiring,
single recording entry point `record_llm_call_usage` (append record +
delegate counter increment to the pre-existing `record_budget_usage`), and
the five LLM call sites (agent usage report, specs/assist REST, MCP
spec_assist, explorer-views/generate, briefing/ask).

## Findings

None blocking. Notable non-findings checked:

- **No structural/no-op adapters.** SQLite and Postgres issue real diesel
  INSERT/SELECT against the pre-existing `budget_call_records` table
  (mapping in `schema.rs` verified against migration 000024); duplicate PK
  ids are rejected by the table constraint, and the mem adapter enforces the
  same contract in code (satisfying `check-mem-port-contracts`).
- **No hardcoded token counts.** `agent_run` records use the reported
  tokens/cost from the usage report; `llm_query` paths split the existing
  length-based estimate into input (prompt estimate) / output (remainder);
  briefing/ask estimates output from the actual produced text. The
  analytics `CostEntry` value is unchanged (rename only).
- **No forged scope.** `tenant_id`/`workspace_id` are always derived from
  the workspace/repo/agent row the handler loaded (agent path resolves the
  agent's workspace row; explorer-views checks workspace membership first);
  on lookup failure the recording is skipped with a warning — no fabricated
  "default" scope (matches `check-fabricated-scope-defaults`).
- **Single counter-increment entry point.** Exhaustive grep:
  `record_budget_usage`'s only production caller is `record_llm_call_usage`;
  no call site reimplements the increment.
- **MCP write-tool gate.** `spec_assist`'s handler writes were pre-existing
  (CostEntry) and `check-mcp-write-tools.sh` passes at the candidate.
- **Minor label inaccuracy in task notes (not a defect):** the Shipped
  section says briefing/ask "keeps the pre-existing analytics CostEntry" —
  graph.rs had no CostEntry at base; the candidate adds one alongside the
  budget record. Improvement, not a regression.

## Independent Evidence

All commands and exit codes in
`/tmp/stage/review-evidence/task-190-review-notes.md`:

- `cargo test -p gyre-adapters budget_call` → exit 0, 3/3 (real SQLite
  round-trip: every field, workspace/since/limit filtering newest-first,
  duplicate-id rejection).
- `cargo test -p gyre-server --lib api::budget` → exit 0, 9/9 including
  `record_llm_call_usage_increments_workspace_and_tenant_counters` and
  `recorded_usage_makes_token_budget_limit_fire` (500/1000 → Ok,
  1100/1000 → Err naming `max_tokens_per_day`).
- `cargo test -p gyre-server --lib agent_usage_report_records_budget_call`
  → exit 0: full HTTP path — POST usage (600+400 tokens, $0.12) appends
  exactly one `agent_run` record via `list_by_workspace`, workspace AND
  tenant counters at 1000/$0.12, `GET /workspaces/:id/budget` reflects it.
- Touched modules all green: `api::spawn` 35/35, `api::specs_assist` 21/21,
  `api::explorer_views` 16/16, `api::graph` 23/23, `mcp` 79/79 (mock-LLM
  tests execute the new recording blocks).
- `scripts/check-arch.sh` passes; also mcp-write-tools,
  mem-port-contracts, in-memory-state-stores, unwired-port-methods,
  scope-literal-defaults, fabricated-scope-defaults, inert-enforcement,
  task-commit-attribution all pass.
- The cold gyre-server lib+test build completed successfully (observed
  ~4h under host load 16-32 on 4 CPUs — infrastructure slowness, not a code
  defect; the tirith command gate blocked bash intermittently during it).

## Test Discrimination

An empirical edit-rebuild kill probe was deferred (a gyre-server test-crate
rebuild costs hours in this sandbox; see evidence file). Discrimination is
established by construction: the tests use fresh `mem::test_state()` stores,
and the only code that writes tokens/cost to `MemBudgetUsageRepository` or
records to `MemBudgetCallRepository` during those tests is the wiring under
test (spawn helpers touch `active_agents` only). With the wiring removed,
`agent_usage_report_...` fails at `.expect("workspace budget usage must
exist after usage report")` and the token-limit test fails at `expect_err`.
Verifier may confirm with a standard kill probe when running full suites.

## Deferred to Verifier

- `cargo build --all` / full workspace suite (workspace-wide invocations are
  rejected by the review-stage cargo wrapper by design; all touched crates
  compiled and passed focused tests; gyre-cli does not construct the
  server's AppState).
- Postgres adapter runtime test (needs a live PG server; PG adapter mirrors
  the tested SQLite adapter).
