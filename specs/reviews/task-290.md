# Review — task-290 (Spec Links: repo-scoped graph + authorization for the four registry-exempt query endpoints)

Candidate: `1bd9a4440c9c2070e06e59b1cef9264e3768dc76`
Base: `077d9421069afb2ef2e788b74285186176084b7c`
Reviewer round: merge-reconciliation review (previous candidate 11253ce3
conflicted with its target; this round resolves against the task-291 base).

## Verdict

**Approved.** Both halves of the spec-links.md §Querying the Graph gap are
really implemented; the merge resolution preserved the implementation intact
and correctly recombined task-291's `direction` field into the four touched
handlers without regression.

## What was verified

### (A) Repo-scoped graph — real implementation

`get_spec_graph` (api/specs.rs:1781) now takes `Query<SpecGraphQuery>`:

- No `repo` parameter → nodes/edges unfiltered — the full graph, unchanged
  (AC1; asserted by the merged test alongside the filtered call).
- `?repo={id}` → nodes filtered by `SpecLedgerEntry.repo_id == repo`, edges by
  `source_repo_id == repo || target_repo_id == repo` — exactly the specced
  predicate, so cross-repo inbound edges are included while foreign-repo
  nodes are excluded (AC2; test
  `get_spec_graph_repo_filter_returns_repo_nodes_and_inbound_edges` seeds a
  two-repo tenant with a repo-2→repo-1 depends_on link and asserts
  nodes == [repo-1's spec] and the inbound edge present — both assertions
  fail on base code, where `?repo` is ignored entirely).
- Invalid input: unknown repo id → 404 via a real `repos.find_by_id` miss;
  empty `?repo=` → 400 (AC3; test
  `get_spec_graph_unknown_or_empty_repo_is_an_explicit_error`). A repo in
  another tenant → 403 (in-handler containment; the collection route names
  no entity the middleware scope chain could resolve; test
  `get_spec_graph_repo_filter_denies_cross_tenant_repo`).

### (B) Authorization for the four query endpoints — real enforcement

- All four routes removed from `scripts/abac-route-registry-exemptions.txt`
  (42 → 38 entries; exactly the four task-named lines, nothing else;
  `/api/v1/specs/:path/reject` untouched as required) and registered as
  `RouteResourceMapping::api(..., "spec", None)` in abac_middleware.rs:435-438,
  each appearing exactly once (no duplicates).
- `scripts/check-abac-route-registry.sh` and
  `scripts/check-abac-exempt-handlers.sh` both pass with the strictly smaller
  file (AC4). The frozen-exemption baseline in the check script was lowered
  46 → 38 in the same commit — a tightening consistent with the check's
  documented intent ("must shrink as routes are moved into the resolver").
- Cross-tenant containment (AC5): each of the four handlers now takes
  `AuthenticatedAgent` and scopes the store read through `links_visible_to`
  (api/specs.rs:1895): rows visible only when their declaring repo
  (`source_repo_id`) belongs to the caller's tenant, enumerated over the real
  workspace/repo stores; the system principal (global token) legitimately
  spans tenants; rows with no declaring repo are withheld rather than given a
  fabricated scope. For `/:path/dependents` and `/:path/dependencies` the
  middleware additionally resolves the addressed spec's tenant
  (`extract_entity_attributes` → `set_tenant_from_repo`), so
  `builtin-tenant-isolation` (Deny, priority 950) policy-denies cross-tenant
  spec queries. Test `spec_graph_query_endpoints_do_not_leak_other_tenants_links`
  seeds two tenants into ONE shared forge-wide store and asserts tenant B
  receives only its own rows on all four endpoints — on base code each call
  succeeds and returns tenant A's rows.

### Merge-resolution fidelity

`git diff 11253ce3..1bd9a444` (previously verified implementation → this
candidate) contains only task-291's direction-field work arriving from the
new base, correctly recombined: `SpecLinkResponse::direction` +
`with_direction_relative_to` applied in `get_spec_links`,
`get_spec_dependents`, `get_spec_dependencies` after the tenant scoping. The
task-290 sections (`SpecGraphQuery`, repo validation, `links_visible_to`,
handler signatures) are retained verbatim. The conflicted coverage file
(specs/coverage/system/spec-links.md rows 12/13) merges both tasks' notes
with refreshed line citations (spot-checked: specs.rs:1781/1895,
abac_middleware.rs:435-438 all accurate).

### Probes run (evidence: /tmp/stage/review-evidence/)

- `cargo check -p gyre-server` — exit 0 (8m49s cold).
- `cargo fmt -p gyre-server --check` — exit 0.
- `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh`,
  `check-arch.sh`, `check-forged-scope-fields.sh`,
  `check-scope-literal-defaults.sh`, `check-fabricated-scope-defaults.sh`,
  `check-inert-enforcement.sh`, `check-mem-port-contracts.sh` — all exit 0.
- Static trace of the auth/policy chain (API-key → user → tenant;
  builtin-readonly-get-only Allow 600 vs tenant-isolation Deny 950 with
  unset resource attrs), corroborated by the existing
  `roleless_caller_gets_no_fabricated_readonly_role` test's documented
  killing direction.

### Deferred to the independent verifier (4 GiB budget)

gyre-server test compilation. Execute each as its own
`cargo test -p gyre-server <name>`:
`get_spec_graph_returns_nodes_and_edges` (regression),
`get_spec_graph_repo_filter_returns_repo_nodes_and_inbound_edges`,
`get_spec_graph_unknown_or_empty_repo_is_an_explicit_error`,
`get_spec_graph_repo_filter_denies_cross_tenant_repo`,
`spec_graph_query_endpoints_do_not_leak_other_tenants_links`,
plus the task-291 merge-regression direction tests
(`get_spec_links_direction`, `get_spec_links_self_link_is_outbound`,
`get_spec_dependents_and_dependencies_populate_direction`).

## Non-blocking observations

1. The new tests drive `api_router()` directly (repo-wide convention), which
   does not layer `abac_middleware` (that happens in `build_router`), so they
   prove the handler-level scoping (what AC5 measures) while the middleware
   mappings are proven mechanically by check-abac-route-registry.sh. A
   middleware-level cross-tenant denial test for `/:path/dependents` would
   strengthen, but is not required by the contract.
2. `GET /api/v1/specs/:path/links` and the no-parameter graph still serve
   forge-wide rows to policy-visible callers — out of this task's assigned
   scope (the task mandates the no-param graph "unchanged" and the controller
   classified `/:path/links` as the acceptable contrast). Candidate follow-up
   gap for the controller.
3. Generated output: the candidate's `web/` tree is identical to the assigned
   base; no pending dist regeneration from this candidate's own changes. The
   verifier's clean rebuild remains binding.
