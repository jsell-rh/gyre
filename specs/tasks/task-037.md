---
title: "Frontend ViewQuery TypeScript Types & Validation"
spec_ref: "view-query-grammar.md"
depends_on: []
progress: complete
review: specs/reviews/task-037.md
coverage_sections: []
commits: ["8321e71a32c10bba653336d07628e347f4977751", "cbb9d3c202673a7f2a18108cc27456ea4ef5c64f"]
---

## Spec Excerpt

From `view-query-grammar.md` §Primitives:

> The view query grammar defines: Scope (all, focus, filter, test_gaps, diff, concept), Emphasis (highlight, dim_unmatched, tiered_colors, heat, badges), Edges (filter, exclude), Zoom (fit, current, level), Annotation (title, description with template variables), Groups, Callouts, Narrative steps, and Interactive Bindings ($clicked, $selected).

From `explorer-implementation.md` §Frontend Components (ExplorerCanvas props):

> ```typescript
> {
>   repoId: string;
>   nodes: GraphNode[];
>   edges: GraphEdge[];
>   activeQuery: ViewQuery | null;
>   filter: 'all' | 'endpoints' | 'types' | 'calls' | 'dependencies';
>   lens: 'structural' | 'evaluative' | 'observable';
> }
> ```

## Current State

- Rust types for ViewQuery, Scope, Emphasis, etc. are fully defined in `crates/gyre-common/src/view_query.rs`.
- Rust validation (`ViewQuery::validate()`) catches schema errors.
- The frontend receives ViewQuery JSON from the WebSocket and passes it to `ExplorerCanvas` as a raw object.
- No TypeScript type definitions exist for ViewQuery or its sub-types.
- No client-side validation — invalid queries could cause runtime errors in the renderer.

## Implementation Plan

1. Create `web/src/lib/types/view-query.ts`:
   - Define TypeScript interfaces mirroring the Rust types:
     - `ViewQuery`, `Scope`, `ScopeAll`, `ScopeFocus`, `ScopeFilter`, `ScopeTestGaps`, `ScopeDiff`, `ScopeConcept`
     - `Emphasis`, `HighlightConfig`, `HeatConfig`, `BadgeConfig`
     - `EdgeFilter`, `Zoom`, `ViewAnnotation`
     - `ViewGroup`, `ViewCallout`, `NarrativeStep`
   - Use discriminated unions for Scope (by `type` field)
   - Export all types

2. Create `web/src/lib/view-query-validator.ts`:
   - `validateViewQuery(query: unknown): { valid: boolean; errors: string[] }`
   - Check required fields, valid scope types, known edge types, depth limits
   - Mirror the checks from Rust's `ViewQuery::validate()` (not necessarily all of them — focus on the ones that would cause rendering errors)

3. Integrate validation in `ExplorerChat.svelte`:
   - When a `view_query` message arrives from the WebSocket, validate before applying
   - If invalid, show an error message in the chat (not a crash)
   - Log the validation errors for debugging

4. Integrate types in `ExplorerCanvas.svelte`:
   - Change `activeQuery` prop type from `any` to `ViewQuery | null`
   - Use the type information for type-safe rendering logic

5. Add unit tests for the validator (valid queries, invalid queries, edge cases).

## Acceptance Criteria

- [ ] TypeScript interfaces for ViewQuery and all sub-types exist in `web/src/lib/types/view-query.ts`
- [ ] Interfaces match the Rust type definitions (discriminated unions for Scope, optional fields, etc.)
- [ ] Validator function catches invalid scope types, missing required fields, depth limits
- [ ] ExplorerChat validates incoming view queries before applying them
- [ ] Invalid queries show an error message instead of crashing
- [ ] ExplorerCanvas `activeQuery` prop is typed as `ViewQuery | null`
- [ ] Unit tests for the validator

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-common/src/view_query.rs` for the authoritative Rust type definitions
3. Read `web/src/lib/ExplorerChat.svelte` for where view queries are received
4. Read `web/src/lib/ExplorerCanvas.svelte` for where view queries are consumed
5. Keep the TypeScript types as close to the Rust types as possible (same field names, same enum variants)
6. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `e9b13077` feat(web): add ViewQuery TypeScript types and client-side validation (TASK-037)
- `35d45dac` fix(web): remove redundant guard and finalize streaming text on error path (TASK-037)

## Shipped

ViewQuery TypeScript types and client-side validation for the explorer,
including the requirements-gap repair from review round 87facbcf
(seed_match mirroring, commit `8321e71a`), preserved through the contract
restoration of round 3d087d81.

### Types (`web/src/lib/types/view-query.ts`)

All ViewQuery sub-types mirror the Rust definitions in
`crates/gyre-common/src/view_query.rs`: `Scope` as a discriminated union on
`type` (`all`/`focus`/`filter`/`test_gaps`/`diff`/`concept`), `Emphasis`,
`Highlight`, `HeatConfig`, `BadgeConfig`, `EdgeFilter`, `Zoom`,
`ViewAnnotation`, `ViewGroup`, `ViewCallout`, `NarrativeStep`. `ScopeConcept`
declares `seed_match?: string` (`'exact'` default | `'substring'`), matching
the Rust field (`view_query.rs:65-66`) and the amended
`specs/system/view-query-grammar.md` concept-scope spec, so typed consumers
can express the field the server actually serializes.

### Validator (`web/src/lib/view-query-validator.js`)

`validateViewQuery(query)` mirrors the rendering-critical checks from Rust's
`ViewQuery::validate()`: required scope + known scope type, per-scope field
checks (focus node/depth/direction, concept seed_nodes/expand_depth/
expand_direction/seed_match/expand_edges, diff commits), edge types against
`KNOWN_EDGE_TYPES`, emphasis (dim_unmatched range, tiered_colors, heat
metric, highlight color, badge template length), zoom (names,
number-as-string trap, level range), callout/group colors, and narrative
order uniqueness. The concept case rejects `seed_match` values outside
`{exact, substring}`, mirroring `view_query.rs:346-351`, so the client no
longer accepts queries the Rust validator would reject.

### Integration

`ExplorerChat.svelte` validates every incoming `view_query` message before
applying it; invalid queries render an error message in the chat instead
of crashing, with errors logged for debugging. `ExplorerCanvas.svelte`
types `activeQuery` as `ViewQuery | null` and reads
`scope.seed_match ?? 'exact'` for concept expansion.

### Test evidence

`cd web && npx vitest run src/__tests__/view-query-validator.test.js` —
58 passed (53 original + 5 seed_match cases: valid `'exact'`, valid
`'substring'`, default-absent, invalid `'regex'`, case-variant `'Exact'`),
exit 0, re-run this round on the restored-contract checkout (log:
/tmp/stage/review-evidence/task-037-validator-tests.txt).
Mutation-verified this round: disabling the seed_match check in the
validator fails the 2 invalid-value tests (log:
/tmp/stage/review-evidence/task-037-mutation-check.txt, exit 1); the
source was restored byte-identical afterward (`git diff` clean).
