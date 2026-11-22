---
title: "Serve the spec's literal /api/v1/deps/* API surface (all ten rows, ?workspace= graph scoping)"
spec_ref: "Spec gap spec-cab97078efffe0fbfb7339be / deps-spec-route-paths / ed9f34316476"
depends_on: [task-281]
progress: not-started
commits: []
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
