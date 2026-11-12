# Review — task-179 (Moldable Views — type, trait, endpoint, and spec detail views)

Spec: `specs/system/system-explorer.md` §2 "Moldable Views".
Candidate: `8b124296a` (base `3b734b1af`, branch `pipeline/task-179/06a78770650c48f3adc8cad3cfc13d9b-1`).
Product commits under review: `f3e7ed8af` (feat), `9699c18b0`/`d655e87ae` (fix), `ee5a6e10e` (dist regen).
Verdict: **approved** — real implementation, real tests, clean base-merge union. Evidence: `/tmp/stage/review-evidence/task-179-evidence.md`.

## Round 1 (2026-11-13)

### What was reviewed

The diff is frontend-only: four new `Moldable*View.svelte` components, a shared computation layer `web/src/lib/moldable/model.js` (single implementation, imported by both the components and the tests), entity-type routing in `NodeDetailPanel.svelte`, agent-provenance/editor wiring in `ExplorerView.svelte`, and `MoldableViews.test.js` (23 tests). No Rust changes; no scripts touched.

### Verified working

- **Entity routing is real and tested.** `moldableViewType()` (model.js) maps type/table/component → Type View, interface/trait → Trait View, endpoint → Endpoint View, spec → Spec View; everything else falls to the retained generic view. `NodeDetailPanel.svelte:313-321` dispatches on it; routing tests assert the `data-moldable-view` attribute per node type and the generic fallback for functions.
- **Type View** renders spec linkage (clickable, opens the spec inline), last-modified agent + time (agent reference is a button that fires `onAgentProvenance`), fields with clickable type annotations, Implements, Used by (callers + incoming FieldOf, with crate attribution), Story (created/modified dates, commit SHAs, churn), and the risk footer (churn bar, coupling, spec coverage, tests, complexity). All sections covered by render tests with mock graph data.
- **Field-type resolution follows the real extractor conventions** — verified against the Rust source, not just the fixture: field type annotation lives in `doc_comment` (rust_extractor.rs:2056, "host field doc_comment should be the type"), and fields link to their type nodes via DependsOn edges (rust_extractor.rs:1073). Primary resolution is the DependsOn edge; fallback matches type nodes by the annotation's identifiers (wrappers/primitives skipped). Both paths have dedicated tests.
- **Trait View** shows crate, methods with signatures (extracted from doc_comment/qualified_name/metadata) and per-method call-site counts, implementations with crate attribution, and Dependents — every caller of the trait or any of its methods, sorted by call-site count. The dependents computation is tested at model level with hand-computed counts.
- **Endpoint View** reads the route method + path from RoutesTo edge metadata (`{"path":…,"method":…}` — the extractor's canonical location), falls back to doc_comment, renders handler file:line, request/response shapes (edge metadata → doc_comment → handler call targets named *Request/*Response), the handler call-flow diagram (depth-3 BFS from the routes_to target — the spec's auth → … → respond chain), gates, and test count (test nodes reaching the endpoint or its handler).
- **Spec View (inline)** fetches real content from the spec ledger (`GET /specs/:path`), renders it as markdown, and computes implementation realization from the graph: governed nodes (spec_path matches + governed_by edges), ✓ present / ✗ soft-deleted, completeness percentage, stored assertion results (`GET /repos/:id/specs/:path/assertions`), inline History (`GET /specs/:path/history`), and Edit → the existing inline spec editor which carries the Preview/Publish workflow. A production-shaped probe (synthetic spec-path node, as the spec-linkage buttons navigate; spec file nodes typed as Module with hash ids, exactly what the extractors emit) confirmed the ledger fetch fires with the right arguments and the realization list computes from real edges.
- **References are clickable end-to-end.** Type/field/method/flow-step/implementation/dependent links call `onNavigate` (detail-panel navigation); spec links open the inline Spec View; agent references open the app-level agent detail (provenance) via the `openDetailPanel` context wired in `ExplorerView.svelte`.
- **Data-model fidelity.** Every node property the views consume exists on `GraphNode` (gyre-common/src/graph.rs): spec_path, visibility, spec_confidence, last_modified_sha/by/at, created_sha/at, complexity, churn_count_30d, test_coverage, deleted_at, test_node. All three API client methods used by the Spec View exist and map to registered server routes.
- **Base-merge union is clean.** The merge of base `3b734b1af` (task-168) conflicted only in generated `web/dist` assets. Authored change sets of the two parents are disjoint: all 11 task-179 files are byte-identical through the merge, and all task-168 changes (Rust + RepoSettings/WorkspaceSettings/locales/api.js) are present. No conflict markers, no trailing whitespace.
- **Tests are not inflated.** The suite imports the production model functions directly (no mirrored logic) and renders the real components. Mutation probes (evidence file): breaking entity-type routing → 2 failures; breaking spec completeness (soft-deleted counted as realized) → 2 failures. Breaking the DependsOn resolution path alone passes only because the independently-tested annotation fallback also resolves the fixture — redundant real resolution, not test fakery.
- Full frontend suite after `npm ci`: **111 files, 2241 passed, 41 skipped, 0 failed** — the NodeDetailPanel refactor broke no other consumer.

### Non-blocking observations

1. `node.persona` in MoldableTypeView is guarded and can never render — `GraphNode` has no persona field. Honest omission (never fabricated); persona is not in the task's acceptance criteria. If the extractor ever records personas, the view picks it up.
2. The `relationships.governedBy` fallback in `navigateToSpec` passes a raw node Id as the spec identity. Current extractors emit governed_by only from module/package nodes (which route to the generic view), so the fallback is effectively unreachable for moldable-view nodes; if reached it degrades to "content not available" rather than resolving the target node. Cosmetic robustness nit.
3. When the Spec View is opened via the synthetic spec-path node (the production path from spec-linkage buttons), the spec file's own graph node appears in the governed/implementation list. This is faithful to the extractor data model (the spec node's `spec_path` points at itself) and inflates completeness by one always-realized element at most. Data-model quirk, not a view defect.
4. Coverage matrix row for system-explorer.md §2 remains `task-assigned` — consistent with repo convention (shipped task-168 rows are also still task-assigned pending a later audit flip); not the implementing worker's stage.
5. `web/dist/` state is generated output owned by the deterministic verifier (regenerated remotely after integration); no review action taken on it.

### Verifier notes

No Rust code changed; no server-side test additions are required beyond the standard matrix. The verifier should run the web build + bundle regeneration, the full `web` vitest suite, and the standing process gates (attribution, architecture lint unaffected).
