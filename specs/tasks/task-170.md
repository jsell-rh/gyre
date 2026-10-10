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
commits: ["9447554ca17555cc49bf1ffa92000fc933230588", "5cf54bce14c1b459045b4477d98a893bdbda5212", "c76fa1fd697ad44ae0fc08e22b70e5768e5b5026", "b69e01002b21ef5d2b7fa446067aca61e3890d0b", "088316309803e92cd7cf96cac53239431c059185", "95801f2b4ebbe93543bfe7d31fce386ce665be42", "49614e41c1b313f0c981d93013dcf6ddbc26c636", "9aa19ef9c94c075ed9d6fd154b037855542ca27b", "3c41b41997c20414f1167d4e8da6746b6e55a875"]
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

## Shipped

**Round be688572 (repair of contract finding 97c0fbed):** the prior
candidate flipped the assigned acceptance-criteria checkboxes to `[x]`,
which `scripts/dev-contract.py:requirement_parts` counts as a change to
the assigned requirements (checkboxes are prose, not an operational
section). That is the entire delta — frontmatter (`title`/`spec_ref`/
`depends_on`/`coverage_sections`) was unchanged and the implementation
tree was intact. This round restored the assigned contract: the task
file keeps its original `[ ]` checkboxes and carries status via
`progress:` plus this section (matching completed tasks task-189/
task-212, which also keep `[ ]`). Implementation files are byte-identical
to the published candidate `06e0665b` — verified `git diff 06e0665b HEAD
-- crates/ web/src/` is empty; no re-implementation needed. No task-215/
task-212/other specs regressions: the only specs/ delta vs assignment
base `19d65446` is this file's own lifecycle fields.

**Implementation (ui-layout.md §4, all four grammar layers):**
- `web/src/lib/types/view-spec.ts` — `ViewSpec`/`DataLayer`/`LayoutType`/
  `EncodingLayer`/`HighlightLayer`/`SubViewSpec` typedefs matching the §4
  JSON examples (kebab-case layout names, all eight layouts);
  `validateViewSpec` client-side mirror (name required, known layout,
  flow⇒trace_source, spec_path⇒repo_id, side-by-side requires left+right,
  max nesting depth 1, sub-view field whitelist, no field inheritance,
  orphan left/right rejected); `isViewSpec` guard; `registerLayoutName`
  extension point keeping client validation in sync with the registry.
  `ViewEvent` per §10 ships in `web/src/lib/viewEvents.js`.
- `crates/gyre-common/src/view_spec.rs` — serde structs
  (`#[serde(rename_all = "kebab-case")]` layout enum;
  `deny_unknown_fields` on `SubViewSpec` enforcing data/layout/encoding
  only at parse time) + `validate_view_spec`/`validate_sub_view`.
- `crates/gyre-server/src/api/explorer_views.rs` — `parse_and_validate`
  400s on POST/PUT `/workspaces/:id/explorer-views` (ViewQuery/ViewSpec
  hybrid payloads rejected so neither grammar smuggles unvalidated
  fields); `/generate` validates LLM output before the SSE `complete`
  event (invalid or foreign-repo spec ⇒ `view_spec: null` + fallback
  list view, never a 500 or unvalidated forward); `validate_repo_ownership`
  checks `repo_id` at the top level AND inside each side-by-side sub-view
  against workspace membership via the repos port. ABAC
  `RouteResourceMapping` entries for all three routes.
- `web/src/lib/layoutRegistry.js` — `registerLayout`/`getLayout`/
  `listLayouts`; duplicate registration no-ops (closed for modification)
  and extends the grammar's accepted names in lockstep;
  `MoldableView.svelte` drives tabs and renderer dispatch through it.

**Fresh verification this round at HEAD `120ca4db` (evidence in
`/tmp/stage/review-evidence/task-170-be688572-*.txt`):**
- `cargo test -p gyre-common --lib view_spec` — 13/13 (spec §4 example
  parses; kebab-case roundtrip; unknown layout rejected;
  flow-without-trace_source; spec_path-without-repo_id; nested
  side-by-side; top-level-only fields in sub-views; no parent repo_id
  inheritance; orphan left/right; flow sub-view; valid composition).
- `cargo test -p gyre-server --lib api::explorer_views` — 16/16 (cold
  build; in-process `oneshot` HTTP: 400 on flow-without-trace_source,
  nested side-by-side, spec_path-without-repo_id, foreign repo_id incl.
  sub-view smuggle, hybrid grammar payload; SSE generate: invalid LLM
  spec ⇒ null view_spec + fallback, valid spec forwarded, hallucinated
  repo_id ⇒ fallback; 503 LLM-unavailable; rate limit).
- vitest `--pool=threads` task-scoped — 27/27 (view-spec 17, viewEvents 9,
  MoldableViewListView 1; locked deps via `npm ci`).
- `check-abac-route-registry.sh` — exit 0; `check-task-commit-
  attribution.sh` — exit 0 (all 8 attributed commits reachable and
  recorded); `check-in-memory-state-stores.sh`, `check-scope-literal-
  defaults.sh`, `check-inert-enforcement.sh` — exit 0.
- Working tree after probes: only this file modified; `web/dist`
  byte-identical to HEAD.

**Sandbox restriction recorded:** TCP-listener probes unsupported
(`accept` errno 95, `/tmp/stage/capabilities.json`) — no live-listener
HTTP smoke possible here; enforcement is covered by the oneshot tests
above. Full suites, all-target Clippy, arch check, and GitHub CI are
owned by verification/publication per assignment scope.
