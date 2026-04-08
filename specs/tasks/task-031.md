# TASK-031: Progressive Drill-Down with Zoom Transitions

**Spec reference:** `explorer-canvas.md` §Progressive Drill-Down  
**Depends on:** None (builds on existing double-click drill-down in ExplorerCanvas)  
**Progress:** `complete`

## Spec Excerpt

From `explorer-canvas.md` §Progressive Drill-Down:

> The canvas supports hierarchical navigation following the knowledge graph's `Contains` edges:
>
> ```
> Level 0: All packages in the repo
>   double-click "greeting-service" package
> Level 1: Modules inside greeting-service
>   double-click "greeting" module
> Level 2: Types/functions inside greeting
>   double-click "GreetingService" type
> Level 3: Fields and methods of GreetingService
> ```
>
> **Breadcrumb bar** shows the drill-down path. Each segment is clickable to navigate back up.
>
> **Zoom transition**: When drilling down, the canvas smoothly zooms into the clicked node, fades out unrelated nodes, and renders the children in the same layout style. When navigating back up, the reverse transition plays.

## Current State

**Implemented.** All acceptance criteria met:
- Lines 4274-4303: `onDblClick()` triggers smooth zoom-into with ease-out cubic interpolation (lines 3816-3822)
- Lines 217-219: `drillFadeAlpha`/`drillFadeTarget` state vars drive fade animation
- Lines 2521-2529, 3851-3860: unrelated nodes fade to ~12% opacity during drill
- Lines 4461-4510: `navigateBreadcrumb()` animates reverse zoom-out
- Breadcrumb at lines 5385-5394 calls `navigateBreadcrumb()` on click, supports URL deep-linking (`#drill=name1/name2`)

## Implementation Plan

1. Add zoom transition animation to `ExplorerCanvas.svelte`:
   - On double-click: compute the bounding box of the clicked node
   - Animate `viewBox` to zoom into the clicked node (300ms ease-out CSS transition or requestAnimationFrame)
   - Simultaneously fade unrelated nodes to 0% opacity (150ms)
   - After zoom completes, swap the node set to children and reset viewBox to fit

2. Add reverse transition:
   - On breadcrumb click to navigate up: animate zoom-out from current viewBox to parent scope
   - Fade in parent-level nodes (150ms)
   - Swap node set and reset viewBox

3. Use CSS transitions on SVG `transform` and `opacity` for smooth animation. Fall back to FLIP animation pattern if CSS transitions are insufficient on SVG.

4. Ensure breadcrumb segments are clickable and trigger the reverse transition at the correct level (not just parent — clicking the root should zoom all the way out).

5. Add test for drill-down state management (node filtering, breadcrumb path updates).

## Acceptance Criteria

- [ ] Double-click on a node triggers a smooth zoom-into animation (300ms)
- [ ] Unrelated nodes fade out during the transition
- [ ] After transition, canvas shows only children of the clicked node
- [ ] Breadcrumb updates to show the full drill-down path
- [ ] Clicking any breadcrumb segment triggers a reverse zoom-out animation
- [ ] Reverse animation fades in parent-level nodes
- [ ] Animation does not cause layout thrashing or jank on graphs with <500 nodes
- [ ] Test verifies drill-down state management

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `web/src/lib/ExplorerCanvas.svelte` for the current double-click handler and node rendering
3. Read `web/src/components/Breadcrumb.svelte` for the current breadcrumb implementation
4. Implement using SVG `viewBox` animation and CSS opacity transitions
5. Test with both small (10 nodes) and medium (200 nodes) graphs
6. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

Implemented as part of the explorer-canvas branch prior to task decomposition.
