---
title: "Moldable Views — type, trait, endpoint, and spec detail views"
spec_ref: "system-explorer.md §2"
depends_on: [task-178]
progress: ready-for-review
coverage_sections:
  - "system-explorer.md §2. Moldable Views"
commits: ["f3e7ed8af3d08409f971e4f5e562cbeae8e1d524"]
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

- [x] Type View shows fields, implements, used-by, story, risk metrics
- [x] Trait View shows methods, implementations, dependents with counts
- [x] Endpoint View shows request/response shapes and flow diagram
- [x] Spec View shows content alongside implementation realization
- [x] All type references are clickable (navigate to that type's view)
- [x] All spec references open spec inline
- [x] All agent references show provenance
- [x] Detail panel routes to correct Moldable View by entity type
- [x] Tests pass

## Agent Instructions

Read `system-explorer.md` §2 "Moldable Views" for the exact wireframes and data requirements for each view type. The data comes from the knowledge graph API — check `GET /repos/:id/graph` response format for node properties (spec_path, visibility, churn_count_30d, etc.). The existing `DetailPanel.svelte` is the entry point — entity type detection should route to the appropriate Moldable View component. The clickable references are critical: every type, spec, and agent reference must be a navigation link. Check the existing graph node types in `crates/gyre-domain/` for the Rust-side data model.

## Shipped

The four moldable views of system-explorer.md §2 now ship as dedicated
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
  Dependents with per-dependent call-site counts. Gap closed: dependents now
  include every caller of the trait's methods (previously only direct
  callers of the trait node itself, which the extractor rarely emits).
- `MoldableEndpointView.svelte` — route method + path now read from the
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

### Test evidence

Focused web probes only (locked `npm ci`; repo's locked vitest). Full logs
and exit codes retained in the controller's review-evidence directory
(batches 1–5, 566 tests, all passing):

- Batch 1 — `MoldableViews.test.js` (new, 22 tests: model unit tests on the
  real functions, per-view rendering with mock graph data, clickable
  reference navigation, agent-provenance callback, spec content/history/
  assertions against a mocked api, NodeDetailPanel routing by node type) +
  `NodeDetailPanel.test.js` (60, unchanged behavior after the extraction).
- Batches 2–5 — every ExplorerView/NodeDetailPanel-adjacent suite
  (NavigableArchitecture, Scope, WorkspaceGraph, DrillUrl, LensUrl,
  DefaultViews, responsive, ViewEvents, Shortcuts, LayoutRenderers,
  LivenessWiring, GeneratedViews, LargeGraph, AppShell, RepoMode,
  WorkspaceHome*, DetailPanel, ExplorerCanvas*, ExplorerChat, canvas-state):
  484 tests passing. No Svelte compile warnings from the new components.

No Rust changes: the knowledge-graph API (`GET /repos/:id/graph`) already
exposes every field the views consume (GraphNode/GraphEdge in
gyre-common/src/graph.rs). `web/dist` intentionally not rebuilt here —
generated assets are restored/rebuilt by the controller's integration
stage.
