# Review — task-290 (Spec Links: repo-scoped graph + authorization for the four registry-exempt query endpoints)

Spec: `specs/system/spec-links.md` §Querying the Graph (API).
Candidate under review: `25181db65ec1ed4469a6c5993feda49ad2169ce5` (branch
`pipeline/task-290/5298525055d144ae870eb5a13dae0c2f-1`, assigned base
`80439c18da824274672787152b03993f84808afa`; product commits `f96b5a40`,
`b675ee0a`, `8cced7f4`, merge reconciliation `b364363f`/`210f41a3`).
Verdict: **approved** — no findings. Both halves of the gap are implemented
with real production behavior; all mechanical checks pass on the exact
candidate; the four new server tests are statically sound but could not be
executed in this sandbox (build wrapper rejects gyre-server test codegen in
the review stage) and are recorded below for the independent verifier.

## Probes run (exact candidate tree)

- `bash scripts/check-abac-route-registry.sh` → exit 0 (exemption file
  42 → 38 entries; `FROZEN_EXEMPTION_COUNT` tightened 46 → 38 — the file
  already sat at 42 below the old frozen 46; only the four in-scope routes
  were deleted, nothing added).
- `bash scripts/check-abac-exempt-handlers.sh` → exit 0 (86 handlers).
- `bash scripts/check-arch.sh` → exit 0.
- `bash scripts/check-inert-enforcement.sh`,
  `check-fabricated-scope-defaults.sh`, `check-scope-literal-defaults.sh`
  → exit 0 each.
- `bash scripts/check-task-commit-attribution.sh` → exit 0 on the candidate
  commit (the FAIL seen in the live working tree is the controller's reset
  of `specs/tasks/task-290.md` frontmatter to `not-started/[]`, which is not
  part of the candidate; the candidate records all three product SHAs).
- `/tmp/stage/bin/cargo check -p gyre-server --tests` → exit 0 (9m44s; no
  warnings in the new code). `cargo fmt -p gyre-server -- --check` → clean.
- Full diff read base..candidate for all six touched files; handler and test
  code read line-by-line in the candidate (working tree verified
  byte-identical to the candidate for `crates/` and `scripts/`).
- Evidence under `/tmp/stage/review-evidence/`.

## Verified working

**(A) Repo-scoped graph.** `get_spec_graph` (api/specs.rs:1781) takes
`Query<SpecGraphQuery>` (:1773): absent `repo` → the full tenant-wide graph,
node and edge construction unchanged from base. Present `repo` → the repo is
resolved and validated BEFORE filtering: unknown id → 404 (`repo '{id}' not
found`), empty value → 400, and for non-system principals a repo in another
tenant → 403 (per-handler tenant containment, workspace lookup fail-closed
via `is_some_and`, never a fabricated scope). Nodes filter on
`SpecLedgerEntry.repo_id == repo`; edges filter on `source_repo_id == repo ||
target_repo_id == repo` — outbound, same-repo inbound, and cross-repo inbound
— exactly the spec's "specs for one repo with all outbound/inbound links".
On base code the parameter is ignored entirely (no Query extractor), so the
new filter test's node-count and inbound-edge assertions fail on base — the
tests constrain the query, not the parameter spelling.

**(B) Authorization.** The four query endpoints (`GET /specs/stale-links`,
`/specs/conflicts`, `/{path}/dependents`, `/{path}/dependencies`) are
resolver-mapped as `RouteResourceMapping::api(..., "spec", None)`
(abac_middleware.rs:435-438, mirroring the pre-existing `/specs/graph` and
`/:path/links` mappings) and removed from
`scripts/abac-route-registry-exemptions.txt` — the file strictly shrank
(42 → 38) with no new exemptions and no duplicate resolver entries (duplicate
scan still at its frozen baseline of 2 pre-existing false positives).
`/api/v1/specs/:path/reject` was correctly left in the exemption file with
no resolver entry (out of contract). All four handlers now take
`AuthenticatedAgent` and scope the store read through `links_visible_to`
(specs.rs:1895): rows visible to a tenant-scoped caller are those declared by
repos of the caller's tenant (workspace → repo enumeration over the real
stores); the system principal (global token, `user_id.is_none()` — unfakeable
per auth.rs) legitimately spans tenants; rows with no `source_repo_id` are
withheld rather than given a fabricated scope (fail-closed). This is the
acceptance criterion's "scoped to tenant B" arm: a tenant B principal no
longer receives tenant A's declared link rows on any of the four endpoints.

**Policy path for the newly mapped routes (static trace, since tests are
deferred).** For the two no-entity collection routes the middleware cannot
resolve `resource.tenant_id`/`resource.workspace_id`, so the builtin
`tenant-isolation` (950) and `workspace-membership-required` (940) denies
keyed on those attributes do not fire; an API-key caller with the default
ReadOnly role matches `builtin-readonly-get-only` (600, Allow read on `*`)
→ 200, which is what the new tests assert. For the `:path` routes the entity
lookup sets `resource.tenant_id` through the ledger entry's repo → workspace
→ tenant chain, giving the same middleware tenant isolation the already-
mapped `/:path/links` route has. Pre-existing tests of these endpoints use
the global `test-token` (system principal) → `links_visible_to` returns the
full store → their assertions are unchanged.

**Test quality.** The four new tests (specs.rs:5295-5722) use one shared
forge-wide state for both tenants (a leak assertion against separate states
would be vacuous), real API-key users bound to distinct tenants, and the
real router. Negative arms are real: unknown repo → 404, empty repo → 400,
cross-tenant repo filter → 403, tenant A's stale/conflict/inbound rows
absent from tenant B's responses with the specific row ids asserted. On base
code every one of these fails (parameter ignored; handlers take no auth and
return the forge-wide store). Coverage rows 12/13 were updated with line
citations that match the merged tree (spot-checked: :1773/:1781/:1895/:1921/
:1960/:1995/:2018, abac_middleware.rs:435-438).

## Verifier must run (deferred: wrapper rejects gyre-server test codegen)

`cargo test -p gyre-server` (each name as its own run, or
`cargo test -p gyre-server spec_graph` for all four):

- `get_spec_graph_repo_filter_returns_repo_nodes_and_inbound_edges`
- `get_spec_graph_unknown_or_empty_repo_is_an_explicit_error`
- `get_spec_graph_repo_filter_denies_cross_tenant_repo`
- `spec_graph_query_endpoints_do_not_leak_other_tenants_links`

Plus the pre-existing endpoint tests that must not regress:
`get_spec_dependents_returns_inbound_depends_and_implements`,
`get_spec_dependents_spec_not_found`,
`get_spec_dependencies_returns_outbound_depends_and_implements`,
`get_stale_links_returns_only_stale`,
`get_conflicts_returns_conflicts_with_links`,
`get_conflicts_excludes_non_approved_conflicts`.

## Non-blocking observations (recorded for follow-up, not task-290 defects)

- The UNFILTERED `GET /api/v1/specs/graph` still returns the forge-wide
  ledger (`spec_ledger.list_all()`, no tenant scoping) to any authenticated
  principal. This is pre-existing behavior the controller's gap analysis
  explicitly classified as non-defective ("properly resolver-mapped");
  middleware policy on a no-entity route cannot constrain tenant either.
  Now that the four sibling endpoints are tenant-scoped, the asymmetry is
  worth a follow-up task.
- `docs/api-reference.md` (line 85) does not yet document the
  `?repo={id}` graph variant; the canonical spec table has it.
- The repo-filter containment compares tenant only (not workspace), so a
  workspace-scoped principal of the same tenant can filter another
  workspace's repo. The task text and acceptance criteria require tenant
  containment only.
