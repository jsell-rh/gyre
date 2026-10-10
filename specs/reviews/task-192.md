# Review — task-192 (Platform Model §5 — Budget CLI: show and set at repo/workspace/tenant scope)

Spec: `specs/system/platform-model.md` §5 Resource Governance → CLI (budget show/set command tree). Comparison base `66422bd` → HEAD `79e92ec`; product surface = three `wip(task-192)` commits (`a5a8218` client types+methods, `6f4d136` main.rs command/dispatch/print, `4d16c6c` test rework onto `RequestBuilder::build()` + docs), the remaining commits are task-file lifecycle only.

Verdict: **complete**.

## Round 2 (independent verification; round 1 = mutation round)

Evidence reuse check: round 1's mutation runs were recorded at orphaned revision `bda3e8b` (earlier attempt lineage, reset and re-preserved as the three listed wip commits). `git diff bda3e8b HEAD -- crates/gyre-cli/` is empty; md5 of `client.rs` and `main.rs` identical at both revisions (only drift is `scripts/` python + task file, outside any crate). Mutation evidence is therefore valid for HEAD. Fresh runs this round at HEAD `79e92ec`:

- `cargo test -p gyre-cli -- budget` → **12 passed, 0 failed** (5 client wiring, 2 merge/shape, 1 verbatim-error, 6 CLI parse; 94 filtered out).
- `bash scripts/check-arch.sh` → passed.
- `cargo build -p gyre-cli --bins` → clean; live binary probes below.
- Prior round: `SKIP_WEB_BUILD=1 cargo build --all` clean; `cargo test -p gyre-cli` 106 passed with `ws_integration` failing only on `TcpListener::bind` (os error 95, sandbox forbids loopback listeners — file untouched by this task, last modified `1a0821a` pre-base; environmental, not a regression).

Live probes of the real binary (refused port = expected here; loopback listeners are disallowed, the URL in the error is the assertion):

- `gyre budget show` from a repo with remote `…/git/platform-team/widgets.git` → real `GET /api/v1/workspaces?slug=platform-team` (git-remote → slug → id resolution reached the real resolver).
- `gyre budget show --tenant` → real `GET /api/v1/budget/summary`.
- `gyre budget show --workspace-name nope` → real `GET /api/v1/workspaces?slug=nope`.
- `gyre budget set --tenant …` → exit 1, honest error "no tenant-level budget set endpoint exists (tenant:global limits are provisioned server-side)" — no invented route.
- `gyre budget set` with no limits → exit 1 with the four flag names.
- `gyre budget show --tenant --workspace` → exit 1 conflict rejection.
- Help surfaces (`budget`, `budget show`, `budget set`) all document that repo scope maps to the owning workspace.

Verified working (no findings):

- **Real endpoints, exact wiring.** Builders hit `GET/PUT /api/v1/workspaces/{id}/budget` and `GET /api/v1/budget/summary` — byte-identical to the routes registered at `api/mod.rs:610-613`. No invented routes. Auth header flows through the same `auth_header()` as every other client method.
- **Fetch-merge-put is required and correct.** Server PUT (`api/budget.rs:121-126`) replaces the whole `BudgetConfig`; the client's `set_workspace_budget` GETs the current config, merges `Option::or` per field, and PUTs the merged config — limits not passed keep their values. Mutation 2 (drop the merge → `max_cost_per_day` wiped to null) killed by `set_workspace_budget_put_body_is_merged_config`.
- **Wire shapes match the server exactly.** CLI `BudgetConfig`/`BudgetUsage`/`BudgetResponse`/`TenantBudgetSummary` field names mirror `gyre-domain/src/budget.rs` and `api/budget.rs:55-76` (domain `BudgetUsage.entity_id` is `Id(String)` — transparently string-serialized, `Id` is a `#[derive(Serialize, Deserialize)]` newtype). PUT body serializes to the server's `SetBudgetRequest` JSON exactly (asserted value-equal in the test).
- **Verbatim error surfacing is real.** `parse_budget_response` bails with `HTTP {status}: {body}`; server error wire shape is `{"error": msg}` (`error.rs:71`). The 403 non-Admin body ("only Admin role may update workspace budget limits", `budget.rs:117-119`) and the 400 cascade body ("…exceeds tenant limit…", `budget.rs:137-147`) surface verbatim. Mutation 3 (drop `{text}` from the bail) killed by `budget_errors_surface_server_body_verbatim`, which builds a real `reqwest::Response` from an `http::Response` carrying the server's exact status+body.
- **Workspace resolution reuses the existing helpers.** `resolve_budget_workspace` composes `infer_repo_from_git_remote()` (main.rs:1912, the same `/git/{ws}/{repo}.git` parser deps/explore/Divergence use) with `GyreClient::resolve_workspace_slug` (the `GET /api/v1/workspaces?slug=` first-match resolver, client.rs:358). No second resolver.
- **Repo scope honestly mapped, no fabricated store.** Help text (all three surfaces) and `docs/cli.md` state repo scope = owning workspace; no repo-keyed budget is invented.
- **Spec's exact command forms work.** `gyre budget set --llm-tokens 500000` and `gyre budget set --workspace --llm-cost 100.00` both parse (tested) and dispatch identically — correct, since workspace is the default (and only CLI-settable) scope; `--workspace` on `set` is a harmless explicit form of the default. `set --tenant` rejects honestly because no tenant set endpoint exists server-side — the only alternative would be an invented route, which the task forbids.
- **Tests are load-bearing.** Round 1 mutation evidence (isolated worktree, private target dir, source restored and worktree removed): wrong URL (`budget`→`budgets`) → 2 tests fail; dropped merge → PUT-body test fails; swallowed error body → verbatim test fails. 3/3 killed. The URL assertions run through `RequestBuilder::build()` — the same `Request` reqwest hands to the connection layer — so method/URL/header/body are the real sent values, not mirrored constants.
- **Mechanical gates.** `check-arch.sh` passed (CLI touches no boundary — it depends only on its own client). Commit attribution: the three listed commits are the only task-labeled product commits on the branch; the `process:` commits touch only `specs/tasks/task-192.md` (lifecycle-only, correctly outside `commits:` per convention). `http = "1"` is a dev-dependency only (test response construction) — appropriate placement, `Cargo.lock` gains exactly that entry.

Minor (not blocking, recorded for completeness):

- `--workspace` on `show`/`set` is a no-op boolean (scope is identical with or without it). The spec's example forms still work verbatim, and the flag documents intent; a stricter mutually-exclusive scope enum would be a stylistic change with no behavioral contract behind it.
- `resolve_workspace_slug` takes the first slug match and is tenant-unscoped — pre-existing helper behavior shared with `deps`/`explore`/`Divergence`, out of this task's scope.
- End-to-end verification against a live server (real 403/400 round-trips, updated budget persisted) is not possible in this sandbox (loopback listeners forbidden, os error 95); the strongest available substitutes were run: exact request-shape assertions through `RequestBuilder::build()` + `reqwest::Response::from` error-path tests + live binary probes reaching the real routes (connection refused at the exact spec URLs). The host/GitHub gates run the live-server suite.

## Independent review round (assigned candidate `a38fca98`, base `6bf777a6`)

Fresh independent verification at the exact assigned candidate (working tree clean, `git status --porcelain` empty; product surface byte-identical to the prior merged-head round — `git diff d98af1ba a38fca98 -- crates/gyre-cli/ docs/cli.md` and `git diff 175a5c80 a38fca98 -- crates/ docs/ scripts/` are both empty). Evidence under `/tmp/stage/review-evidence/` (suffix `-reviewer`).

- **Build + tests:** `cargo build -p gyre-cli --bins` clean; `cargo test -p gyre-cli --bin gyre` → 107 passed, 0 failed; budget filter → 13 passed, 0 failed.
- **Mutation probes (all killed, source restored md5-verified after each):**
  1. Corrupt both URLs (`budget`→`budgets`, `summary`→`summaries`) → 3 tests fail (`get_workspace_budget_builds_real_route`, `set_workspace_budget_put_body_is_merged_config`, `budget_summary_builds_real_route`).
  2. Drop the fetch-merge (`Option::or` removed) → 2 tests fail (`merge_budget_config_keeps_unset_limits`, `set_workspace_budget_put_body_is_merged_config`).
  3. Swallow the server error body (drop `{text}` from the bail) → `budget_errors_surface_server_body_verbatim` fails.
- **Wire shapes verified against server source:** CLI `BudgetConfig`/`BudgetUsage`/`BudgetResponse`/`TenantBudgetSummary` field names mirror `gyre-server/src/api/budget.rs:55-76` and `gyre-domain/src/budget.rs:5-20`; `Id` is a transparent string newtype (`gyre-common/src/id.rs:5-6`), so `entity_id` round-trips as a plain string. PUT body serializes to exactly `SetBudgetRequest`'s four fields. Server error wire shape is `{"error": msg}` (`api/error.rs:71`), Forbidden→403, InvalidInput→400 — matches the test fixtures and the real handler messages (budget.rs:117-119, 137-147).
- **Routes are real:** `api/mod.rs:610-613` registers `GET/PUT /api/v1/workspaces/:id/budget` and `GET /api/v1/budget/summary`, both with ABAC registry entries (`abac_middleware.rs:372-373`). The server PUT is a full config replace (budget.rs:121-126), which is why the client's fetch-merge-put is required, not cosmetic.
- **Live binary probes (loopback listeners forbidden in this sandbox, errno 95 — connection-refused at the exact URL is the assertion):** from a repo with remote `…/git/platform-team/widgets.git`, bare `budget show` reaches `GET /api/v1/workspaces?slug=platform-team` (real `infer_repo_from_git_remote` → `resolve_workspace_slug` resolution; both reused, not duplicated — main.rs:1911 is the same parser deps/explore use, client.rs:358 the same slug resolver); `--tenant` reaches `GET /api/v1/budget/summary`; `--workspace-name core` reaches `GET /api/v1/workspaces?slug=core`; git repo without a gyre `/git/` remote exits 1 with an honest inference error; `set --tenant`, empty `set`, and `--tenant --workspace` all exit 1 with honest errors.
- **Help surfaces (built binary, exit 0):** `budget`, `budget show`, `budget set` all document that repo scope maps to the owning workspace budget; no repo-keyed budget store fabricated anywhere in the diff (grep for hardcod/TODO/unimplemented/stub/fake in added lines: 0 hits).
- **Mechanical gates at candidate:** `check-arch.sh`, `check-relative-path-defaults.sh` (exemption pointer `main.rs:1862` matches the pre-existing task-099 F6 starter-kit site, merely shifted by inserted lines — verified the base's site at 6bf777a6:main.rs:1737 is byte-identical code), `check-task-commit-attribution.sh`, `check-abac-route-registry.sh`, `python3 scripts/check-rustfmt-diff.py 6bf777a6` → all exit 0.
- **Attribution:** all 8 frontmatter commits exist and are task-192-labeled product-surface commits; the extra task-labeled commits (`67ad071a`, `c1a762b7`, `6abdfdec`, `ed0669fb`) are recorded (checkpoint/implement entries touching specs/ + recovery of the same product surface) and the gate passes with them. The task-200 frontmatter addition (`6bf777a6`, the assignment base itself) is the check's documented remedy for upstream self-recording drift, mirroring prior task-211/task-220 repairs; the exemption file is untouched.

Verdict: **approved** — no findings. The task-200 commits-list repair is process metadata for a pre-existing upstream gap (the base commit predates this branch's work), not a product change; it was mutation-checked by the implementer and re-verified by the passing gate here. Live end-to-end HTTP round-trips (real 403/400/persisted budget) remain host/CI verification items — the sandbox forbids listeners (capabilities.json: `tcp_listener_probe.supported=false`, errno 95); the exact checks: run `cargo test -p gyre-server -- api::budget` (server-side 403/cascade tests exist at budget.rs:350+) and `cargo test -p gyre-cli --test ws_integration` on a host with loopback networking.
