# Review — task-290 (Spec Links: repo-scoped graph `?repo=` filter + ABAC for the four registry-exempt query endpoints)

Spec: `specs/system/spec-links.md` §Querying the Graph (API). Gap record
`58102d3a27f920beeb3fba71:exempt-query-endpoints-no-authorization`.
Candidate under review: `73c447a7` (branch `pipeline/task-290/3884141732d6426ebd46201f71dbda0f-1`,
base `8e2e7458`; product commits `f96b5a40` + `b675ee0a`, process commits
`c0edace6`/`a28fd4a0`/`73c447a7` touch only the task file).
Verdict: **approve** — no findings. Both required behaviors are implemented in real
production code; all mechanical gates pass with a strictly smaller exemption file;
the four new tests are real router-level tests that fail on the base code. Runtime
execution of the gyre-server test binary was blocked by this sandbox (no crates.io
network; the review-stage wrapper forbids gyre-server codegen) and is recorded as a
deferred verifier check — an infrastructure restriction, not a code defect.

## Probes run

- `bash scripts/check-abac-route-registry.sh` → exit 0.
- `bash scripts/check-abac-exempt-handlers.sh` → exit 0 (86 handlers).
- Exemption file: 42 → **38** entries (strictly smaller); `FROZEN_EXEMPTION_COUNT`
  lowered 46 → 38 (tightened to the exact new count, with an explanatory comment);
  `/api/v1/specs/:path/reject` left exempt as the task requires.
- `bash scripts/check-arch.sh`, `check-in-memory-state-stores.sh`,
  `check-inert-enforcement.sh`, `check-scope-literal-defaults.sh`,
  `check-fabricated-scope-defaults.sh` → all exit 0.
- `bash scripts/check-task-commit-attribution.sh` → exit 0 against the candidate's
  committed task file (the worktree copy carries the executor's review-stage reset
  `commits: []`, which makes the same script fail against the worktree — artifact of
  the reset, not the candidate; worktree restored after the probe).
- `cargo check -p gyre-server --tests` → could not run: crates.io unreachable and no
  crate sources in CARGO_HOME (transport restriction). Compile surface verified by
  inspection instead (imports, port/domain signatures, error variants, struct-literal
  field sets, extractor ordering — all check out; details in
  `/tmp/stage/review-evidence/task-290-review-notes.md`).

## What was verified

**(A) Repo-scoped graph** — `get_spec_graph` (api/specs.rs:1757) now takes
`Query<SpecGraphQuery>` + `AuthenticatedAgent`. No `repo` param → the full graph,
byte-for-byte the previous construction (same node/edge mapping, now wrapped in
`Ok`). With `?repo={id}`: the repo is resolved first (`repos.find_by_id`) — unknown
id → 404, empty value → 400, and (beyond the letter of the task, correctly) a repo
in another tenant → 403 for non-system principals (workspace → tenant comparison,
fails closed on unresolvable workspace; the system principal legitimately spans
tenants). Nodes filter on `SpecLedgerEntry.repo_id == repo`; edges filter on
`source_repo_id == repo || target_repo_id == repo` — exactly the task's definition,
so a repo-2 inbound edge appears while no foreign-repo node does. The task's
verification intent (both assertions fail on base code, since base ignores `?repo`
entirely) is directly encoded in
`get_spec_graph_repo_filter_returns_repo_nodes_and_inbound_edges`.

**(B) Authorization** — the four query endpoints are resolver-mapped as
`RouteResourceMapping::api(..., "spec", None)` (abac_middleware.rs:435-438), exact
pattern matches to the router literals, no duplicates, and removed from
`scripts/abac-route-registry-exemptions.txt`. `:path/reject` untouched. Because the
two collection routes address no path entity (the middleware's `spec` entity lookup
keys on the `:path` param), the handlers additionally scope the store read to the
caller's tenant via `links_visible_to` (api/specs.rs:1866): rows are visible only
when their declaring `source_repo_id` belongs to a repo under a workspace of the
caller's tenant (real store enumeration; the system principal sees the full store;
rows with no `source_repo_id` are withheld rather than given a fabricated scope —
consistent with the repo's fabricated-scope-defaults invariant). For the :path
routes the middleware now runs real policy evaluation including builtin
`tenant-isolation` (950) via the ledger → repo → workspace → tenant scope chain, so
a cross-tenant spec path is policy-denied before the handler's row scoping. AC5's
"policy-denied or scoped to tenant B" is satisfied on both layers.

**Tests** — four new tests seed real workspaces/repos/users/API keys into ONE shared
AppState and drive the real router with real bearer tokens:
repo-filter + inbound-edge + unchanged-unfiltered-graph, 404/400 explicit errors,
cross-tenant repo 403, and a four-endpoint no-leak suite where tenant B must not
receive any of tenant A's stale/conflict/declared rows. Kill analysis: every
assertion that matters fails on base code (base ignores `?repo`; base returns the
forge-wide store). The system-principal carve-out preserves all pre-existing
test-token regression tests (verified against the global-token auth path:
agent_id "system", user_id None). Coverage rows 12/13 updated to `implemented`
with concrete source evidence — no `verified` claim, no unrelated rows touched.

## Verifier must run (deferred — sandbox transport restriction)

1. `cargo check -p gyre-server --tests`
2. `cargo test -p gyre-server --lib -- api::specs::tests::get_spec_graph_repo_filter_returns_repo_nodes_and_inbound_edges api::specs::tests::get_spec_graph_unknown_or_empty_repo_is_an_explicit_error api::specs::tests::get_spec_graph_repo_filter_denies_cross_tenant_repo api::specs::tests::spec_graph_query_endpoints_do_not_leak_other_tenants_links`
3. The pre-existing spec-graph regression tests (`api::specs::tests::get_spec_graph`,
   `get_spec_links`, `get_spec_dependents`, `get_spec_dependencies`,
   `get_stale_links`, `get_conflicts` filters) and `abac_middleware` resolver tests.
4. Re-run `scripts/check-abac-route-registry.sh` + `scripts/check-abac-exempt-handlers.sh`
   on the merged tree.

Full probe log: `/tmp/stage/review-evidence/`.
