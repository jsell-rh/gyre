---
title: "Frontend ViewQuery TypeScript Types & Validation"
spec_ref: "view-query-grammar.md"
depends_on: []
progress: complete
review: specs/reviews/task-037.md
coverage_sections: []
commits: 
  - e9b13077
  - 35d45dac
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
