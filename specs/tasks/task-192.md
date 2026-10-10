---
title: "Add gyre budget CLI: show and set at repo/workspace/tenant scope"
spec_ref: "platform-model.md §CLI"
depends_on: [task-211]
progress: ready-for-review
coverage_sections:
  - "platform-model.md §CLI"
commits: ["f4bdb092211438e96dd40533f72a69fe3bc075b8"]
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

**Checkpoint recovery round (merged head `d51ccc5`):** the assignment was
recovered after an interruption and re-verified at the merged HEAD (base
`a1751da1` merged into the branch; product surface byte-identical to the
reviewed candidate — `git diff 02f861b9..HEAD -- crates/ docs/ scripts/` is
empty). Fresh probes, evidence under `/tmp/stage/review-evidence/`:
- `cargo test -p gyre-cli --bin gyre` → 107 passed, 0 failed (budget
  filter: 13 passed) — `task192-cli-bin-tests.txt`,
  `task192-budget-tests-only.txt`.
- Mutation check at this HEAD: renaming the client URL `budget`→`budgets`
  fails 2 wiring tests (`get_workspace_budget_builds_real_route`,
  `set_workspace_budget_put_body_is_merged_config`); source restored
  (md5-verified) and re-run green — `task192-mutation-url-check.txt`,
  `task192-post-mutation-restore.txt`.
- Live binary probes (refused port = the assertion; loopback listeners are
  forbidden in this sandbox, errno 95): bare `budget show` from a repo with
  remote `…/git/platform-team/widgets.git` reaches
  `GET /api/v1/workspaces?slug=platform-team` (real git-remote→slug→id
  resolution); `--tenant` reaches `GET /api/v1/budget/summary`;
  `set --tenant`, empty `set`, and `--tenant --workspace` all exit 1 with
  honest errors — `task192-live-url-probes.txt`.
- `bash scripts/check-arch.sh`, `bash scripts/check-relative-path-defaults.sh`
  (exemption line re-pointed to main.rs:1862 by the moved code),
  `bash scripts/check-task-commit-attribution.sh` (post-merge frontmatter
  intact) → all pass — `task192-check-arch.txt`,
  `task192-relative-path-check.txt`, `task192-commit-attribution.txt`.
- Help surfaces verified via the built binary (`task192-budget-cmd-help.txt`,
  `task192-budget-subcommand-help.txt`): all three document that repo scope
  maps to the owning workspace budget.
- Live end-to-end HTTP against a running server could not be exercised here (sandbox cannot bind listeners); the client-level tests assert the exact request wire format (method, URL, auth header, JSON body) which the routes in `api/mod.rs:610-613` accept, and error paths are exercised with real `reqwest::Response` objects carrying the server's exact wire shape.

**Merge round (merged head `175a5c80`, base `6bf777a6`):** the branch was
merged with the new assignment base (task-200 message-bus work) and re-audited.
Product surface is byte-identical to the reviewed candidate —
`git diff d98af1ba HEAD -- crates/gyre-cli/ docs/cli.md` is empty. Fresh probes,
evidence under `/tmp/stage/review-evidence/` (suffix `-merged-head`):
- `cargo test -p gyre-cli --bin gyre` → 107 passed, 0 failed; budget filter →
  13 passed, 0 failed — `task192-cli-bin-tests-merged-head.txt`,
  `task192-budget-tests-merged-head.txt`.
- `bash scripts/check-arch.sh` → passed;
  `bash scripts/check-relative-path-defaults.sh` → OK (exemption pointer
  `main.rs:1862` matches the actual code line at this HEAD);
  `python3 scripts/check-rustfmt-diff.py 6bf777a6` → changed lines clean
  (2 Rust files) — the rustfmt failures in the prior attempt's log were from
  the superseded checkout, not this history.
- One real gate failure found and repaired: `check-task-commit-attribution.sh`
  exited 1 naming `6bf777a6 task-200` — the assignment base itself (a
  task-200 product-surface commit: per-kind payload validation in
  gyre-common/message.rs + api/messages.rs + mcp.rs) absent from task-200's
  `commits:` frontmatter. Root cause is the landing commit's self-recording
  limitation, upstream-drift class, same shape task-211 repaired for task-210.
  Repair (the check's documented remedy, mirrors upstream task-220's fix
  commit `4635b533` byte-for-byte): appended
  `6bf777a6a44f28052ed5af28bf6fb013fde6df48` to task-200's `commits:` list.
  Mutation-checked — removing the SHA re-fails the gate with the identical
  violation, restore re-passes (exit 0). Exemption file untouched (frozen at
  baseline). Evidence: `task192-commit-attribution-merged-head.txt`,
  `task192-attribution-mutation-check.txt`,
  `task192-attribution-after-restore.txt`.
- Help surfaces re-verified via the built binary at this HEAD
  (`task192-budget-help-merged-head.txt`): all three document repo scope maps
  to the owning workspace budget.

**Checkpoint recovery round (merged head `22e77241`, base `770785f7`):** the
branch was recovered from interrupted-attempt source `f4bdb092` (the same
content as the previously reviewed candidate — it is a squashed preservation
of the branch at the interruption point) and merged with the new assignment
base. Product surface is byte-identical to the recovered candidate —
`git diff f4bdb092 HEAD -- crates/gyre-cli/ docs/cli.md` is empty (the merge
brought only task-068/task-224 work: domain view-query resolver, mcp graph
tools, explorer ws — zero budget/CLI interaction, confirmed by
`git diff 653a696f 770785f7 --stat -- crates/gyre-cli/
crates/gyre-server/src/api/budget.rs crates/gyre-server/src/api/mod.rs`
being empty). `f4bdb092` added to `commits:` for review scoping (recovery
checkpoint carrying the product surface, same precedent as `ed0669fb`/
`6abdfdec`). Fresh probes, evidence under `/tmp/stage/review-evidence/`
(suffix `-recovered`):
- `cargo test -p gyre-cli --bin gyre` → 107 passed, 0 failed; budget filter →
  13 passed, 0 failed.
- Mutation check re-run at this HEAD: renaming the client URL
  `budget`→`budgets` fails the same 2 wiring tests
  (`get_workspace_budget_builds_real_route`,
  `set_workspace_budget_put_body_is_merged_config`); source restored
  (md5-verified) and the full 107-test suite re-run green —
  `task192-mutation-url-check-recovered.txt`.
- Live binary probes from a repo whose origin remote is
  `…/git/platform-team/widgets.git` (refused connect = the assertion; this
  sandbox forbids listeners, errno 95): bare `budget show` reaches
  `GET /api/v1/workspaces?slug=platform-team`; `--tenant` reaches
  `GET /api/v1/budget/summary`; `set --llm-tokens 500000` resolves the
  workspace first; `set --tenant`, empty `set`, and `--tenant --workspace`
  all exit 1 with honest errors — `task192-live-url-probes-recovered.txt`,
  `task192-live-probe-show-bare.txt`, `task192-live-probe-remaining.txt`.
- `bash scripts/check-arch.sh`,
  `bash scripts/check-relative-path-defaults.sh` (exemption pointer
  `main.rs:1862` matches the actual code line at this HEAD),
  `bash scripts/check-task-commit-attribution.sh`,
  `python3 scripts/check-rustfmt-diff.py 770785f7` (changed lines clean) →
  all pass — `task192-check-arch-recovered.txt`,
  `task192-relative-path-check-recovered.txt`,
  `task192-commit-attribution-recovered.txt`.
- Help surfaces verified via the built binary at this HEAD
  (`task192-budget-cmd-help-recovered.txt`,
  `task192-budget-show-help-recovered.txt`,
  `task192-budget-set-help-recovered.txt`): all three document that repo
  scope maps to the owning workspace budget.
- Live end-to-end HTTP against a running server could not be exercised here
  (sandbox cannot bind listeners); the client-level tests assert the exact
  request wire format (method, URL, auth header, JSON body) which the routes
  in `api/mod.rs:609-613` accept, and error paths are exercised with real
  `reqwest::Response` objects carrying the server's exact wire shape. Host
  verification / GitHub CI must run the live-server round-trip.
