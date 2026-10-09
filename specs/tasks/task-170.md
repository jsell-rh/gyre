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
commits: ["5cf54bce14c1b459045b4477d98a893bdbda5212", "c76fa1fd697ad44ae0fc08e22b70e5768e5b5026", "b69e01002b21ef5d2b7fa446067aca61e3890d0b", "088316309803e92cd7cf96cac53239431c059185", "95801f2b4ebbe93543bfe7d31fce386ce665be42", "49614e41c1b313f0c981d93013dcf6ddbc26c636", "9aa19ef9c94c075ed9d6fd154b037855542ca27b", "3c41b41997c20414f1167d4e8da6746b6e55a875"]
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

- [x] `ViewSpec` TypeScript type defined with all four layers
- [x] Server-side Rust struct with serde + validation
- [x] Nesting depth limit enforced (side-by-side sub-views cannot contain side-by-side)
- [x] `flow` layout requires `trace_source` in data layer (400 if missing)
- [x] `spec_path` filter requires `repo_id` (400 if missing)
- [x] `repo_id` validated against workspace membership
- [x] Layout registry pattern implemented in frontend
- [x] Tests pass for validation edge cases

## Agent Instructions

Read `ui-layout.md` §4 thoroughly — it contains extensive detail on each layer, the flow layout particle rendering, composability rules, and LLM constraints. The TypeScript types must match the JSON schema examples in the spec exactly. The server validation must reject the same invalid cases both server-side and client-side (belt and suspenders). Check existing graph types in `web/src/lib/types/` and `crates/gyre-common/src/` for naming conventions.

## Shipped

**Grammar types + validation (both sides):**
- `web/src/lib/types/view-spec.ts` — `ViewSpec`/`DataLayer`/`LayoutType`/`EncodingLayer`/`HighlightLayer`/`SubViewSpec` typedefs matching ui-layout.md §4 JSON examples exactly (kebab-case layout names), `validateViewSpec` client-side mirror (name required, known layout, flow⇒trace_source, spec_path⇒repo_id, side-by-side requires left+right, max nesting depth 1, sub-view field whitelist, no field inheritance, orphan left/right rejected), `isViewSpec` guard. `ViewEvent` interface ships in `web/src/lib/viewEvents.js` (§10) with tests.
- `crates/gyre-common/src/view_spec.rs` — serde structs (`#[serde(rename_all = "kebab-case")]` enum, `deny_unknown_fields` on `SubViewSpec` enforcing the data/layout/encoding-only rule at parse time) + `validate_view_spec` with the same rules, incl. per-sub-view validation.
- `crates/gyre-server/src/api/explorer_views.rs` — `parse_and_validate` on POST/PUT `/workspaces/:id/explorer-views` (400 on any invalid case; ViewQuery/ViewSpec hybrid payloads rejected so neither grammar can smuggle unvalidated fields) and on the LLM output of `/generate` before the SSE `complete` event (invalid ⇒ `{view_spec: null, explanation, fallback list view}` per §2, never a 500 or raw forward). `validate_repo_ownership` checks `repo_id` at the top level AND inside each side-by-side sub-view against workspace membership (400/foreign-repo fallback). Routes + ABAC `RouteResourceMapping` entries present.

**Repair in this assignment (5cf54bce):** `registerLayout` did not extend the accepted layout-name set, so a spec using a newly registered layout failed `validateViewSpec` ("unknown layout") — the extensibility mechanism broke its own contract (§4 Extensibility; both files' docs already claimed the sync existed). Fixed: `LAYOUT_TYPES` is a mutable exported array, `registerLayoutName` adds names idempotently (existing names rejected — grammar closed for modification), `registerLayout` extends both in lockstep. Regression test added (registered layout validates; duplicate registration doesn't corrupt the set).

**Test evidence** (recorded in `/tmp/stage/review-evidence/task-170-test-summary.txt`):
- `cargo test -p gyre-common --lib view_spec` — 13 passed, 0 failed (both §4 spec examples parse+validate; every rejection case).
- `cargo test -p gyre-server --lib api::explorer_views` — 16 passed, 0 failed (400 on flow-without-trace_source, nested side-by-side, spec_path-without-repo_id, foreign repo_id incl. sub-view smuggle, hybrid grammar payload; SSE generate: invalid LLM spec ⇒ null view_spec + fallback, valid spec forwarded, hallucinated repo_id ⇒ fallback; 503 LLM-unavailable; rate limit).
- `vitest --pool=threads` (forks pool cannot start in this sandbox — worker IPC timeout, infra limitation, not a code defect): view-spec 17, MoldableViewListView 1, MoldableViewNodeTypeFilter 5, viewEvents 8, layout-engines 11 — 42 passed, 0 failed.

**Scope note:** the live Explorer render surface is the single-canvas ExplorerView/ExplorerCanvas per `explorer-implementation.md` (draft; task-065+) with the ViewQuery grammar (`view-query-grammar.md`, task-062+) superseding this grammar for rendering. This task delivers the §4 ViewSpec grammar layer (types, both-side validation, layout registry, 400 enforcement) it was scoped for; the registry-dispatched MoldableView surface is exercised through its test suite, and `parse_and_validate` deliberately accepts both grammars on storage endpoints with hybrid payloads rejected.
