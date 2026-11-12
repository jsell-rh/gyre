# Review — task-179 (Moldable Views — type, trait, endpoint, and spec detail views)

Spec: `specs/system/system-explorer.md` §2 "Moldable Views".
Commits under review (base `b4b6bae1` → candidate `d88ac24e`): `f3e7ed8af`
(implementation), `9699c18b0` (field-type resolution repair for the prior
round's finding), `624872246`/`d88ac24e6` (process).
Verdict: **approved** — no findings.

## Round on candidate d88ac24e (2026-11-12)

This round re-reviewed the full diff and verified the repair of the prior
round's single durable finding (field-type clickability).

### Prior finding — repaired and verified

The prior round found `findFieldTypeNode()` computed the field's type from
`field.qualified_name.split('::').pop()` — the FIELD name under every
extractor's convention — so field-type rows rendered the field name as the
type annotation and never produced a clickable link, and the shipped test
passed only against a fixture convention no extractor produces.

The repair (`9699c18b0`) replaces it with `resolveFieldType(field, nodes,
edges)`:

1. **Primary — the DependsOn edge**: every extractor emits a `DependsOn` edge
   from the field node to the exact type node when the type is in the graph
   (verified `rust_extractor.rs:1058-1076`; the fix also handles the
   go/typescript/python extractors' dot-separated conventions).
2. **Fallback — the doc_comment type annotation**: identifiers parsed from
   `doc_comment` (which every extractor uses to store the field's TYPE —
   verified `rust_extractor.rs` `make_node(..., Some(type_str))`), with
   wrappers (`Option`/`Vec`/`Result`/…) and language primitives skipped,
   matched against type/interface/trait nodes by name or qualified_name
   suffix (`::Name` / `.Name`).

`MoldableTypeView` renders the annotation from `doc_comment` (plain text when
no node resolves — primitives/external types) and navigates to the resolved
type node on click. The test fixtures now use the real extractor conventions
(`qualified_name: 'domain::User::status'`, `doc_comment: 'TaskStatus'`,
DependsOn edge; plus a no-edge `Option<Agent>` → Agent fallback case), and the
clickable-annotation assertion requires the TYPE (`': TaskStatus'`), not the
field name.

### Verification performed (evidence: `/tmp/stage/review-evidence/`)

- **Mutation probe**: with `resolveFieldType()` disabled (`return null`),
  both field-type tests FAIL (2 failed / 23, exit 1) — the tests kill the
  prior bug class and are not self-confirming. Tree restored clean afterwards
  (`git diff --quiet -- web/` verified).
- **Focused suites**: `MoldableViews.test.js` 23/23; with
  `NodeDetailPanel.test.js` + `ExplorerCanvasDrillDown.test.js` 86/86, exit 0.
- **Full frontend suite** (`npm test` after `npm ci`): 110 files,
  2204 passed | 41 skipped, exit 0.
- **Rust-side data contracts** (read-only source verification):
  - Field nodes: `qualified_name = Parent::field_name`, type annotation in
    `doc_comment`, FieldOf + DependsOn edges (`rust_extractor.rs:1030-1076`).
  - Endpoint routes: RoutesTo edge metadata `{"path":…,"method":…}` and the
    module→endpoint Contains edge carry the route (`rust_extractor.rs:459-501`);
    `computeEndpointMeta` reads both, doc_comment fallback next.
  - Wire format: `GET /repos/:id/graph` → `KnowledgeGraphResponse
    {nodes, edges}` with snake_case `node_type`/`edge_type`
    (`depends_on`, `field_of`, `routes_to`, `governed_by`, `calls`,
    `implements`, `contains`) and edge fields `source_id`/`target_id`/
    `edge_type`/`metadata` (`gyre-common/src/graph.rs`,
    `gyre-server/src/api/graph.rs`) — exactly what model.js consumes.

### Acceptance criteria — all verified

- **Type View** (fields, implements, used-by, story, risk metrics):
  `MoldableTypeView.svelte` renders all sections from real graph data;
  risk footer computes churn/coupling/spec/tests from node properties
  (`churn_count_30d`, `test_coverage`, …) and edge counts.
- **Trait View** (methods, implementations, dependents with counts):
  `MoldableTraitView.svelte` + `computeTraitDependents` (callers of the trait
  or its methods, each with its call-site count, sorted heaviest-first).
- **Endpoint View** (request/response shapes and flow diagram):
  `MoldableEndpointView.svelte` — route from edge metadata, handler
  file:line, shapes from doc_comment/metadata/handler call targets,
  depth-3 BFS flow from the handler, gates, test count.
- **Spec View inline** (content alongside realization):
  `MoldableSpecView.svelte` fetches real spec content
  (`GET /specs/:path`), stored assertion results
  (`GET /repos/:id/specs/:path/assertions`), history
  (`GET /specs/:path/history` — all routes registered in `api/mod.rs`);
  realization list with ✓/✗ (soft-deleted) and completeness %; Edit opens
  the inline spec editor carrying the Preview/Publish workflow; History
  toggles an inline version list.
- **All type references clickable**: field types via `resolveFieldType`
  (mutation-proven), Implements/Used-by/Methods/Flow-step rows all navigate.
- **Spec references open inline**: spec linkage buttons navigate to the spec
  entity; ExplorerView renders it in the same detail panel
  (`onNavigate={(n) => { detailNode = n; }}`).
- **Agent references show provenance**: `onAgentProvenance` wired through
  ExplorerView's `getContext('openDetailPanel')` (provided by App.svelte:627)
  → `goToEntityDetail('agent', id)` — the full agent provenance view.
- **Detail panel routes by entity type**: `NodeDetailPanel.svelte` routes via
  `moldableViewType(node.node_type)` (type/table/component→Type,
  interface/trait→Trait, endpoint→Endpoint, spec→Spec; generic fallback).
- **Tests pass**: full frontend suite green (2204 passed, exit 0).

No test inflation found: fixtures follow the real extractor conventions, the
model functions are imported (not mirrored) from the single implementation in
`lib/moldable/model.js`, and the mutation probe proves the field-type tests
fail when production behavior is disabled.

No out-of-scope edits: the diff touches only the moldable views, the detail
panel routing, ExplorerView wiring, their tests, docs/ui.md, and the task
file. Rust crates are untouched (frontend-only change; the graph extractors
and API it consumes are pre-existing).
