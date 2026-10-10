# Review — task-155 (search.md §CLI — `gyre search` command)

Spec: `specs/system/search.md` §CLI (lines 152-160) — the six CLI invocations:
simple search, `--type`, `--type`+`--status`, `--type commit --since 7d`,
`--workspace`, `--suggest`.
Candidate: `5a97f049b90883d915bb58443f27fcc989638687` (base
`e96d25abcdbb51f8890ea36d11541bfa9f80a2b8`). Diff touches
`crates/gyre-cli/src/main.rs`, `crates/gyre-cli/src/client.rs`, `docs/cli.md`,
`specs/tasks/task-155.md` only — no server code changed.
Verdict: **approved**.

## What was verified (independent probes; evidence in /tmp/stage/review-evidence/)

- **Contract equality**: `requirement_parts(assigned job body) ==
  requirement_parts(candidate task file)` — front and prose both equal (the
  prior contract finding `d1fac5b9` is repaired; the baseline-repair notes now
  live under `### Baseline repair` inside `## Shipped`, which
  `requirement_parts` strips). `check-task-commit-attribution.sh` exit 0.
- **Wire contract is real**: `GyreClient::search` (client.rs:390-418) sends
  exactly the params the server's `SearchParams` deserializes
  (search.rs:16-23): `q`, `entity_type`, `workspace_id`, `limit`. Observed
  live in reqwest error URLs from the built binary — `--type task --status
  in_progress` produced `?q=q&entity_type=task&limit=100` (the over-fetch
  `max(limit, 100)` before client-side filtering), `--limit 5` produced
  `limit=5`, bare query produced `limit=20` (default). Response types match
  the server's Serialize structs field-for-field (`SearchResultItem` →
  `SearchResult`).
- **Flags filter, not fake**: the checkpoint's q-folding approach (append
  `status:approved` to `q`) was rejected and replaced — under the server's
  AND-of-terms engine (`score_doc` mem_search.rs:28-47) those tokens are
  unmatchable, so the flags would have silently annihilated results. Instead
  `--status`/`--since` resolve live entity state per result (`get_task`,
  `get_mr`, `get_agent` — real endpoints, routes confirmed in api/mod.rs:290,
  :301, :234) and filter with fail-closed semantics for unresolvable state
  (spec/commit types, deleted entities counted on stderr). This is a genuine
  improvement over both the checkpoint and a naive facet-fold.
- **Mutation probes (tests kill real bugs)** — each mutation applied to
  main.rs, focused tests run, source restored (sha256-verified, tree clean):
  - filter predicate forced always-true → 3 tests fail
  - suggest prefix filter dropped → 2 tests fail
  - `parse_since` forced to accept everything → 6 tests fail
  (reproducible via `mutation-probes.sh`; log in `mutation-probes.log`)
- **Binary smoke** (built from candidate, temp HOME config, dead port):
  `search --help` shows all six flags incl. `-t`/`-w` shorts; no-query prints
  usage exit 0; `--since bogus` / `2026-13-01` / `2026-02-30` / `é` all exit 1
  with a clean message (multibyte handled without panic — char-boundary-safe
  `next_back` split, covered by `parse_since_multibyte_unit_does_not_panic`);
  workspace slug resolves through `GET /api/v1/workspaces?slug=...` before
  search (endpoint confirmed: workspaces.rs:160-168 filter is exact-match;
  `(tenant_id, slug)` UNIQUE per migration 000019).
- **Suites and gates**: `cargo test -p gyre-cli --bin gyre` → 110 passed, 0
  failed. `cargo clippy -p gyre-cli --all-targets` → clean except the
  pre-existing `gyre-common` loop-counter warning (view_query.rs:798, crate
  untouched by this diff, present at base). `check-rustfmt-diff.py` →
  "changed lines clean (2 Rust files checked)". All mechanical checks pass:
  arch, byte-slice-truncation, unbounded-external-http (CLI's `Client::new()`
  predates the diff and is a first-party localhost client, not an external
  fallback path), relative-path-defaults (the bootstrap starter-kit repair in
  `4c0df440` converts a cwd-relative default into a hard error — correct fix
  for the F6 class), fail-open-ref-resolution, forged-scope-fields,
  inert-enforcement, mem-port-contracts, fabricated-scope-defaults,
  scope-literal-defaults, lossy-secret-conversion, in-memory-state-stores,
  dead-message-kinds, mcp-write-tools, forwarded-header-trust,
  migration-sql-portability, migration-versions, abac-route-registry,
  abac-exempt-handlers.

## Task-contract assessment

Acceptance criteria all met within the task's own scoping rules: search API
called and results displayed as `[type] title (id)` + snippet lines;
`--type`/`--workspace` server-side, `--status`/`--since` client-side against
live state (the plan explicitly authorizes this split pending task-153/154);
`--suggest` documented fallback (plan explicitly authorizes it "until
task-153 adds the /search/suggest endpoint"); `--limit` default 20 with
server cap 100 respected; the three required parse tests pass plus
substantive units. The implementation is honest about its boundaries —
unresolvable state fails closed and is reported, rather than guessed at.

## Not defects (recorded, outside this task's diff)

- Server-side index gaps: all three index write sites pass
  `workspace_id: None`, so `--workspace` filters server-side can never match
  current indexed docs; facets freeze at create-time values; only task/mr/
  agent types are indexed at all. These are the pre-existing coverage rows 3/
  4/5/6/8 (task-201/202/203) — untouched by this diff and explicitly out of
  scope for a CLI task. The CLI's live-state resolution actually mitigates the
  frozen-facet problem for `--status`/`--since` rather than inheriting it.
- Live E2E against a real server could not run here: the sandbox cannot
  `accept()` TCP (errno 95, capabilities.json). Wire contract verified by
  source + observed request URLs; host verification / GitHub CI should run
  the flow listed in review-probes.md §7.
