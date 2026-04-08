# TASK-029: Lens Toggle UI + Observable Placeholder

**Spec reference:** `explorer-canvas.md` §Lens Definitions, §Component Architecture  
**Depends on:** None  
**Progress:** `not-started`

## Spec Excerpt

From `explorer-canvas.md` §Lens Definitions:

> The Explorer canvas is a single interactive surface. The knowledge graph topology (nodes + edges) is always visible. A lens toggle in the toolbar controls what data is overlaid.
>
> - **Structural (default)**: Pure structure. No particles. No animation.
> - **Evaluative**: OTLP trace spans overlaid on the structural view. Particles animated along edges.
> - **Observable (future)**: Grayed out in UI with label: "Requires production telemetry integration."

From §Component Architecture:

> ```
> MoldableView
>   ├── Explorer tab (merged Graph + Flow)
>   │     ├── LensToggle (Structural / Evaluative / Observable)
>   │     ├── ExplorerCanvas (SVG graph layer — always rendered)
>   │     ├── EvaluativeOverlay (Canvas 2D particles — rendered when lens=evaluative)
>   │     │     └── PlaybackControls (play/pause, scrubber, speed)
>   │     ├── NodeBadge (SVG overlay — metrics per node)
>   │     └── Breadcrumb (drill-down path)
> ```

## Current State

- `ExplorerView.svelte` renders the canvas and chat but has no lens toggle UI.
- `EvaluativeOverlay.svelte` and `PlaybackControls.svelte` exist but are always-visible or hidden ad hoc — no toggle controls their visibility.
- The `lens` prop exists on `ExplorerCanvas` but no UI to switch it.
- Observable lens is not mentioned anywhere in the frontend.

## Implementation Plan

1. Create `LensToggle.svelte` component:
   - Three toggle buttons: Structural (default, active), Evaluative, Observable
   - Observable button is grayed out/disabled with tooltip: "Requires production telemetry integration"
   - Toggle emits `onLensChange` event with value `'structural' | 'evaluative' | 'observable'`

2. Add `LensToggle` to `ExplorerView.svelte` toolbar area (above canvas, per the ASCII art in the spec).

3. Wire lens state to control:
   - `EvaluativeOverlay` visibility: only render when `lens === 'evaluative'`
   - `PlaybackControls` visibility: only render when `lens === 'evaluative'`
   - Pass `lens` prop to `ExplorerCanvas` (already accepts it)

4. Add component test for `LensToggle`.

## Acceptance Criteria

- [ ] Lens toggle renders three options: Structural, Evaluative, Observable
- [ ] Structural is the default active lens
- [ ] Switching to Evaluative shows EvaluativeOverlay + PlaybackControls
- [ ] Switching to Structural hides particles and playback controls
- [ ] Observable is grayed out with "Requires production telemetry integration" tooltip
- [ ] Component test verifies toggle behavior

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `web/src/components/ExplorerView.svelte` for the current explorer layout
3. Read `web/src/components/EvaluativeOverlay.svelte` and `PlaybackControls.svelte` for existing overlay components
4. Create `LensToggle.svelte` in `web/src/components/`
5. Follow the existing component patterns (Svelte 5, $state, design system CSS)
6. Add a vitest component test
7. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

_(none yet)_
