---
title: "Moldable Views — type, trait, endpoint, and spec detail views"
spec_ref: "system-explorer.md §2"
depends_on: [task-178]
progress: not-started
coverage_sections:
  - "system-explorer.md §2. Moldable Views"
commits: []
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
