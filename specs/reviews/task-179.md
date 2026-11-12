# Review: TASK-179 — Moldable Views (type, trait, endpoint, and spec detail views)

**Reviewer:** Independent review agent
**Date:** 2026-10-09
**Comparison base:** e402e1ed620c0407906528548621734d776d43a6
**Candidate:** 34446bad0a784bb2de46534a45e614b8e982c7c5
**Verdict:** approved

---

## Scope of the diff

`web/src/lib/moldable/` (new: `model.js` computation layer, `shared.css`,
`Moldable{Type,Trait,Endpoint,Spec}View.svelte`), `NodeDetailPanel.svelte`
(inline view markup extracted into the components; routing by `node_type`),
`ExplorerView.svelte` (wires `repoId`, `onAgentProvenance` → app-level agent
detail, `onEditSpec` → inline spec editor), new
`web/src/__tests__/MoldableViews.test.js` (23 tests), `docs/ui.md`,
`specs/tasks/task-179.md`. No Rust changes — correct: `GraphNode`/`GraphEdge`
(gyre-common/src/graph.rs) already serialize every field the views consume
(verified field-by-field against the struct).

## Independent evidence

All probes ran against the exact candidate with the locked toolchain
(`npm ci`, repo vitest). Logs under `/tmp/stage/review-evidence/`:
`focused-vitest.log`, `full-vitest.log`, `probe-model.mjs`,
`probe-dependson.mjs`, `probe-endpoint-shapes.mjs`, `attribution-check.log`.

- **Full frontend suite**: 111 files, 2229 passed / 41 skipped, exit 0.
- **Focused suites**: `MoldableViews.test.js` + `NodeDetailPanel.test.js`
  (unchanged, pre-existing) → 83/83, exit 0.
- **Production build** (`vite build`): exit 0.
- **Direct model probes** (node, importing the real `model.js`):
  - `resolveFieldType`: DependsOn edge resolves the exact extractor-emitted
    type node (primary); doc_comment fallback unwraps `Option<Id>` → the `Id`
    node; primitives (`String`) render plain non-clickable annotations.
  - `computeTraitDependents`: counts call sites into the trait's methods
    (2 calls from one dependent counted as 2), not just direct trait callers.
  - `computeSpecCompleteness`: soft-deleted governed nodes excluded from
    `realized`; pct = realized/total.
  - `moldableViewType` routing table matches the acceptance mapping and
    leaves function/module/span/edge on the generic view.
- **Extractor conventions verified against Rust source** (so the model's
  claims are not just test-fixture conventions):
  - RoutesTo edge metadata `{"path":…,"method":…}` and module→endpoint
    Contains edge with the same meta — rust_extractor.rs
    `extract_endpoints_from_text` (lines 474–507).
  - Field nodes: `doc_comment` = the type annotation, FieldOf field→parent,
    DependsOn field→type-node — rust_extractor.rs (~1040–1085),
    typescript_extractor.rs (~499–510), go_extractor.rs (~423–431),
    python_extractor.rs (~465–467). This is exactly what the round-1 repair
    (9699c18b0) keyed the field-type clickability on — the base rendered the
    FIELD NAME (`qualified_name.split('::').pop()`) as the type; the candidate
    now resolves the real type and the new test asserts the annotation is the
    TYPE, so it fails against the old logic.
- **Host wiring** (code-verified): spec-reference buttons create a synthetic
  `{node_type:'spec'}` node that `ExplorerView` sets as `detailNode` →
  `MoldableSpecView` renders inline in the same panel; agent references
  (`last_modified_by`) call `onAgentProvenance` → `openDetailPanel({type:'agent'})`
  → `goToEntityDetail('agent', …)` (full agent provenance); `onEditSpec` →
  `openSpecEditor`, which owns the Preview / Full-preview / Publish workflow.
  Spec content/history/assertions use the real ledger endpoints
  (`GET /specs/:path`, `…/history`, `GET /repos/:id/specs/:path/assertions`).
- **Task commit attribution** passes on the committed tree (the working-tree
  `task-179.md` reset to `not-started`/`commits: []` is the controller's
  contract checkout, not a candidate defect).

## Acceptance criteria

- [x] Type View shows fields, implements, used-by, story, risk metrics
- [x] Trait View shows methods, implementations, dependents with counts
- [x] Endpoint View shows request/response shapes and flow diagram
- [x] Spec View shows content alongside implementation realization
- [x] All type references are clickable (field types via DependsOn/annotation;
      implements/used-by/methods/handlers/flow steps navigate)
- [x] All spec references open spec inline
- [x] All agent references show provenance (clickable → agent detail)
- [x] Detail panel routes to correct Moldable View by entity type
      (tested per type, plus negative case for non-moldable types)
- [x] Tests pass (23 new tests import the real production model — no mirrored
      logic; the pre-existing mirrored-logic blocks in
      `NodeDetailPanel.test.js` date from the base, untouched)

The `[Preview] [Publish] [History]` row ships as Edit + History: Edit opens
the inline spec editor that already owns the progressive Preview/Publish
workflow (ExplorerView `runThoroughPreview`/`requestPublish`), History is
inline in the view. Documented as a deviation in the task's Shipped notes and
consistent with the spec header's amendment deferring ghost overlays to the
Editor Split preview workflow — accepted.

## Non-blocking observations (for the record; not task-contract defects)

1. **Persona row is unreachable today.** `MoldableTypeView` renders
   `{#if node.persona}`, but `GraphNode` has no `persona` field, so the row
   never renders from real graph data. The candidate documented this honestly
   ("persona row when the extractor recorded one — never fabricated") and
   persona is absent from the task's acceptance criteria and from the graph
   API the task scoped data to. A future enhancement could resolve it via
   `GET /agents/:id` (`persona_id`) using `last_modified_by`.
2. **Endpoint params fallback carried over verbatim from base.**
   `computeEndpointShapes`' last-resort params fallback parses
   `node.qualified_name || node.name` for `:param` patterns; with the rust
   extractor's `file::route_name` qualified names this fabricates a Params
   chip equal to the name segment on paramless routes (probe:
   `probe-endpoint-shapes.mjs`). Pre-existing behavior at the assignment
   base, moved into `model.js` unchanged — not a regression introduced by
   this candidate. Fix would parse `endpointMeta.path` instead.
3. `{#if node.created_by}` (Story section) is likewise dead — `GraphNode`
   has no `created_by`. Also pre-existing at base.

None of these are fakery of specced enforcement, none are regressions, and
none violate the task's acceptance criteria.
