---
title: "Add gyre budget CLI: show and set at repo/workspace/tenant scope"
spec_ref: "platform-model.md §CLI"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "platform-model.md §CLI"
commits: ["4d16c6c5b2ded0f53c67e528f4ecf5299fd714ca", "6f4d1366b84193c972c2e7c13a1a42059da2806a", "a5a82183d3e67df0aac25f30e2ba16c5709081f2", "87fae32e02d6d4bdeca2070de71e5bec2eb7d458"]
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

`gyre budget show|set` is implemented end-to-end in `crates/gyre-cli` against the three real server routes (`GET/PUT /api/v1/workspaces/:id/budget`, `GET /api/v1/budget/summary` — mod.rs:610-613). Recovered checkpoint commits (a5a8218, 6f4d136, 4d16c6c) supplied the base implementation; this assignment audited every acceptance criterion, fixed one real display defect, and re-verified all gates.

**Behavior:**
- `show` (no flags) resolves the current repo's owning workspace via the same `infer_repo_from_git_remote` + `resolve_workspace_slug` pair `deps`/`explore` use, then prints limits + live usage (tokens/cost/agents, used vs limit, % util). `--workspace-name <SLUG>` targets a named workspace; `--workspace` selects the same repo→workspace resolution explicitly. `--tenant` prints the tenant summary: tenant rows + per-workspace table + totals. `--tenant` combined with `--workspace/--workspace-name` is rejected.
- `set` accepts `--llm-tokens/--llm-cost/--max-agents/--max-agent-lifetime-secs` (+ scope flags); the server PUT is a full config replace, so the client fetches the current config and sends it merged with the overrides — unset limits keep their values. `--tenant` on `set` bails with an explicit message (no tenant set endpoint exists). Errors surface the server body verbatim: `{"error": "only Admin role may update workspace budget limits"}` (403), cascade `workspace max_tokens_per_day (…) exceeds tenant limit (…)` (400).
- Help text (command + subcommands, verified via the built binary) states repo scope maps to the owning workspace budget; no repo-keyed budget store is fabricated. `docs/cli.md` documents the commands.

**Defect fixed this round:** `budget_util` rendered `100%+` for a `Some(0.0)` limit at zero usage — a workspace provisioned with a zero limit on an unused day printed full utilization. Now `0%` at zero usage, `100%+` for any usage. Boundary test `budget_util_zero_limit_and_boundary` added and mutation-checked (pre-fix logic fails it: `left: "100%+", right: "0%"`).

**Test evidence** (saved under `/tmp/stage/review-evidence/task192-*.txt`):
- `cargo test -p gyre-cli --bin gyre` → 107 passed, 0 failed (13 budget tests: 5 client route/body tests asserting exact method+URL+auth+serialized merged PUT body, server-shape parsing tests, verbatim 403/400 error-surfacing test, 6 CLI parse tests, 1 boundary test).
- `bash scripts/check-arch.sh` → passed.
- `cargo run -p gyre-cli -- budget --help` / `show --help` / `set --help` → exit 0, help text confirmed (evidence file `task192-budget-help.txt`).
- `tests/ws_integration.rs::test_auth_and_ping_roundtrip` fails in this sandbox with `Os { code: 95, kind: Unsupported }` at the TCP listener bind — matches the recorded `capabilities.json` restriction (`tcp_listener_probe.supported=false`, errno 95). Infrastructure limitation, not a code defect; requires host/CI verification.
- Live end-to-end HTTP against a running server could not be exercised here (sandbox cannot bind listeners); the client-level tests assert the exact request wire format (method, URL, auth header, JSON body) which the routes in `api/mod.rs:610-613` accept, and error paths are exercised with real `reqwest::Response` objects carrying the server's exact wire shape.
