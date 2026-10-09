---
title: "View Specification Grammar — TypeScript types and server-side validation"
spec_ref: "ui-layout.md §4"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "ui-layout.md §4. View Specification Grammar"
  - "ui-layout.md §Structure"
  - "ui-layout.md §Data Layer"
  - "ui-layout.md §Layout Layer"
  - "ui-layout.md §Highlight Layer"
  - "ui-layout.md §Encoding Layer"
  - "ui-layout.md §Extensibility"
  - "ui-layout.md §LLM Constraints"
commits: ["b69e01002b21ef5d2b7fa446067aca61e3890d0b", "088316309803e92cd7cf96cac53239431c059185", "95801f2b4ebbe93543bfe7d31fce386ce665be42", "49614e41c1b313f0c981d93013dcf6ddbc26c636", "9aa19ef9c94c075ed9d6fd154b037855542ca27b", "3c41b41997c20414f1167d4e8da6746b6e55a875"]
---

## Spec Excerpt

The Explorer renders views from a declarative JSON specification (ui-layout.md §4). This grammar is the interface through which LLMs create visualizations and humans save/share views.

The view spec has four layers:
- **Data Layer**: `concept`, `node_types`, `edge_types`, `depth`, `filter`, `repo_id`, `trace_source`
- **Layout Layer**: `graph`, `hierarchical`, `layered`, `list`, `timeline`, `side-by-side`, `diff`, `flow`
- **Encoding Layer**: Maps data attributes to visual properties (color, size, border, opacity, label, group_by, edge_color, edge_style, particle_color, particle_speed, node_badge)
- **Highlight Layer**: `spec_path`, `node_ids`, `edge_types`

Extensibility: new layout types can be added via a layout registry. The grammar is open for new layouts but closed for modification.

LLM Constraints: LLM can only produce view specs within this grammar, read-only access to knowledge graph, no arbitrary code execution.

## Implementation Plan

1. **TypeScript types** (`web/src/lib/types/view-spec.ts`):
   - Define `ViewSpec`, `DataLayer`, `LayoutType`, `EncodingLayer`, `HighlightLayer` types
   - Define `SideBySideViewSpec` with left/right sub-view fields
   - Add type guards and validation functions (max nesting depth 1, no field inheritance in sub-views)
   - Define the `ViewEvent` interface for standardized interaction events

2. **Rust types** (`gyre-domain` or `gyre-common`):
   - Define `ViewSpec` serde struct matching the TypeScript types
   - Server-side validation: reject invalid specs (400) on `/explorer-views` CRUD and `/generate` endpoints
   - Validate `side-by-side` nesting depth, `trace_source` requirement for `flow` layout, `repo_id` requirement when `spec_path` is set
   - Validate `repo_id` belongs to workspace (prevent cross-workspace data leakage)

3. **Layout registry** (frontend):
   - Create a `layoutRegistry` map from layout name → renderer component
   - Allow registration of new layout types without modifying core Explorer

4. **Tests**:
   - Unit tests for TypeScript validation functions
   - Rust tests for server-side view spec validation
   - Edge cases: invalid layout, nesting depth > 1, missing trace_source for flow

## Acceptance Criteria

- [ ] `ViewSpec` TypeScript type defined with all four layers
- [ ] Server-side Rust struct with serde + validation
- [ ] Nesting depth limit enforced (side-by-side sub-views cannot contain side-by-side)
- [ ] `flow` layout requires `trace_source` in data layer (400 if missing)
- [ ] `spec_path` filter requires `repo_id` (400 if missing)
- [ ] `repo_id` validated against workspace membership
- [ ] Layout registry pattern implemented in frontend
- [ ] Tests pass for validation edge cases

## Agent Instructions

Read `ui-layout.md` §4 thoroughly — it contains extensive detail on each layer, the flow layout particle rendering, composability rules, and LLM constraints. The TypeScript types must match the JSON schema examples in the spec exactly. The server validation must reject the same invalid cases both server-side and client-side (belt and suspenders). Check existing graph types in `web/src/lib/types/` and `crates/gyre-common/src/` for naming conventions.

## Repair (this round)

Addressed the review finding (probe artifacts + smuggling bypass):

1. **Hybrid grammar bypass (root cause)** — `parse_and_validate` no longer
   parses ViewQuery-first with serde's unknown-field tolerance. It classifies
   the payload by top-level key sets (`VIEW_QUERY_FIELDS` / `VIEW_SPEC_FIELDS`,
   each verified to exactly match its struct's fields): mixed payloads 400
   ("view spec mixes ViewQuery and ViewSpec fields"), pure ViewQuery payloads
   validate as ViewQuery, everything else parses as ViewSpec (unknown fields
   400 via serde). The confirmed bypass — valid `scope` + nested
   `side-by-side` riding the ViewQuery branch to 201 — is closed.
2. **Same-class hole** — orphan `left`/`right` on a non-`side-by-side` layout
   is now rejected in both `validate_view_spec` (Rust) and `validateViewSpec`
   (TS mirror).
3. **Probe artifacts** — `crates/gyre-common/examples/probe_parse.rs`
   deleted; the `PROBE_` test converted to a proper regression test
   (`create_view_rejects_hybrid_viewquery_viewspec_payload`), which fails on
   the pre-repair code (201 instead of 400).

Verification: `cargo test -p gyre-server --lib explorer_views` (16 passed),
`cargo test -p gyre-common view_spec` (13 passed, incl. new
`orphan_sub_views_rejected_on_non_side_by_side_layout`), `vitest run
src/__tests__/view-spec.test.js` (16 passed, incl. new orphan left/right
rejection test).
