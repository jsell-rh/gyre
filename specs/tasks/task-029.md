---
title: "Lens Toggle UI + Observable Placeholder"
spec_ref: "explorer-canvas.md §Lens Definitions, §Component Architecture"
depends_on: []
progress: complete
coverage_sections: []
commits: []
---

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

**Implemented.** All acceptance criteria met:
- `ExplorerCanvas.svelte` lines 5058-5060: three lens buttons (Structural/Evaluative/Observable)
- Observable button is disabled with tooltip: "Observable lens is disabled — pending production OpenTelemetry collector integration" (line 5060), styled with `opacity: 0.35`, `cursor: not-allowed`, lock icon
- Lens state controls `EvaluativeOverlay` visibility via `{#if lens === 'evaluative'}` (lines 5063-5068)
- Component tests at `ExplorerCanvas.test.js` lines 732-800 cover all lens toggle scenarios

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

Implemented as part of the explorer-canvas branch prior to task decomposition.
