# Review — task-192 (Budget CLI: show and set at repo/workspace/tenant scope)

Independent review of candidate `9baa6901256929fe0a00cca7717d50ca179a68bf` against base `8c2d177505852b3e39cd77f4f782fb355de245aa`. Verdict: **approved**.

## Scope inspected

Diff `8c2d1775..9baa6901` touches `crates/gyre-cli/{Cargo.toml,src/client.rs,src/main.rs}`, `docs/cli.md`, `Cargo.lock`, task/review files. All product-file changes in the range trace exclusively to the five commits listed in the task frontmatter (`git log -- <file>` cross-check: touched-by-unlisted = none for every changed product file).

## Static verification

- **Routes are real.** Builders hit exactly `GET/PUT /api/v1/workspaces/{id}/budget` and `GET /api/v1/budget/summary` — byte-identical to the registrations at `api/mod.rs:610-613`. No invented routes.
- **Wire shapes match the server.** CLI `BudgetConfig`/`BudgetUsage`/`BudgetResponse`/`TenantBudgetSummary` mirror `gyre-domain/src/budget.rs` and `api/budget.rs:55-76` field-for-field; no serde renames. Domain `BudgetUsage.entity_id: Id` serializes transparently as a plain string (server's own test asserts `v["entity_id"] == "proj-1"` as a string), so the CLI's `String` field is correct.
- **Fetch-merge-put is required and correct.** Server PUT (`api/budget.rs:121-126`) constructs a fresh `BudgetConfig` from the request body — a full replace. `merge_budget_config` (`Option::or` per field) preserves unset limits; the task's "otherwise fetch-merge-put" branch is the one taken.
- **Resolver reuse, not duplication.** `resolve_budget_workspace` composes `infer_repo_from_git_remote()` (main.rs:1912) with the existing `GyreClient::resolve_workspace_slug` (client.rs:358) — the same pair `deps`/`explore`/`Divergence` use. No second resolver.
- **Repo scope honestly mapped.** Help text on all three surfaces (`budget`, `show`, `set`) states repo scope resolves to the owning workspace budget; no repo-keyed store fabricated. `set --tenant` bails honestly (no tenant set endpoint exists server-side).
- **`http = "1"` is a dev-dependency only** (test response construction); `Cargo.lock` gains exactly that entry.

## Independent probes (evidence in `/tmp/stage/review-evidence/`)

- `cargo test -p gyre-cli --bin gyre budget` → **13 passed, 0 failed** (exit 0). Full bin suite: **107 passed, 0 failed**.
- `bash scripts/check-arch.sh` → passed (exit 0).
- **Mutation probes (source restored byte-identical after each, md5-verified against the candidate commit):**
  - M1 wrong URL (`budget`→`budgets`): `get_workspace_budget_builds_real_route` + `set_workspace_budget_put_body_is_merged_config` FAIL. Tests are URL-load-bearing.
  - M2 revert the zero-limit `budget_util` fix: `budget_util_zero_limit_and_boundary` FAIL (left `"100%+"`, right `"0%"`). The fixed defect's test kills its regression.
  - M3 swallow server error body (drop `{text}` from bail): `budget_errors_surface_server_body_verbatim` FAIL. Verbatim 403/400 surfacing is load-bearing.
  - M4 drop the fetch-merge (send overrides only): `set_workspace_budget_put_body_is_merged_config` FAIL (`max_cost_per_day` wiped to null). Merge semantics are load-bearing.
  - Reproduction script: `task192-mutation-probes.sh`.
- **Live binary probes** (built from candidate; server unreachable by design — refused-connection URL is the assertion, since TCP listeners are forbidden in this sandbox per `capabilities.json` errno 95):
  - `gyre budget show` from a repo with remote `…/git/platform-team/widgets.git` → real `GET /api/v1/workspaces?slug=platform-team` (git-remote → slug → id chain reached).
  - `budget show --tenant` → `GET /api/v1/budget/summary`; `--workspace-name nope` → `GET /api/v1/workspaces?slug=nope`.
  - No git remote → honest error naming `--workspace-name`; `set --tenant` → honest no-endpoint error; `set` with no limits → error naming all four flags; `--tenant --workspace` → conflict rejection.
  - Help text verified on all three surfaces.
- **Commit attribution**: `check-task-commit-attribution.sh` fails only on pre-existing task-210 (`a781ede2`, unrelated to this candidate range); zero task-192 violations, and every product-surface change in the range comes from a listed commit.

## Notes (non-blocking)

- `--workspace` on `show`/`set` is behaviorally identical to the default repo scope (both resolve the owning workspace). The spec's example forms (`budget set --workspace --llm-cost 100.00`) work verbatim; the flag documents intent. Consistent with the task's own framing ("default = repo→workspace").
- Live-server end-to-end (real 403/400 round-trip, persisted update) is not exercisable here — TCP listener binds fail with errno 95 (`capabilities.json`). The verifier's strongest available substitutes were run: exact request-shape assertions through `RequestBuilder::build()` (the same `Request` reqwest hands to the connection layer), real `reqwest::Response` error-path tests carrying the server's exact wire shape, and live binary probes reaching the real routes. Host/CI should run the live-server suite (`gyre-cli` `ws_integration` + server budget handler tests) to close that last mile.
