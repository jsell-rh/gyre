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

## Independent review round (candidate `8a04176a`, base `770785f7`)

Fresh verification of the exact assignment: base `770785f7` → candidate
`8a04176a`. The candidate range carries the product surface as recovery
checkpoint `f4bdb092` (squashed preservation of the reviewed branch; `git diff
f4bdb092 8a04176a -- crates/gyre-cli/ docs/cli.md` is empty), the merge with
the assignment base (`22e77241`), and two `process:` lifecycle commits.

All probes re-run independently at the candidate; evidence under
`/tmp/stage/review-evidence/` (files `task192-*-review.txt`, `task192-*-{cmd,show,set}.txt`,
`task192-live-*.txt`, `task192-mutation-*.txt`):

- `cargo test -p gyre-cli --bin gyre` → **107 passed, 0 failed** (budget
  filter: 13 passed, 0 failed) — `task192-full-cli-bin-tests.txt`,
  `task192-budget-tests-review.txt`.
- `SKIP_WEB_BUILD=1 cargo build --all` → clean.
- `bash scripts/check-arch.sh` → passed; `bash scripts/check-relative-path-defaults.sh`
  → OK (exemption pointer `main.rs:1862` matches the code line at this HEAD —
  the candidate re-pointed it because the budget command moved the line, and
  the flagged site is the pre-existing task-099 F6 hazard, untouched by this
  task); `bash scripts/check-task-attribution.sh` → OK;
  `python3 scripts/check-rustfmt-diff.py 770785f7` → changed lines clean.
- **Mutation check re-run at this candidate**: URL `budget`→`budgets` in both
  GET/PUT builders fails exactly the 2 wiring tests
  (`get_workspace_budget_builds_real_route`,
  `set_workspace_budget_put_body_is_merged_config`); source restored
  (md5-verified against the pre-mutation checksum), full budget suite re-green
  — `task192-mutation-url-check.txt`, `task192-pre-mutation-md5.txt`,
  `task192-post-mutation-restore.txt`. Worktree clean after restore.
- **Live binary probes** (refused connect = the assertion; this sandbox
  forbids loopback listeners, errno 95 per `capabilities.json`): from a repo
  whose origin is `…/git/platform-team/widgets.git`, bare `budget show` reaches
  `GET /api/v1/workspaces?slug=platform-team`; `--tenant` reaches
  `GET /api/v1/budget/summary`; `--workspace-name core` reaches
  `GET /api/v1/workspaces?slug=core`; `set --llm-tokens 500000` and
  `set --workspace --llm-cost 100.00` resolve the workspace first (spec's
  exact forms); `set --tenant`, empty `set`, `--tenant --workspace`, and
  `--tenant --workspace-name` all exit 1 with honest errors; bare `show`
  outside a gyre-cloned repo exits 1 telling the user to run from a
  gyre-cloned repository or pass `--workspace-name` —
  `task192-live-*.txt`.
- **Help surfaces** (`task192-help-{cmd,show,set}.txt`): all three document
  that repo scope maps to the owning workspace budget.
- `ws_integration::test_auth_and_ping_roundtrip` fails with
  `Os { code: 95, kind: Unsupported }` at the TCP bind — matches the recorded
  sandbox restriction; the test file is untouched by this task (last modified
  `1a0821a`, pre-base). Environmental, not a code defect —
  `task192-ws-integration-env-check.txt`.

Wire-format and server-side re-verification (independent of prior rounds):

- Routes `GET/PUT /api/v1/workspaces/:id/budget` and
  `GET /api/v1/budget/summary` confirmed registered at `api/mod.rs:610-613`;
  client builders produce byte-identical URLs.
- Client `BudgetConfig`/`BudgetUsage`/`BudgetResponse`/`TenantBudgetSummary`
  field names confirmed against `gyre-domain/src/budget.rs` and
  `api/budget.rs:55-76` (`Id` is a transparent serde newtype, so
  `entity_id: Id` on the server vs `String` on the client round-trips).
- Fetch-merge-put confirmed necessary: server `set_workspace_budget`
  (`api/budget.rs:121-126`) replaces the whole config; the client's
  merge keeps unset limits (`merge_budget_config_keeps_unset_limits` +
  the PUT-body test assert this).
- Verbatim error surfacing confirmed against the server: `ApiError` renders
  `{"error": msg}` (`error.rs:71`); the 403 body text in the client test
  matches `budget.rs:117-119` and the 400 cascade text matches
  `budget.rs:137-147` character-for-character.
- `--repo` flag from the plan sketch was dropped in favor of
  `--workspace-name <SLUG>`: plan item 1 said "follow the existing patterns"
  and allowed the deviation implicitly; the spec excerpt's own examples never
  use `--repo`; the acceptance criteria name `--workspace`-targets-a-named-
  workspace via slug, and `docs/cli.md` documents the shipped forms.
  Deviation is documented in the task's Shipped section. Not a contract breach.

Host/GitHub verification to record: live-server round-trip (real 403 for
non-Admin, real 400 cascade for a limit above the tenant ceiling, updated
budget persisted and re-read by `budget show`).

Verdict: **approved** — no findings. The implementation satisfies the task
contract: real endpoints only, honest repo→workspace scope mapping stated in
help and docs, load-bearing wiring tests, correct merge semantics, verbatim
error surfacing, and all mechanical gates green at the exact candidate.
