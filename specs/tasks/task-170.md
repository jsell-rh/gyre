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

**Contract-repair round (assignment 8dd76606):** the prior candidate `99259f16`
was rejected for a contract violation — it was cut from older `f38abb7e`, so
against assignment base `f4acb4eb` its tree deleted `specs/reviews/task-189.md`
and rewound `specs/tasks/task-189.md` (normative changes outside task-170's
scope). The merge `6f2339a6` (candidate + base) restored both; this round
verified them byte-identical to base and confirmed the only specs/ change
base→HEAD is task-170's own lifecycle fields (progress, commits, checked
criteria, appended Shipped sections). Requirements, plan, and acceptance
criteria unchanged from creation.

**Grammar types + validation, both sides (belt and suspenders):**
- `web/src/lib/types/view-spec.ts` — `ViewSpec`/`DataLayer`/`LayoutType`/
  `EncodingLayer`/`HighlightLayer`/`SubViewSpec` typedefs matching the
  ui-layout.md §4 JSON examples (kebab-case layout names); `validateViewSpec`
  client-side mirror (name required, known layout, flow⇒trace_source,
  spec_path⇒repo_id, side-by-side requires left+right, max nesting depth 1,
  sub-view field whitelist, no field inheritance, orphan left/right
  rejected); `isViewSpec` guard. `ViewEvent` interface per §10 ships in
  `web/src/lib/viewEvents.js` (dispatch/subscribe/DOM bridge + tests).
- `crates/gyre-common/src/view_spec.rs` — serde structs
  (`#[serde(rename_all = "kebab-case")]` layout enum; `deny_unknown_fields`
  on `SubViewSpec` enforcing data/layout/encoding-only at parse time) +
  `validate_view_spec`/`validate_sub_view` with the same rules.
- `crates/gyre-server/src/api/explorer_views.rs` — `parse_and_validate` on
  POST/PUT `/workspaces/:id/explorer-views` (400 on every invalid case;
  ViewQuery/ViewSpec hybrid payloads rejected so neither grammar can smuggle
  unvalidated fields); LLM-output validation on `/generate` before the SSE
  `complete` event per §2 (invalid ⇒ `{view_spec: null, explanation,
  fallback list view}`, never a raw 500 or unvalidated forward);
  `validate_repo_ownership` checking `repo_id` at the top level AND inside
  each side-by-side sub-view against workspace membership. Routes registered
  in `api/mod.rs` with ABAC `RouteResourceMapping` entries (zero exemption
  entries).
- `web/src/lib/layoutRegistry.js` — `registerLayout`/`getLayout`/
  `listLayouts`; `registerLayout` extends the grammar's accepted layout-name
  set in lockstep (`registerLayoutName`, duplicate registration is a no-op —
  closed for modification); `MoldableView.svelte` dispatches renderer
  components through the registry (§4 Extensibility).

**Test evidence (fresh runs this round at HEAD `6f2339a6`, recorded in
`/tmp/stage/review-evidence/task-170-round-8dd76606-verification.txt`):**
- `cargo test -p gyre-common --lib view_spec` — 13 passed, 0 failed.
- `cargo test -p gyre-server --lib api::explorer_views` — 16 passed, 0 failed
  (400 on flow-without-trace_source, nested side-by-side,
  spec_path-without-repo_id, foreign repo_id incl. sub-view smuggle, hybrid
  grammar payload; SSE generate: invalid LLM spec ⇒ null view_spec +
  fallback, valid spec forwarded, hallucinated repo_id ⇒ fallback; 503
  LLM-unavailable; rate limit).
- vitest `--pool=threads` (forks pool cannot start in this sandbox — TCP
  listener probe unsupported, `accept` errno 95 per
  `/tmp/stage/capabilities.json`; infra limitation, not a code defect):
  task-scoped files `view-spec.test.js` (17), `MoldableViewListView.test.js`
  (1), `viewEvents.test.js` (9) — 27/27 passed; MoldableView dispatch also
  covered by pre-existing `MoldableViewNodeTypeFilter.test.js` (5).
- `check-abac-route-registry.sh` — exit 0.
- `check-task-commit-attribution.sh` — exit 1, **pre-existing base defect**:
  pristine base `f4acb4eb` fails its own check (task-189's head commit
  missing from task-189's frontmatter; verified in a throwaway worktree at
  the base). Repairing requires editing task-189's contract — out of scope
  for this task. Task-170's own attribution is clean: all 8 listed SHAs are
  HEAD ancestors and no task-170-labeled commit is unlisted.

**Full-suite context:** full vitest run (60 files) shows 9 timeout failures
in `ExplorerCanvas.test.js` ghost-overlay tests — pre-existing on the
pristine base `f4acb4eb` (fails 1/132 in isolation at base; passes 132/132
in isolation at HEAD; no task-170 module in its import graph). Not a
task-170 regression; recorded for verification.

**Sandbox restriction recorded:** TCP-listener probes unsupported (`accept`
errno 95). HTTP enforcement is covered by in-process
`axum::Router::oneshot` tests exercising the full request/response path
minus TCP transport. Full workspace suites, Clippy, arch checks, and GitHub
CI not repeated this round — owned by verification/publication.
