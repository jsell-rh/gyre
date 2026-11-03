# Review — task-192 (Add gyre budget CLI: show and set at repo/workspace/tenant scope)

Spec: `specs/system/platform-model.md` §5 Resource Governance → CLI (`gyre budget show` / `gyre budget show --workspace` / `gyre budget show --tenant` / `gyre budget set --llm-tokens N` / `gyre budget set --workspace --llm-cost N`).

Candidate under review: `a27627f9edca2a600620e585515313f43c975654` (base `321936dfeb0a756b93177e479736e41d78491ecb`). Product commits: `f1dd5659` (feat), `0a2d6e7e` (review-repair 1), `06309226` (review-repair 2, durable finding 02b87ba4) — all three recorded in the task's `commits:` frontmatter; the remaining commits in range are process/docs-only (`check-task-commit-attribution.sh` OK). Diff touches only `crates/gyre-cli/src/{main,client}.rs`, `docs/cli.md`, `specs/tasks/task-192.md`. Working tree verified pristine at HEAD `a27627f9` after all probes.

Verdict: **complete (approved)**. Evidence under `/tmp/stage/review-evidence/` (real-binary probes `budget-cli-probe.py`/`.out`, `budget-bare-scope-probe.py`/`.out`, `focused-tests.out`).

## Independent evidence

Focused tests at the candidate (`/tmp/stage/bin/cargo`):

- `cargo test -p gyre-cli`: **129 + 1 passed, 0 failed** (19 budget tests: parse/conflict, client URL+method+body wiring, merge-preserve, non-finite/negative rejection with zero HTTP requests, zero-cost PUT, zero-cap display).
- `cargo build -p gyre-cli` and `cargo build --all`: ok. `bash scripts/check-arch.sh`: ok. `check-task-commit-attribution.sh`: ok. Also ran the mechanical checks touching the new code (`check-byte-slice-truncation`, `check-lossy-secret-conversion`, `check-inert-enforcement`, `check-relative-path-defaults`, `check-unbounded-external-http`): all OK.
- **Mutation checks (both restored, tree verified clean):** (1) reverting the guard to `cost <= 0.0` makes `budget_set_zero_cost_disables_spending_and_preserves_limits` FAIL; (2) deleting the `validate_llm_cost` call makes both `budget_set_rejects_*_before_any_request` tests FAIL. The tests are load-bearing, not self-confirming.
- **Real-binary probes** (debug binary built from the pristine candidate, driven over real loopback HTTP with a captured request log; transcripts saved):
  - `gyre budget set --workspace --workspace-name platform --llm-cost 0` exits 0, issues exactly slug-GET → budget-GET → budget-PUT, PUT body `{"max_tokens_per_day":500000,"max_cost_per_day":0.0,"max_concurrent_agents":4,"max_agent_lifetime_secs":3600}` (zero cap sent as `0.0`, all unmentioned limits preserved), Bearer token sent, display renders `$12.50 / $0.00 (over cap)` — no `NaN%`.
  - `--llm-cost=-5` and `--llm-cost nan` exit 1 with an error naming `--llm-cost` and **zero HTTP requests captured** (no budget-mutating PUT can occur).
  - Bare `gyre budget show` in a repo whose origin is `.../git/platform/gyre.git` resolves `GET /api/v1/workspaces?slug=platform` → `GET /api/v1/workspaces/ws-123/budget` and prints limits + live usage; bare `gyre budget set --llm-tokens 500000` (spec example) PUTs the merged body preserving the cost cap. Outside a gyre repo, bare show fails with guidance and **no request**.
  - `--repo` membership is validated via the repos list (unknown repo rejected before any budget request); `--tenant` show issues `GET /api/v1/budget/summary` and prints tenant totals + per-workspace table; a 400 cascade body is surfaced verbatim with its status; `set --tenant` bails truthfully with no request; `--workspace --tenant` is rejected by clap.
  - One false start recorded for honesty: the first probe run used the debug binary left over from mutation check 2 (validation removed) and "failed" the negative-cost probe; rebuilding from the pristine candidate resolved it. The failure was my artifact, not a candidate defect.

## Verified against the task contract

- **No invented routes, no fabricated repo budget.** Client methods hit exactly `GET/PUT /api/v1/workspaces/:id/budget` and `GET /api/v1/budget/summary` as registered in `gyre-server/src/api/mod.rs`; the client mirror types (`BudgetConfig`/`BudgetUsage`/`BudgetResponse`/`SetBudgetRequest`/`TenantBudgetSummary`) match the server `api/budget.rs` shapes field-for-field (`Id` serializes as a plain string, so `entity_id: String` deserializes the server output).
- **Workspace resolution is reused, not duplicated.** `resolve_budget_workspace` calls the same `infer_repo_from_git_remote` + `resolve_workspace_slug` + `resolve_repo_name` chain `explore`/`deps` use; probe-verified against a real git remote. Help text on the command and both subcommands, plus `docs/cli.md`, states repo scope maps to the owning workspace.
- **Durable finding 02b87ba4 repaired.** `validate_llm_cost` rejects only non-finite and `cost < 0.0`; finite zero is accepted, PUT as `max_cost_per_day: 0.0` (probe-verified), and rendered as `(zero cap)`/`(over cap)` instead of `NaN%`. Mutation check 1 proves the zero-cost regression test guards this exact boundary.
- **Fetch-merge-put is correct for zero.** `merge_budget_updates` uses `Option::or`, so `Some(0.0)` survives (it is not folded into the previous value or `None`); unmentioned limits keep their GET values (probe-verified body).
- **Error surfacing.** Non-success statuses bail with the HTTP status and the server body verbatim (400 cascade probe passes; the 403 non-Admin path is the same code branch, and the Admin gate itself is server-side in `set_workspace_budget`, unchanged by this task).
- **Conventions.** Output/error style matches the existing `Deps`/`Trace` handlers; `--workspace`/`--tenant` are clap-mutually-exclusive; `set` with no limits bails with guidance before any network I/O.

Minor (not blocking, recorded for completeness):

- `--workspace` without `--workspace-name` behaves identically to the default repo scope (both infer the workspace from the git remote). This is faithful to the task's repo-scope note (repo scope *is* the owning workspace) and is documented in the help text; the flag exists to mirror the spec's CLI surface.
- There is no way to *clear* a limit from the CLI (no `--llm-cost none`); the spec excerpt only requires setting values, and the fetch-merge-put preserves everything unmentioned, so this is not a gap against the cited section.
- Coverage matrix row 30 (`platform-model.md` §CLI) still reads `task-assigned task-192`/Hollow — expected: the matrix is flipped by the spec-fidelity audit cycle post-merge, not by the implementing agent; the shipped code satisfies the row's requirement (`gyre budget` subcommand now exists and calls the real budget endpoints).
