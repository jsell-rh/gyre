# Review — task-192 (Add gyre budget CLI: show and set at repo/workspace/tenant scope)

Spec: `specs/system/platform-model.md` §5 Resource Governance → CLI.
Candidate: `4bf33f9dbb552b11d320fa8c12db2464fd91ede4` on `pipeline/task-192/a2eb285221e6481db0ca4b6378b4d6ce-1` (base `915cc789`). Implementation commit `f1dd5659` (attributed in the task's `commits:` frontmatter).
Verdict: **needs-revision** (two confirmed defects; all other acceptance criteria independently verified).
Full evidence: `/tmp/stage/review-evidence/task-192-review.md`, `e2e-transcript-part1.txt`, `e2e-transcript-part2.txt`, `operator-nan-cost-probe.py`, `operator-zero-limit-probe.py`, `cost-variant-probe.txt`.

## Round 1

### Diff scope

`crates/gyre-cli/src/main.rs` (+582: `Budget` command, `Show`/`Set` subcommands, dispatch, `resolve_budget_workspace`, `merge_budget_updates`, print helpers, 10 tests), `crates/gyre-cli/src/client.rs` (+344: mirror response/request types, `get_workspace_budget`/`set_workspace_budget`/`budget_summary`, 4 mock-server wiring tests), `docs/cli.md` (+33), `specs/tasks/task-192.md`. No server changes, no new routes, no fabricated repo-level budget store.

### Verified working (independently)

- **Endpoints**: CLI calls exactly the routes registered in `gyre-server/src/api/mod.rs:610-613` (`GET/PUT /api/v1/workspaces/:id/budget`, `GET /api/v1/budget/summary`) — no invented routes. Mirror types match `api/budget.rs` + `gyre-domain/src/budget.rs` field-for-field (`gyre_common::Id` is a `String` newtype with derived serde, so the CLI's `entity_id: String` round-trips correctly).
- **Workspace resolution** reuses the exact `infer_repo_from_git_remote` + `resolve_workspace_slug` + `resolve_repo_name` chain `gyre explore` uses — no hand-rolled second resolver.
- **Help text** on `gyre budget`, `budget show`, `budget set`, and `docs/cli.md` states that repo scope maps to the owning workspace, as the repo-scope note requires.
- **Focused tests**: `cargo test -p gyre-cli budget` → 13 passed; full `cargo test -p gyre-cli` → 123 + 1 passed, 0 failed; `SKIP_WEB_BUILD=1 cargo build --all` → ok; `scripts/check-arch.sh` → ok; `check-task-commit-attribution`, `check-byte-slice-truncation`, `check-relative-path-defaults`, `check-inert-enforcement`, `check-fabricated-scope-defaults` → all ok.
- **Mutation testing** (reverted after each; tree verified clean, HEAD unchanged): a URL typo in `get_workspace_budget` and a `#[serde(rename)]` on a `SetBudgetRequest` field each cause the wiring tests to FAIL on the exact request line / serialized body — the tests genuinely kill URL/body drift, they are not self-confirming.
- **Live end-to-end** against a real debug-build `gyre-server` (loopback, SQLite, fresh DB; workspace + repo created via the real API):
  - `budget show --workspace --workspace-name platform` prints limits + live usage; spec invocations `set --llm-tokens 500000` and `set --workspace --llm-cost 100.00` PUT and print the updated budget;
  - fetch-merge-put verified live: after the second `set`, the tokens limit from the first `set` is preserved (the server PUT replaces the whole config; the CLI merge preserves unmentioned limits);
  - tenant cascade: with a `tenant:global` ceiling of 600000 seeded in the server's own DB, `set --llm-tokens 700000` surfaces `HTTP 400 Bad Request: {"error":"workspace max_tokens_per_day (700000) exceeds tenant limit (600000)"}` verbatim; a set at exactly the ceiling succeeds;
  - non-Admin per-agent token: `set` → HTTP 403 with the server's Admin-only message; `show --tenant` → HTTP 403; plain `show` allowed;
  - repo-scope inference: from a git repo whose origin is `<server>/git/platform/gyre.git`, bare `gyre budget show` and bare `gyre budget set --llm-tokens 550000` both resolve and target the owning workspace;
  - `--repo nonexistent` rejected; `set --tenant` and flag-less `set` with no limits bail truthfully.

### Findings

- [-] **F1 (major): `gyre budget set --llm-cost NaN` (or `inf`) silently clears the cost limit instead of setting one.** The `Set` handler performs no finiteness validation on `llm_cost`; clap parses `NaN`/`inf` as f64, and serde_json serializes non-finite f64 as `null`, so the PUT body carries `max_cost_per_day: null` — with the fetch-merge-put this *removes* a previously-set cost limit (e.g. $50) while the command exits 0 and prints a success budget. User input meant to set a financial governance limit instead disables it with no error — a fail-open on a governance control. Independently reproduced on the clean candidate binary: `python3 /tmp/stage/review-evidence/operator-nan-cost-probe.py` (exit 0, PUT body `max_cost_per_day: null`, assertion fails). Variants: `inf` behaves identically; `-5`/`-inf` are only incidentally rejected by clap's leading-dash parsing, not by validation (`cost-variant-probe.txt`). The server performs no value validation either (only the cascade), so the CLI is the only gate. Remedy: validate `llm_cost` (finite, > 0) — and correspondingly the other numeric limits — in the `Set` handler before fetch-merge-put, bailing with a clear error; add a parse/validate test that kills non-finite input.
- [-] **F2 (moderate): zero limits render as `NaN%` in `gyre budget show`.** `budget_usage_line`/`budget_cost_line` (main.rs) divide by the limit when it is `Some(0)`: a workspace with zero token/cost/agent limits prints `0 / 0 (NaN%)` for all three utilization lines. Independently reproduced: `python3 /tmp/stage/review-evidence/operator-zero-limit-probe.py` (stdout contains `0 / 0 (NaN%)`). A zero cap needs a bounded or explicitly-unavailable utilization display (e.g. `n/a` or `0%`), not NaN/Infinity. Remedy: handle `Some(0)` in the two helpers (and the agent-count variant), with a unit test.

### Notes (non-blocking)

- The implementer's shipped notes claim e2e transcripts at `/tmp/stage/review-evidence/`, which did not exist at review time (per-stage `/tmp` does not persist). This review regenerated equivalent — and broader — live-server evidence independently, so the claim is corroborated rather than relied upon.

All acceptance criteria except the input-validation/display defects above are met with independently verified production behavior. Needs revision for F1 and F2.
