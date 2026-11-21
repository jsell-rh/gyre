---
title: "Spec Links — repo-scoped graph (?repo= filter) and authorization for the four registry-exempt query endpoints"
spec_ref: "Spec gap spec-10bdced21a063f819b1baabd / spec-links-graph-repo-filter / 5a915155d9bc"
depends_on: []
progress: not-started
commits: ["f96b5a402e5f52ebf1f69775a5d3d139c591551a"]
---

## Required behavior

spec-links.md §Querying the Graph (API): two defects keep the section open. (A) The repo-filtered variant "GET /api/v1/specs/graph?repo={id} — Specs for one repo with all outbound/inbound links" is unimplemented: get_spec_graph (api/specs.rs:1742) takes only State, no Query extractor, and always returns every ledger node and every store edge. Add an optional repo query parameter: when absent, the response is exactly today's full tenant-wide graph (unchanged). When present: nodes are the ledger entries whose repo_id equals the given repo id (SpecLedgerEntry.repo_id), and edges are all stored links touching that repo — source_repo_id == repo (outbound and same-repo inbound) or target_repo_id == repo (cross-repo inbound) — i.e. the repo's specs with all outbound and inbound links. An unknown/invalid repo id returns an explicit error (400/404), not an empty graph. (B) Per controller review feedback (58102d3a27f920beeb3fba71:exempt-query-endpoints-no-authorization): four of the seven specced endpoints — GET /api/v1/specs/stale-links, GET /api/v1/specs/conflicts, GET /api/v1/specs/{path}/dependents, GET /api/v1/specs/{path}/dependencies — are listed in the frozen ABAC route-registry exemption file (scripts/abac-route-registry-exemptions.txt lines 37-41) and their handlers (get_stale_links:1841, get_conflicts:1858, get_spec_dependents:1776, get_spec_dependencies:1809 in api/specs.rs) take only State/Path with no auth extractor and no scope comparison, so any authenticated principal in any tenant receives the forge-wide links store (cross-tenant spec-graph data) — the same hazard class as the patrol endpoint, violating the repo invariant that every registry-exempt route MUST enforce per-handler authorization (check-abac-exempt-handlers.sh passes only because its Rule 1 skips handlers that name no auth extractor). Bring all four under middleware ABAC: register each as a RouteResourceMapping::api entry in abac_middleware.rs (mirroring /api/v1/specs/:path/links at :430 and /api/v1/specs/graph at :425, both already mapped, resource "spec") and REMOVE the four routes from scripts/abac-route-registry-exemptions.txt — the file must shrink; no new exemptions, no duplicate resolver entries (the registry is frozen at baseline counts). Do NOT add a resolver entry for /api/v1/specs/:path/reject (also in the exemption file) — it is outside this section's scope and adding an unused mapping is out of contract. If any of the four cannot be resolver-mapped for a structural reason discovered during implementation, the fallback is per-handler scope enforcement in the handler body (load the scoped entity, compare tenant/workspace against the caller, return Forbidden on mismatch) — never leaving it exempt with no authorization decision.

## Acceptance Criteria

- [ ] GET /api/v1/specs/graph (no parameter) returns the same full graph as before.
- [ ] GET /api/v1/specs/graph?repo={id} returns only that repo's spec nodes plus every link touching them, including inbound links whose source spec lives in another repo.
- [ ] An invalid repo id yields an explicit error response rather than an empty or unfiltered graph.
- [ ] The four query endpoints (stale-links, conflicts, dependents, dependencies) are removed from scripts/abac-route-registry-exemptions.txt and have RouteResourceMapping resolver entries; scripts/check-abac-route-registry.sh and scripts/check-abac-exempt-handlers.sh pass with a strictly smaller exemption file.
- [ ] A principal authenticated in tenant B calling GET /api/v1/specs/stale-links, /specs/conflicts, /specs/{path}/dependents, or /specs/{path}/dependencies does not receive tenant A's spec-link data (policy-denied or scoped to tenant B), where on current code the call succeeds and returns the forge-wide store.

## Verification intent

Repo filter: in a tenant with two repos where repo 1's spec has an inbound depends_on link from repo 2's spec, call GET /api/v1/specs/graph?repo={repo1_id}: assert no node with a different repo_id appears and the repo 2 inbound edge is present. On current code the ?repo parameter is ignored entirely (the handler has no Query extractor), so the response is identical to the unfiltered graph and both assertions fail — proving the filter actually constrains the query rather than the endpoint merely accepting the parameter. Authorization: as a principal scoped to tenant B (with tenant A holding stale/conflicted/linked specs), call the four query endpoints: on current code each call succeeds and returns tenant A's cross-tenant link rows (the handlers take no auth extractor and read the forge-wide store); after the fix the calls are policy-denied or return only tenant B's data, and the exemption file's line count has strictly decreased.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/spec-links.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `88a83dc373d68c28920f79ff68d005c2766889f423ca9c64be0118f867efc3b7`.

[
  {
    "id": "58102d3a27f920beeb3fba71",
    "decision": "gap",
    "requirement_quote": "| `GET /api/v1/specs/graph?repo={id}` | GET | Specs for one repo with all outbound/inbound links |",
    "detail": "Six of the seven specced endpoints exist and query the real links store and spec ledger: GET /api/v1/specs/graph (full graph), /specs/{path}/links (both outbound and inbound), /specs/{path}/dependents and /dependencies (impact analysis over depends_on/implements), /specs/stale-links, and /specs/conflicts (both-approved conflicts_with links). Two defects keep the section open. (1) The repo-filtered graph variant is not implemented: get_spec_graph takes no query parameter and always returns every ledger node and every store edge; no handler anywhere consumes a repo filter for the spec graph, so \"specs for one repo with all outbound/inbound links\" cannot be queried. (2) Four of the endpoints — GET /api/v1/specs/stale-links, GET /api/v1/specs/conflicts, GET /api/v1/specs/{path}/dependents, GET /api/v1/specs/{path}/dependencies — are listed in the frozen ABAC route-registry exemption file and their handlers (get_stale_links, get_conflicts, get_spec_dependents, get_spec_dependencies) perform no per-handler authorization at all: the signatures take only State/Path with no auth extractor and no scope comparison, so any authenticated principal in any tenant receives the forge-wide links store (cross-tenant spec-graph data). This violates the repo invariant that every registry-exempt route MUST enforce per-handler authorization; check-abac-exempt-handlers.sh passes only because its Rule 1 skips handlers that name no auth extractor (\"no auth identity at all is a different check's job\"). By contrast /api/v1/specs/graph and /api/v1/specs/:path/links are properly resolver-mapped in abac_middleware.rs and receive middleware policy evaluation.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "quote": "        .route(\"/api/v1/specs/graph\", get(specs::get_spec_graph))",
        "detail": "Only one spec-graph route is registered; there is no repo-scoped variant."
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "pub async fn get_spec_graph(State(state): State<Arc<AppState>>) -> Json<SpecGraphResponse> {",
        "detail": "The handler signature accepts only State -- no Query extractor, no repo parameter. The ?repo={id} filter of the spec's endpoint table is unimplemented."
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "        .iter()\n        .filter(|l| l.source_path == spec_path || l.target_path == spec_path)",
        "detail": "By contrast the per-spec links endpoint does return both directions (source_path or target_path match), confirming the six implemented endpoints query real state."
      },
      {
        "file": "scripts/abac-route-registry-exemptions.txt",
        "quote": "/api/v1/specs/:path/dependencies\n/api/v1/specs/:path/dependents",
        "detail": "The dependents and dependencies endpoints are in the frozen ABAC exemption file, so they run without middleware policy evaluation; per the repo invariant an exempt handler must then enforce per-handler authorization, which these handlers do not."
      },
      {
        "file": "scripts/abac-route-registry-exemptions.txt",
        "quote": "/api/v1/specs/conflicts\n/api/v1/specs/stale-links",
        "detail": "The conflicts and stale-links endpoints are likewise registry-exempt with no per-handler authorization, so any authenticated principal in any tenant receives the forge-wide (cross-tenant) links store."
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "quote": "pub async fn get_stale_links(State(state): State<Arc<AppState>>) -> Json<Vec<SpecLinkResponse>> {",
        "detail": "Handler signature: only State, no auth extractor and no scope comparison — the response is the unfiltered links store. get_conflicts, get_spec_dependents and get_spec_dependencies have the same shape (State/Path only)."
      },
      {
        "file": "scripts/check-abac-exempt-handlers.sh",
        "quote": "continue  # no auth identity at all is a different check's job",
        "detail": "The mechanical check passes despite the missing authorization because Rule 1 skips any exempt handler whose signature names no auth parameter — handlers with no auth identity at all fall outside its scope, so the four query endpoints are never flagged."
      },
      {
        "file": "crates/gyre-server/src/abac_middleware.rs",
        "quote": "RouteResourceMapping::api(\"/api/v1/specs/graph\", \"spec\", None),",
        "detail": "By contrast /api/v1/specs/graph (and /api/v1/specs/:path/links) are resolver-mapped, proving the four exempt query endpoints could be brought under the same middleware policy evaluation."
      }
    ]
  }
]
