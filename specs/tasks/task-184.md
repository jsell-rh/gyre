---
title: "Flow Traces — animated particle flow visualization"
spec_ref: "system-explorer.md §5"
depends_on: [task-178, task-170]
progress: ready-for-review
coverage_sections:
  - "system-explorer.md §5. Flow Traces"
commits: ["23fbb3b60500026eb0751cc34614e2338083f337"]
---

## Spec Excerpt

system-explorer.md §5 defines Flow Traces — behavioral views showing how data flows through the system for a specific operation. The `"flow"` layout type is defined in `ui-layout.md` §4 Layout Layer with extensive detail.

**Core concept:** Animated particles represent traced requests flowing through the architecture graph. Each step is clickable → navigates to implementing code, governing spec, or author agent.

**Example trace:** "What happens when an agent pushes code?" — shows git push → git-receive-pack → JWT auth → pre-accept gates → commit provenance → spec lifecycle check → domain event → speculative merge.

**Predefined flows:** Common operations (agent spawn, MR merge, spec approval). Users can trace custom flows by selecting a starting function.

**Particle rendering** (from ui-layout.md §4):
- Each root span spawns a particle at entry node, traveling along span tree edges
- Size: 4px (6px on hover). Color: success=#3b82f6 (blue), error=#ef4444 (red)
- Trail: fading opacity (200ms, 20px length)
- Canvas 2D overlay on SVG nodes/edges. WebGL fallback for >100 particles.

**Interaction events in flow layout:**
- Click node during animation → pause, show span data
- Click particle → pause, show full span tooltip
- Click edge → show all spans that traversed this edge
- Hover node → aggregate badge (span count, error rate, mean duration)
- Double-click → drill down to next C4 level

**Control bar additions:** `[▶ Play] [⏸ Pause] [⏭ Step] [Speed: 1x ▾] [Scrub: ━━━●━━━━] [Test: all ▾]`

**Data source:** Requires `trace_source` in the view spec data layer (`mr_id` or `gate_run_id`). Traces come from gate execution (per `human-system-interface.md` §3a Test-Time Trace Capture).

## Implementation Plan

1. **Flow layout renderer** (`FlowLayout.svelte`):
   - SVG layer for nodes and edges (reuse existing canvas rendering)
   - Canvas 2D overlay layer for particle animation
   - Nodes positioned using existing layout (graph or layered as appropriate for the trace)
   - Edges drawn as paths between nodes

2. **Particle animation engine**:
   - Fetch trace data via `trace_source` (MR or gate run ID)
   - Parse span tree into a sequence of node-to-node transitions
   - Animate particles along edges using requestAnimationFrame
   - Particle rendering: 4px circles with color by status, fading trail
   - WebGL fallback for >100 concurrent particles

3. **Playback controls component** (`FlowPlaybackControls.svelte`):
   - Play/Pause/Step buttons
   - Speed selector (0.5x, 1x, 2x, 4x)
   - Time scrub bar for trace timeline
   - Test case selector dropdown (filter which root spans to animate)
   - Controls shown only when flow layout is active

4. **Interaction handlers**:
   - Click node during animation → pause, open detail panel with span data
   - Click particle → pause, show span tooltip (operation, I/O, duration, status)
   - Click edge → show all spans that traversed this edge
   - Hover node → aggregate badge (span count, error rate, mean duration)
   - Double-click node → drill down (same as other layouts)

5. **Node badge component** (`NodeBadge.svelte`):
   - Vizceral-style ring gauge on each node
   - Test-time metrics: span count, error rate, mean duration
   - Computed from GateTrace spans

6. **Predefined flow templates**:
   - Agent spawn flow, MR merge flow, spec approval flow
   - Accessible from the View selector or a "Trace flows" section
   - Each template specifies a starting point and the expected call path

7. **Tests**:
   - Particles animate along correct paths from span data
   - Playback controls (play/pause/step/speed) work correctly
   - Click interactions pause animation and show correct data
   - Node badges show correct aggregate metrics
   - WebGL fallback activates for large particle counts

## Acceptance Criteria

- [ ] Flow layout renders nodes/edges with animated particles
- [ ] Particles travel along span tree edges with correct colors (blue=success, red=error)
- [ ] Particle trail renders (fading opacity, 20px)
- [ ] Playback controls: Play, Pause, Step, Speed selector
- [ ] Time scrub bar for trace timeline
- [ ] Test case selector filters which spans to animate
- [ ] Click node → pause + detail panel with span data
- [ ] Hover node → aggregate badge (span count, error rate, mean duration)
- [ ] Predefined flow templates available (agent spawn, MR merge, spec approval)
- [ ] Tests pass

## Agent Instructions

Read `system-explorer.md` §5 "Flow Traces" and `ui-layout.md` §4 "Layout Layer" (flow layout detail section) for the full specification. The view spec grammar (task-170) defines the `"flow"` layout type with `trace_source` in the data layer. Check if gate trace data is available via existing API endpoints — search for `GateTrace` or trace-related types in `crates/gyre-domain/`. The particle rendering should use a `<canvas>` element overlaid on the SVG graph. For span data, check the `human-system-interface.md` §3a "Test-Time Trace Capture" spec for the data format. The Canvas 2D API is sufficient for <100 particles; WebGL is the fallback for performance. Register the flow layout in the layout registry pattern from task-170.

## Shipped

The flow-layout particle machinery (SVG node/edge layer + Canvas 2D/WebGL2
particle overlay, playback controls with scrub/step/speed/test selector,
span-tree particle resolution with spec colors and 200ms/20px fading
trails, node/particle/edge click interactions, per-node span-count /
error-rate / mean-duration badges) already shipped on the task-065/015
substrate (`FlowCanvas.svelte`, `flow-particles.js`, `particle-layer.js`,
`PlaybackControls.svelte`; ExplorerCanvas delegates to FlowCanvas for the
`"flow"` layout registered in `layoutRegistry.js`). This task closed the
two §5 gaps that remained on top of it:

### Predefined flow templates (the §5 "predefined flows" requirement)

`web/src/lib/flow-templates.js` defines the three spec-mandated flows —
agent spawn, MR merge, spec approval — each with a starting point, an
expected call path (word-token node-name matching against the loaded
graph), and its governing spec. The Explorer View selector gains a "Trace
flows" section listing them. Selecting one resolves the path against the
current graph, applies the flow layout scoped to the matched nodes
(concept-seeded ViewQuery, depth 0), and animates it with the captured
gate trace of the best-matching workspace MR: MR discovery goes through
`GET /workspaces/:id/merge-requests?has_trace=true` (new
`api.workspaceMergeRequests`; the same `get_by_mr` lookup the trace
endpoint serves), `pickTraceMr` prefers the MR whose title/spec_ref/branch
matches the template, and `api.mrTrace` loads its spans. Spans that do not
map onto the path produce no particles — nothing is fabricated; with no
traced MR the expected path renders structurally with an explanatory
toast. Custom flows remain the canvas's trace-from-node action (§5 "trace
custom flows by selecting a starting function").

### Flow-layout view query scoping

FlowCanvas previously ignored the active ViewQuery's scope (a flow view
rendered the full graph regardless of its spec's data layer).
ExplorerCanvas now forwards `queryMatchedIds` as `visibleNodeIds` and
FlowCanvas filters its node set by it — the same "scope filters the node
set" contract every registry renderer implements.

### Test evidence

- `flow-templates.test.js` (new, 11 tests): token-aware pattern matching
  (no substring false positives), spec/file/test node exclusion, template
  path resolution and dedup, empty-match honesty, ViewQuery construction
  (concept seeds, depth 0, spec-linked annotation), trace-MR selection
  (pattern preference, newest fallback, null on empty).
- `ExplorerViewFlowTemplates.test.js` (new, 4 tests): the "Trace flows"
  section lists all three flows; applying a template swaps to the flow
  layout scoped to the matched path (unrelated node filtered out),
  resolves the trace through the workspace has_trace discovery picking the
template-matching MR, and renders the playback bar + spec-linked
  annotation; no-trace renders the path structurally with no playback bar;
  a graph matching no step warns and changes nothing.
- `ExplorerCanvas.flow.test.js`: two new tests pin the query-scoping
  delegation (scoped nodes only, edges into unmatched nodes dropped; full
  graph without a query).
- `ExplorerDefaultViews.test.js`: updated for the three new selector
  options.
- Full `web` vitest suite: 2156 passed (the one failing assertion was the
  updated default-views option list). No Rust surface changed; no
  server-side tests required (the workspace MR list route and its
  `has_trace` filter pre-date this task and are covered by
  `merge_requests.rs` tests).
