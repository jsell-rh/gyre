# Review — task-192 (Add gyre budget CLI: show and set at repo/workspace/tenant scope)

Spec: `specs/system/platform-model.md` §5 Resource Governance → CLI (`gyre budget show` / `set` at repo/workspace/tenant scope).

Commits under review: `f1dd5659`, `0a2d6e7e`, `06309226`, `2b846915` (all recorded in the task's `commits:` frontmatter; process/docs commits touch only `specs/tasks/task-192.md`). Cumulative diff base `321936d` → candidate `1f8f9012`: `crates/gyre-cli/src/main.rs` (+873), `crates/gyre-cli/src/client.rs` (+398), `docs/cli.md` (+37), task file. Working tree verified byte-clean at `1f8f9012` after all probes.

Verdict: **approved** — no findings.

## Independent evidence (all under `/tmp/stage/review-evidence/`)

Transport note: `static.crates.io` is unreachable from this sandbox (curl exit 7), but a local offline cargo cache existed, so all probes ran with `cargo --offline`; no check was blocked.

- **`cargo test -p gyre-cli --offline`**: 129 + 1 passed, 0 failed — matches the shipped claim exactly. `cargo build -p gyre-cli --offline`: ok.
- **Mutation probes** (each applied, suite re-run, then restored; tree verified clean afterwards):
  - Removing the `validate_llm_cost` call in `run_budget_set` → both `budget_set_rejects_*_before_any_request` tests FAIL (and the PUT actually mutates the mock budget — the tests catch the real regression).
  - Reverting `cost < 0.0` to `cost <= 0.0` → `budget_set_zero_cost_disables_spending_and_preserves_limits` FAILS.
  - Changing the client PUT URL `.../budget` → `.../budget-limit` → 3 tests FAIL (client URL/body test + both command-flow PUT-body tests).
- **Real-binary E2E probe** (`budget-e2e-probe.py`, 10 scenarios, all pass; drives the actual clap dispatch + config load, not just internals):
  1. `show --workspace-name platform` → `GET /api/v1/workspaces?slug=platform` then `GET /api/v1/workspaces/ws-123/budget`, output prints `123456 / 500000 (24.7%)` and `$12.50 / $100.00`.
  2. `set --llm-cost 50` → exactly 3 requests; PUT body is the fetch-merged config `{"max_tokens_per_day":500000,"max_cost_per_day":50.0,"max_concurrent_agents":4,"max_agent_lifetime_secs":3600}` (unmentioned limits preserved — necessary because the server PUT replaces the whole config, verified in `api/budget.rs:120-124`).
  3. `set --llm-cost 0` → PUT carries `"max_cost_per_day":0.0` with all other limits preserved (zero cap = disable spending, not clear).
  4. `set --llm-cost=-5` → exit 1, error names `--llm-cost`, **zero** requests reach the server (validation before network I/O).
  5. `show --tenant` → `GET /api/v1/budget/summary`, per-workspace table + tenant totals printed.
  6. Non-Admin 403 → surfaced verbatim with `HTTP 403` and the server message.
  7. Tenant-cascade 400 → surfaced verbatim with `HTTP 400` and `exceeds tenant limit`.
  8. Bare `show` with no git remote and no flags → truthful error naming the git-remote inference and the `--workspace-name` remedy.
  9. `set --tenant` → truthful bail (no tenant-level set endpoint exists), zero requests.
  10. `budget --help` documents that repo scope maps to the owning workspace.
- **Mechanical gates**: `python3 scripts/check-rustfmt-diff.py 321936d` → exit 0 (the round-2 rustfmt repair verified independently, not just claimed); `bash scripts/check-arch.sh` → exit 0; `bash scripts/check-task-commit-attribution.sh` → exit 0.

## Verified by inspection (no findings)

- **Real endpoints only**: the three client methods hit exactly `GET/PUT /api/v1/workspaces/:id/budget` and `GET /api/v1/budget/summary` as registered in `gyre-server/src/api/mod.rs` (591/592/594). No invented routes, no repo-level budget store, no hardcoded budget values.
- **Type fidelity**: CLI `BudgetConfig`/`BudgetUsage`/`BudgetResponse`/`SetBudgetRequest`/`TenantBudgetSummary` mirror `gyre-server/src/api/budget.rs:42-69` field-for-field; `gyre_common::Id` is a newtype over `String` with derived serde, so the CLI's `entity_id: String` parses the server's `Id` field correctly.
- **Resolver reuse, not duplication**: `resolve_budget_workspace` calls the same `resolve_workspace_slug` / `resolve_repo_name` / `infer_repo_from_git_remote` chain `gyre explore`/`gyre deps` use (verified against the explore/deps handlers); `--repo` is validated for membership exactly as explore does.
- **Error surfacing**: server `ApiError::InvalidInput` → 400 and `Forbidden` → 403 (error.rs:56-59); the CLI bails with `HTTP {status}: {body}` so both cascade and authorization failures reach the operator verbatim.
- **Scope semantics**: `--workspace`/`--tenant` are mutually exclusive (clap `conflicts_with`, tested); default repo scope resolves the owning workspace per the task's repo-scope note, and the help text on the command, both subcommands, and `docs/cli.md` state this explicitly.

Non-blocking observations (recorded for completeness, not defects):

- `--workspace` without `--workspace-name` is behaviorally identical to the default repo scope (both infer the workspace from the git remote). This follows the task's own design ("default = repo→workspace", repo maps to the owning workspace), so it is the specified behavior, not a gap.
- A `-0.0` `--llm-cost` passes validation (`-0.0 < 0.0` is false), serializes as `-0.0`, and displays via the zero-cap arm (f64 pattern equality) — a harmless edge equivalent to a zero cap.
- The test-only `read_request` helper in `client.rs` would panic on a truncated request; it only ever sees well-formed reqwest traffic in tests.
