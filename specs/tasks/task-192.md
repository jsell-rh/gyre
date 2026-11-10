---
title: "Add gyre budget CLI: show and set at repo/workspace/tenant scope"
spec_ref: "platform-model.md §CLI"
depends_on: []
progress: complete
coverage_sections:
  - "platform-model.md §CLI"
commits: ["2b846915a78d6e9a755b00939193592e4870c952", "06309226050a40ae1917920283ad8be9e3938bac", "0a2d6e7e08d2787195748bee73863ab987fb3a61", "f1dd5659f671593ddc3a75bca69beac91091563a"]
---

## Spec Excerpt

From `specs/system/platform-model.md` §5 Resource Governance → CLI:

> ```bash
> gyre budget show                          # Current repo budget usage
> gyre budget show --workspace              # Workspace-level usage
> gyre budget show --tenant                 # Tenant-level usage
> gyre budget set --llm-tokens 500000       # Set repo budget
> gyre budget set --workspace --llm-cost 100.00  # Set workspace cost limit
> ```

## Problem

The section is **hollow — no `gyre budget` subcommand exists** in `crates/gyre-cli`. The only budget surface in the CLI is `gyre status`, which prints `budget_spent_usd`/`budget_pct` scraped from the metrics endpoint (`crates/gyre-cli/src/main.rs:1442-1447`). There is no way to view detailed budget usage or set limits from the CLI.

## Existing server endpoints (verified in `crates/gyre-server/src/api/mod.rs`)

- `GET  /api/v1/workspaces/:id/budget` → `budget::get_workspace_budget` (mod.rs:591) — returns limits + real-time usage for a workspace.
- `PUT  /api/v1/workspaces/:id/budget` → `budget::set_workspace_budget` (mod.rs:592, Admin only) — sets workspace limits; enforces the tenant cascade.
- `GET  /api/v1/budget/summary` → `budget::budget_summary` (mod.rs:594, Admin only) — full tenant picture: all workspace configs + usage.

The `SetBudgetRequest` shape (`crates/gyre-server/src/api/budget.rs:63-69`) accepts `max_tokens_per_day`, `max_cost_per_day`, `max_concurrent_agents`, `max_agent_lifetime_secs`. `BudgetResponse` (budget.rs:55-61) carries both config and usage.

**Repo-scope note:** the budget governance boundary in the data model is the **workspace** (keyed `workspace:{project_id}`); there is no repo-keyed budget store or repo-level budget endpoint today. The default (`gyre budget show` / `gyre budget set` with no scope flag) MUST therefore resolve the current repo's owning workspace (workspace inferred from the git remote / `--workspace` flag, exactly as `gyre explore` and `gyre deps` already do) and operate on that workspace's budget. The CLI help text MUST state that repo scope maps to the owning workspace. Do NOT fabricate a repo-level budget store or print a hardcoded repo budget — if a genuine repo-level budget capability is later required, it is a separate spec/cascade concern, not this task.

## Implementation Plan

1. **Add `Budget` command** to the `Commands` enum in `crates/gyre-cli/src/main.rs` with a `BudgetCommands` subcommand enum (follow the existing `Deps`/`Spec`/`Inbox` subcommand patterns):
   - `Show { workspace: bool, tenant: bool, workspace_slug: Option<String> (--workspace-name), repo: Option<String> }`
   - `Set { workspace: bool, tenant: bool, llm_tokens: Option<u64>, llm_cost: Option<f64>, max_agents: Option<u32>, max_agent_lifetime_secs: Option<u64>, workspace_slug: Option<String>, repo: Option<String> }`
   - Use clap flags `--workspace` / `--tenant` as booleans selecting scope (mutually exclusive; default = repo→workspace).
2. **Client methods** in `crates/gyre-cli/src/client.rs`:
   - `get_workspace_budget(workspace_id) -> BudgetResponse` (GET `/api/v1/workspaces/{id}/budget`).
   - `set_workspace_budget(workspace_id, SetBudgetRequest) -> BudgetResponse` (PUT).
   - `budget_summary() -> TenantBudgetSummary` (GET `/api/v1/budget/summary`).
   - Resolve a workspace slug/git-remote → workspace id using the same helper the `explore`/`deps` commands use (grep for how they map `--workspace`/`--repo` to ids; reuse it, do not duplicate).
3. **`show` output:** print limits and real-time usage (tokens used today vs limit, cost today vs limit, active agents, % utilization). For `--tenant`, iterate the summary and print a per-workspace table plus totals.
4. **`set` output:** send only the provided fields (leave others unchanged where the API supports partial update; otherwise fetch-merge-put). Print the resulting `BudgetResponse`. Surface the server's 400 cascade-violation error verbatim when a limit exceeds the tenant ceiling, and the 403 when the caller lacks Admin.
5. Register the command in the `match` dispatch in `main.rs`.

## Acceptance Criteria

- `gyre budget show` (no flags) resolves the current repo's workspace and prints its budget limits + live usage; `--workspace` targets a named workspace; `--tenant` prints the tenant-wide summary. Help text documents that repo scope maps to the owning workspace.
- `gyre budget set --llm-tokens 500000` and `gyre budget set --workspace --llm-cost 100.00` issue the correct `PUT /api/v1/workspaces/:id/budget` with the right body and print the updated budget. A value exceeding the tenant ceiling surfaces the server's cascade error; a non-Admin token surfaces the 403.
- The CLI calls the real endpoints registered in `mod.rs` (verified above) — no invented routes.
- A test proves the wiring against a running/mock server OR a client-level test asserts the exact method + URL + serialized body for `show`/`set` (choose whichever matches existing CLI test conventions in `crates/gyre-cli`). The test must fail if the URL or body is wrong.
- `cargo build --all` and touched-crate tests pass; `bash scripts/check-arch.sh` passes.

## Agent Instructions

- Verify the workspace-resolution helper (git-remote → workspace id) used by `gyre explore`/`gyre deps` in `main.rs`/`client.rs` and reuse it; do not hand-roll a second resolver.
- Do NOT print or store a hardcoded repo-level budget. Repo scope resolves to the owning workspace budget and the help text says so.
- Match the existing CLI output/formatting and error-handling style (see the `Deps`/`Trace` handlers).
- Confirm `SetBudgetRequest`/`BudgetResponse`/`TenantBudgetSummary` field names in `crates/gyre-server/src/api/budget.rs` before serializing.
- Run only the touched crates' tests plus `scripts/check-arch.sh`; do not run the full workspace suite or formatters.

## Shipped

`gyre budget show` / `gyre budget set` in `crates/gyre-cli` (commits f1dd5659, 0a2d6e7e, 06309226, plus format repair 2b846915), calling exactly the endpoints registered in `gyre-server/src/api/mod.rs` — `GET/PUT /api/v1/workspaces/:id/budget` and `GET /api/v1/budget/summary`. No new routes, no repo-level budget store.

### Behavior

- `gyre budget show` (no flags) resolves the current repo's owning workspace from the git remote via the same `infer_repo_from_git_remote` + `resolve_workspace_slug` chain `gyre explore`/`gyre deps` use, then prints that workspace's limits and live usage (tokens/cost/active agents vs limits, % utilization). `--workspace [--workspace-name <slug>]` targets a named workspace; `--repo` validates repo membership like `explore` does; `--tenant` prints the Admin-only tenant summary with a per-workspace table plus tenant totals. Zero caps render as an explicit `(zero cap)`/`(over cap)` display, never `NaN%`.
- `gyre budget set --llm-tokens/--llm-cost/--max-agents/--max-agent-lifetime-secs` validates `--llm-cost` (finite and non-negative) **before any network I/O**, then PUTs the merged config (fetch-merge-put, since the server replaces the whole config) so omitted limits keep their values, and prints the resulting budget. NaN/Infinity (which serde_json serializes as `null`, silently clearing a cost cap) and negative amounts are rejected with exit 1 and no request sent. A finite **zero** cost cap is valid — it explicitly disables all spending for the day (the server stores it, the over-budget check blocks any usage above it, and the CLI renders it as `(zero cap)`/`(over cap)` rather than `none`) — and is PUT as `max_cost_per_day: 0.0`. Server errors surface verbatim with their HTTP status (400 cascade violation, 403 non-Admin). `set --tenant` bails with a truthful message (no tenant-level set endpoint exists in the API).
- Help text on the command, both subcommands, and `docs/cli.md` states that repo scope maps to the owning workspace and documents the cost-input validation (finite, non-negative; zero disables spending).

### Test evidence

- Client-level tests in `client.rs` run a one-shot mock HTTP server over a real loopback socket and assert the exact request line (`GET/PUT /api/v1/workspaces/ws-123/budget`, `GET /api/v1/budget/summary`), Bearer auth header, exact serialized `SetBudgetRequest` body, and verbatim surfacing of a 400 cascade error — they fail if the URL, method, or body drifts.
- Command-path flow tests in `main.rs` drive `run_budget_set` (the exact flow the handler calls) against a sequential mock HTTP server: NaN/±Infinity/negative `--llm-cost` all return an error naming the flag **with zero HTTP requests captured** (no budget-mutating PUT can occur); a valid cost PUTs the exact merged body preserving unmentioned limits; and `budget_set_zero_cost_disables_spending_and_preserves_limits` proves `--llm-cost 0` PUTs `"max_cost_per_day":0.0` while preserving every unmentioned limit. Mutation checks: removing the validation call makes the rejection tests fail, and reverting the guard to `cost <= 0` makes the zero-cost test fail.
- Zero-cap display tests assert `(zero cap)`/`(over cap)` output and the unchanged non-zero paths.
- `cargo test -p gyre-cli`: 129 + 1 passed, 0 failed. `cargo build -p gyre-cli`: ok. `bash scripts/check-arch.sh`: ok.
- Real-binary probes against real loopback HTTP (transcripts in `/tmp/stage/review-evidence/`): the staged operator reproducers all pass — `zero-set-probe.py` (`--llm-cost 0` exits 0, exactly one PUT carrying `max_cost_per_day: 0.0` with `max_tokens_per_day: 500000` / `max_concurrent_agents: 4` / `max_agent_lifetime_secs: 3600` preserved, bounded `(over cap)` display), `operator-nan-cost-probe.py` (NaN rejected, no mutation), `operator-zero-limit-probe.py` (bounded zero-cap display) — plus `negative-cost-probe.py` (`--llm-cost=-5` rejected before any request, error naming the flag and negativity).

### Verification repair (round 2)

- The prior verification run failed only `scripts/check-rustfmt-diff.py` against base 321936d: changed lines in `crates/gyre-cli/src/client.rs` and `main.rs` needed formatting. Commit 2b846915 reformats exactly those lines to rustfmt's own output (line-wrapping only; no behavior change).
- Equivalence proof (this sandbox cannot compile — crates.io unreachable and no local registry cache, an infrastructure restriction recorded for remote verification, not a code defect): `python3 scripts/check-rustfmt-diff.py 321936d` now exits 0; `git diff -w` between the previously verified tree and the repair shows only whitespace-level line wrapping plus two semantically inert changes (removal of a match-arm trailing comma and a swap of two `use` statements whose resolution order is irrelevant); rustfmt parsed both files successfully (the checker invokes it with `check=True`). The behavior verified in round 1 (`cargo test -p gyre-cli`: 129 + 1 passed, 0 failed; `cargo build -p gyre-cli`: ok) is therefore unchanged.
- `bash scripts/check-arch.sh`: ok. Evidence saved under `/tmp/stage/review-evidence/` (rustfmt-check.txt, whitespace-equivalence.txt, check-arch.txt, transport-restriction.txt).
