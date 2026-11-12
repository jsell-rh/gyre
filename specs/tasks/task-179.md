---
title: "Moldable Views — type, trait, endpoint, and spec detail views"
spec_ref: "system-explorer.md §2"
depends_on: [task-178]
progress: ready-for-review
coverage_sections:
  - "system-explorer.md §2. Moldable Views"
commits: ["f3e7ed8af3d08409f971e4f5e562cbeae8e1d524", "9699c18b08d0fac4a246e94929e8c074b4e0a790"]
---

## Spec Excerpt

system-explorer.md §2 defines moldable views — tailored detail views for each entity type. These are NOT generic detail panels; they surface the information that matters for each kind of thing.

**Type View** (struct/enum): Spec linkage, last modified (agent + time), persona version. Fields list with clickable types (navigate to type). Implements/Used by sections. "Story" section showing creation history across milestones. Risk metrics: churn, coupling, spec coverage, test coverage.

**Trait View**: Spec linkage, crate location. Methods list. Implementations list (all impls with crate). Dependents list (who uses this trait, with call site counts).

**Endpoint View**: Spec linkage, handler location (file:line). Request/response shapes. Flow diagram (auth → ABAC → budget → create → respond). Gates and test count.

**Spec View (Inline)**: Spec content (editable) alongside implementation realization. Shows which code elements implement this spec with completeness percentage. [Preview] [Publish] [History] actions.

Every field type is clickable → navigates to that type. Every spec reference → opens spec inline. Every agent reference → shows provenance.

## Implementation Plan

1. **Type View component** (`MoldableTypeView.svelte`):
   - Header: name, visibility, spec linkage (clickable), last modified agent + time, persona
   - Fields section with clickable type links
   - Implements section (which traits this type implements)
   - Used by section (call sites grouped by crate/module)
   - Story section (creation/modification history from knowledge graph timeline)
   - Risk footer: churn/coupling/spec_coverage/tests metrics

2. **Trait View component** (`MoldableTraitView.svelte`):
   - Header: name, visibility, spec linkage, crate
   - Methods list with signatures
   - Implementations list with crate attribution
   - Dependents list with call site counts

3. **Endpoint View component** (`MoldableEndpointView.svelte`):
   - Header: method + path, role requirement, spec linkage, handler location
   - Request/response body shapes
   - Flow diagram (auth → ABAC → business logic → respond)
   - Gates and test coverage

4. **Spec View (inline) component** (`MoldableSpecView.svelte`):
   - Spec content (markdown viewer/editor)
   - Implementation realization: list of code elements with ✓/✗ status
   - Completeness percentage
   - Preview/Publish/History actions

5. **Entity routing in detail panel**:
   - Detect entity type from graph node data
   - Route to appropriate Moldable View component
   - Make all references clickable (type → navigate, spec → inline, agent → provenance)

6. **Tests**:
   - Each moldable view renders correctly with mock data
   - Clickable references navigate correctly
   - Story section shows timeline data

## Acceptance Criteria

- [ ] Type View shows fields, implements, used-by, story, risk metrics
- [ ] Trait View shows methods, implementations, dependents with counts
- [ ] Endpoint View shows request/response shapes and flow diagram
- [ ] Spec View shows content alongside implementation realization
- [ ] All type references are clickable (navigate to that type's view)
- [ ] All spec references open spec inline
- [ ] All agent references show provenance
- [ ] Detail panel routes to correct Moldable View by entity type
- [ ] Tests pass

## Agent Instructions

Read `system-explorer.md` §2 "Moldable Views" for the exact wireframes and data requirements for each view type. The data comes from the knowledge graph API — check `GET /repos/:id/graph` response format for node properties (spec_path, visibility, churn_count_30d, etc.). The existing `DetailPanel.svelte` is the entry point — entity type detection should route to the appropriate Moldable View component. The clickable references are critical: every type, spec, and agent reference must be a navigation link. Check the existing graph node types in `crates/gyre-domain/` for the Rust-side data model.

## Shipped

The four moldable views of system-explorer.md §2 ship as dedicated
components routed by entity type, with one shared computation layer and
tests that exercise the real implementation.

### What shipped

- `web/src/lib/moldable/model.js` — the single computation layer (node-type
  routing table, relationship graph, risk, story, method signatures,
  endpoint route/flow/shapes/gates/test counts, spec governed-nodes and
  completeness). Extracted from NodeDetailPanel.svelte; the new tests import
  these functions directly instead of mirroring logic into the test file.
- `MoldableTypeView.svelte` — spec linkage (clickable → opens the spec
  inline), last-modified agent + time, persona row when the extractor
  recorded one (never fabricated), fields with clickable type annotations,
  Implements, Used by with crate attribution, Story (creation history,
  churn, commit SHAs), and the risk footer (churn / coupling / spec
  coverage / tests).
- `MoldableTraitView.svelte` — crate, methods with signatures and
  per-method call-site counts, implementations with crate attribution, and
  Dependents with per-dependent call-site counts. Dependents include every
  caller of the trait's methods plus direct callers of the trait node.
- `MoldableEndpointView.svelte` — route method + path read from the
  RoutesTo edge metadata (`{"path":…,"method":…}` — the extractor's
  canonical location per rust_extractor.rs; doc_comment is the fallback),
  handler file:line, request/response body shapes, the handler call-flow
  diagram (depth-3 BFS), gates from metadata/doc-comment, and test count.
- `MoldableSpecView.svelte` — spec content fetched from the spec ledger
  (`GET /specs/:path`) and rendered as markdown alongside the
  implementation realization: governed nodes with ✓ (present in the current
  extraction) / ✗ (soft-deleted), completeness percentage, stored assertion
  results (`GET /repos/:id/specs/:path/assertions`), plus Edit (opens the
  inline spec editor, which carries the progressive Preview / Publish
  workflow) and inline History (`GET /specs/:path/history`) actions.
- `NodeDetailPanel.svelte` routes `node_type` → the moldable view component
  (type/table/component → Type; interface/trait → Trait; endpoint →
  Endpoint; spec → Spec; everything else keeps the generic view) and keeps
  the edge/span/evaluative/shared sections. All clickable references are
  wired: type refs navigate to the type's view, spec refs open the spec
  inline in the same panel, and agent refs (`last_modified_by`) open the
  agent's provenance via the app-level detail panel (ExplorerView wires
  `openDetailPanel({type:'agent'})`).
- `docs/ui.md` documents the views and components.

Deviation note: the Spec View action row ships Edit + History rather than
literal Preview/Publish buttons — the inline spec editor (opened by Edit)
already owns the progressive Preview / Full preview / Publish workflow, and
duplicating it inside the read-oriented spec view would fork that flow.
Progressive preview itself is §3 (task-186) territory.

### Field-type resolution repair (commit 9699c18b0)

The first review round found the field-type clickability was computed from
`field.qualified_name.split('::').pop()` — the FIELD NAME under the real
extractor convention, so the row rendered `status : status` and produced
no clickable link. Fixed by replacing `findFieldTypeNode()` with
`resolveFieldType(field, nodes, edges)` in model.js:

1. Primary source: the DependsOn edge every extractor emits from the field
   node to the exact type node (rust_extractor.rs, go_extractor.rs,
   typescript_extractor.rs, python_extractor.rs).
2. Fallback: identifiers parsed from the doc_comment type annotation
   (Option/Vec/Result/… wrappers and language primitives skipped), matched
   against type/interface/trait nodes by name or qualified_name suffix
   (`::Name` / `.Name` — the python/typescript/go extractors use
   dot-separated qualified names).

`MoldableTypeView.svelte` now renders `: <annotation>` from doc_comment
(plain text when no node resolves — primitives/external types) and
navigates to the resolved type node on click. The test fixture was
rewritten to the real extractor convention (qualified_name
`domain::User::status`, doc_comment `TaskStatus`, DependsOn edge) and
asserts the annotation is the TYPE, so it fails against the old
field-name logic; a second test covers the no-edge doc_comment fallback
(`Option<Agent>` → the Agent type node).

### Test evidence

Focused web probes only (locked `npm ci`; repo's locked vitest).

- Feature round (f3e7ed8af, 566 tests passing, logs retained in the
  controller's review-evidence directory): `MoldableViews.test.js` (new —
  model unit tests on the real functions, per-view rendering with mock
  graph data, clickable reference navigation, agent-provenance callback,
  spec content/history/assertions against a mocked api, NodeDetailPanel
  routing by node type) plus `NodeDetailPanel.test.js` (60), and every
  ExplorerView/NodeDetailPanel-adjacent suite (484 more).
- Repair round (9699c18b0): `npx vitest run
  src/__tests__/MoldableViews.test.js src/__tests__/NodeDetailPanel.test.js`
  → 2 files, 83/83 tests passed, exit 0; `npx vitest run
  src/__tests__/ExplorerCanvasDrillDown.test.js` (real `Parent.field`
  fixture) → 3/3, exit 0. Direct probe of `resolveFieldType` against the
  reviewer's exact scenario (field `status`, qualified_name
  `domain::Task::status`, doc_comment `TaskStatus`, DependsOn edge) resolves
  TaskStatus via the edge and via the doc_comment fallback, renders
  `String` as non-clickable plain text, and unwraps `Option<Id>` to the Id
  type node (evidence: /tmp/stage/review-evidence/field-type-probe-output.txt).

No Rust changes: the knowledge-graph API (`GET /repos/:id/graph`) already
exposes every field the views consume (GraphNode/GraphEdge in
gyre-common/src/graph.rs). `web/dist` intentionally not rebuilt here —
generated assets are restored/rebuilt by the controller's integration
stage.
