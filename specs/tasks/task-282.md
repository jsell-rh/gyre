---
title: "Serve the spec's literal /api/v1/deps/* API surface (all ten rows, ?workspace= graph scoping)"
spec_ref: "Spec gap spec-cab97078efffe0fbfb7339be / deps-spec-route-paths / ed9f34316476"
depends_on: [task-281]
progress: ready-for-review
commits: ["53b8b41983bdf52dc96e9f5d72dd8c8621a8eca8", "88309116208775e04bc778364f335ae5afcb6688"]
---

## Required behavior

Every row's PURPOSE is implemented by a real handler today, but none of the spec's literal paths is served. Register the spec's literal routes in crates/gyre-server/src/api/mod.rs, backed by the real handler cores: GET /api/v1/deps/graph (full tenant-wide graph) with the SAME path accepting ?workspace={id} for the workspace-scoped graph (query-parameter scoping on the graph endpoint — not only the dedicated /workspaces/:id/dependency-graph path); GET /api/v1/deps/repo/{id}/dependents; GET /api/v1/deps/repo/{id}/dependencies; GET /api/v1/deps/repo/{id}/impact (full blast-radius analysis — including the cascade test results and test status delivered by impact-cascade-results-ui); GET /api/v1/deps/stale; GET /api/v1/deps/breaking; POST /api/v1/deps/manual (declare a manual edge — the source repo is derived from or validated against the caller's scope; no unvalidated caller-supplied tenant/workspace/repo scoping fields per check-forged-scope-fields); DELETE /api/v1/deps/{id} (manual-only removal — refuses non-manual edges, same semantics as the existing delete handler); POST /api/v1/deps/breaking/{id}/acknowledge. The existing routes (/api/v1/dependencies/*, /api/v1/repos/:id/*, /api/v1/workspaces/:id/dependency-graph) keep working — the satisfied CLI and UI consume them; the spec paths are additions, not a breaking rename. Every new route gets an ABAC RouteResourceMapping entry in the abac_middleware resolver (scripts/check-abac-route-registry.sh — no exemption-file growth, no duplicate resolver entries), and any route that runs without middleware ABAC enforces per-handler authorization (load the scoped entity, compare tenant/workspace against the caller, Forbidden on mismatch). Update docs/api-reference.md to document the spec-literal paths.

## Acceptance Criteria

- [ ] Each of the ten spec rows answers on its literal path with the specced purpose: tenant-wide graph; workspace-scoped graph via GET /api/v1/deps/graph?workspace={id}; per-repo dependents; per-repo dependencies; full blast-radius impact (with cascade results and test status); stale list; unacknowledged breaking list; manual edge declaration; manual-only delete (a non-manual/auto-detected edge is refused with an error); breaking-change acknowledge.
- [ ] A consumer written against the canonical spec table gets zero 404s across all ten rows.
- [ ] The existing CLI commands (gyre deps show/graph/impact/stale/breaking/add/acknowledge) still work unchanged against the existing routes.
- [ ] scripts/check-abac-route-registry.sh and check-abac-exempt-handlers.sh pass with all new routes registered in the resolver (no new exemption entries), and docs/api-reference.md lists the /api/v1/deps/* paths.

## Verification intent

curl GET /api/v1/deps/graph with a valid token: pre-fix the route does not exist — grep for 'api/v1/deps' across crates/ and web/src returns zero matches, and every row of the spec's API table 404s because the implemented handlers live only under /api/v1/dependencies/*, /api/v1/repos/:id/*, and /api/v1/workspaces/:id/dependency-graph — so a spec-conforming consumer gets 404 on every row and the probe must fail until the literal paths are registered with ABAC mappings. Second probe: DELETE /api/v1/deps/{id} against an auto-detected (non-manual) edge must be refused — proving manual-only deletion semantics survived the new route.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/dependency-graph.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `1745ed1044f3de3976ac34455f356a3300421816ea7a6e7574e2e5d9ec4ad3a4`.

[
  {
    "id": "79be3a435216d41c4ebe0660",
    "decision": "gap",
    "requirement_quote": "| `GET /api/v1/deps/graph` | GET | Full tenant-wide dependency graph |",
    "detail": "Per the cycle-3 review correction (spec-endpoint-paths-not-served), this section is a gap. Every row's PURPOSE is implemented by a real handler — tenant graph, workspace-scoped graph, per-repo dependents/dependencies, blast radius, stale list, unacknowledged breaking list, manual add, manual-only delete, acknowledge — but NONE of the spec's literal paths is served: grep for 'api/v1/deps' across crates/ and web/src returns zero matches. The routes are registered as /api/v1/dependencies/graph, /api/v1/workspaces/:id/dependency-graph, /api/v1/repos/:id/dependencies(+/:dependency_id), /api/v1/repos/:id/dependents, /api/v1/repos/:id/blast-radius, /api/v1/dependencies/stale, /api/v1/dependencies/breaking(+:id/acknowledge). Workspace scoping is a dedicated path rather than the spec's `?workspace={id}` query parameter on the graph endpoint (the stale endpoint does accept ?workspace_id=). A consumer written against the canonical spec gets 404 on every row; docs/api-reference.md documenting the implemented shapes is not a spec-lifecycle amendment renaming the endpoints, and nothing in the spec makes the literal paths unsatisfiable.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "quote": "        .route(\"/api/v1/dependencies/graph\", get(dependencies::get_graph))",
        "detail": "The tenant-wide graph is served at /api/v1/dependencies/graph, not the spec's /api/v1/deps/graph; grep for the literal 'api/v1/deps' prefix across the entire codebase (crates/ and web/src) returns 0 matches."
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "quote": "            \"/api/v1/workspaces/:id/dependency-graph\",",
        "detail": "Workspace scoping is a dedicated route rather than the spec's `GET /api/v1/deps/graph?workspace={id}` query-parameter form."
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "quote": "        .route(\n            \"/api/v1/repos/:id/dependencies\",\n            get(dependencies::list_dependencies).post(dependencies::add_dependency),\n        )",
        "detail": "Dependents/dependencies/manual-add/delete live under /api/v1/repos/:id/* instead of /api/v1/deps/repo/{id}/* and /api/v1/deps/* — the handlers are real (dependencies.rs), the paths are not the spec's."
      }
    ]
  }
]

## Shipped

All ten spec-literal `/api/v1/deps/*` rows are served by real handler cores,
as additions beside the existing routes (no rename — the CLI and UI consumers
are untouched):

### Routes (crates/gyre-server/src/api/mod.rs, task-282 block)

- `GET /api/v1/deps/graph` — tenant-wide graph; the SAME path accepts
  `?workspace={id}` for the workspace-scoped graph (`get_graph` gained a
  `GraphQuery` extractor: absent/empty query means tenant-wide graph,
  unchanged for the existing `/api/v1/dependencies/graph` consumers; unknown
  workspace returns 404, not an empty graph).
- `GET /api/v1/deps/repo/{id}/dependents|dependencies|impact` — same handler
  cores as `/api/v1/repos/:id/dependents|dependencies|blast-radius`; impact
  carries the full task-281 blast radius (per-dependent `test_status`, durable
  `cascade_test_results`).
- `GET /api/v1/deps/stale`, `GET /api/v1/deps/breaking` — same handlers as
  the `/api/v1/dependencies/*` routes.
- `POST /api/v1/deps/manual` (`add_manual_dependency`) — body carries only
  repo identities and edge attributes (no tenant/workspace scoping fields,
  check-forged-scope-fields clean); the source repo is validated against the
  caller's tenant (`require_repo_tenant_containment`, Forbidden on mismatch)
  and against repo/workspace-scoped token claims.
- `DELETE /api/v1/deps/{id}` (`delete_dependency_by_id`) — manual-only
  removal: an auto-detected edge is refused with InvalidInput and survives;
  per-handler tenant containment runs before any refusal detail is revealed.
- `POST /api/v1/deps/breaking/{id}/acknowledge`
  (`deps_acknowledge_breaking_change`) — same acknowledgment core as the
  existing route, plus per-handler tenant containment against the change's
  source repo; idempotent.

All ten routes are registered in the ABAC resolver
(crates/gyre-server/src/abac_middleware.rs — per-repo routes use the
`:repo_id` param so the repo-to-workspace-to-tenant scope chain resolves;
the flat write routes enforce per-handler containment).

### Test and gate evidence

- `cargo check -p gyre-server --lib --tests` — exit 0 (after finishing the
  previous checkpoint's unfinished fix: Debug derives on the response DTOs
  the new `expect_err`-based probe tests require; response shapes unchanged).
- `cargo check -p gyre-cli` — exit 0; the CLI consumes the existing routes
  over HTTP and calls no handler directly, so it is unchanged.
- `scripts/check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh`,
  `check-forged-scope-fields.sh` — all pass, no new exemption entries.
- Standalone axum-0.7 routing probe (registered the exact route shapes):
  all ten literal rows match — zero 404s, no Router conflict panic, and a
  GET on the flat `:id` path 405s, proving the DELETE registration matched
  rather than a catch-all. Saved under /tmp/stage/review-evidence.
- Probe tests in the `api::dependencies` tests module:
  `deps_graph_endpoint_workspace_query_scopes_the_graph`,
  `deps_manual_endpoint_declares_edge_and_enforces_caller_scope`,
  `deps_delete_by_id_is_manual_only_and_tenant_contained`,
  `deps_acknowledge_endpoint_is_tenant_contained_and_effective`.

### Deferred to the independent verifier

gyre-server test compilation exceeds this sandbox's 4 GiB build budget (the
build wrapper blocks it), so the verifier must run:

- `cargo test -p gyre-server --lib api::dependencies::tests` (the four
  task-282 probe tests plus the pre-existing dependencies tests as
  regression cover for the unchanged existing surface);
- `cargo test -p gyre-server --test task164_dependency_graph_detail`
  (updated for the `get_graph` Query parameter; compiles per
  `cargo check --tests`).

docs/api-reference.md documents all ten `/api/v1/deps/*` paths, and coverage
row 15 of specs/coverage/system/dependency-graph.md is re-evidenced with the
literal-path source locations.
