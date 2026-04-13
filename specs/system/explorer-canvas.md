# Explorer Canvas

> **Status: Draft.** Replaces the existing Graph and Flow tabs in MoldableView with a unified canvas implementing the three-lens architecture from `system-explorer.md` §3 and `vision.md` Principle 5 (Feedback Loop).

## Problem

The current implementation has two separate tabs:
- **Graph tab**: static SVG rendering of the knowledge graph (ExplorerCanvas)
- **Flow tab**: same SVG graph + a canvas particle overlay (FlowRenderer + FlowCanvas)

These are the same canvas rendered twice with different overlays. Splitting them into tabs means the human loses structural context when looking at flow data, and can't see timing data while examining structure. The spec says these should be one view with lens toggles — the topology is constant, the overlay changes.

Additionally:
- The Graph tab has no concept of causality (no call graph traversal)
- The Flow tab only shows OTLP trace data, not structural flow
- Neither supports the progressive drill-down (double-click → zoom into children) described in system-explorer.md §1
- Node positions are computed independently between the two tabs, causing misalignment

## Design

### One Canvas, Three Lenses

The Explorer canvas is a single interactive surface. The knowledge graph topology (nodes + edges) is always visible. A lens toggle in the toolbar controls what data is overlaid.

```
┌──────────────────────────────────────────────────────────────────┐
│ [Structural ▾]  [Evaluative]  [Observable]    ⌘K  [?]          │
│ ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ── ──│
│                                                                  │
│    ┌─────────┐        Calls         ┌─────────┐                 │
│    │ health_ │ ─────────────────▶   │ AppConf │                 │
│    │ check   │                      │  ig     │                 │
│    └─────────┘                      └─────────┘                 │
│         │                                ▲                       │
│         │ Calls                          │ Calls                 │
│         ▼                                │                       │
│    ┌──────────┐                     ┌─────────┐                 │
│    │ Greeting │ ────────────────▶   │  User   │                 │
│    │ Service  │      FieldOf        │         │                 │
│    └──────────┘                     └─────────┘                 │
│                                                                  │
│ [Breadcrumb: workspace > repo > module]          [zoom: 1.0x]   │
├──────────────────────────────────────────────────────────────────┤
│ ▶ Architecture Insights                                          │
└──────────────────────────────────────────────────────────────────┘
```

### Lens Definitions

#### Structural (default)

**Data source**: Knowledge graph nodes + edges (especially `Calls`, `RoutesTo`, `Contains`).

**What it shows**:
- Static topology: boundaries, interfaces, data shapes, dependencies
- Causal flow paths: follow `Calls`/`RoutesTo` edges from any starting node
- Spec linkage: nodes colored by spec coverage (green = governed, amber = suggested, red = no spec)
- Complexity/churn indicators: node size reflects complexity, border thickness reflects churn

**Interaction**:
- Click a node → detail panel (type view, trait view, endpoint view per system-explorer.md §2)
- Double-click a node → drill into children (follow `Contains` edges). Breadcrumb updates.
- Click an edge → shows the relationship detail (what calls what, where in code)
- Right-click → context menu: "View spec", "View provenance", "Trace from here"
- "Trace from here" on any node: highlights the subgraph reachable via outgoing `Calls`/`RoutesTo` edges, dims everything else. This is the §5 Flow Trace.

**No particles. No animation. Pure structure.**

#### Evaluative

**Data source**: OTLP trace spans from TraceCapture gates (`GET /merge-requests/:id/trace`).

**What it overlays on the structural view**:
- Particles animated along edges following actual execution paths from test runs
- Node heat: colored by duration (cool blue = fast, warm red = slow)
- Node badges: span count, error rate, mean duration
- Edge thickness: proportional to call frequency in traces
- Failed spans: red glow on nodes, error particles
- Playback controls: play/pause, scrubber, speed (0.25x–5x)

**Interaction**:
- Everything from Structural lens still works (click, double-click, drill-down)
- Click a particle → detail panel shows the span: operation name, duration, status, attributes, input/output summary
- Click a node → detail panel adds an "Evaluative" tab showing all spans that touched this node, sorted by duration
- Hover a node → tooltip shows timing stats (p50, p95, error rate)

**The structural topology is always visible underneath.** Evaluative data is overlaid, not replacing.

#### Observable (future — disabled until production telemetry integration)

**Data source**: Production OpenTelemetry collector (not yet built).

**What it would overlay**:
- Live traffic flow as particles (Vizceral-style)
- Node heat by error rate, latency, throughput
- Edge thickness by request volume
- Anomaly callouts: "latency spike on endpoint X", "error rate elevated on module Y"

**Grayed out in UI with label: "Requires production telemetry integration."**

### Progressive Drill-Down

The canvas supports hierarchical navigation following the knowledge graph's `Contains` edges:

```
Level 0: All packages in the repo
  double-click "greeting-service" package
Level 1: Modules inside greeting-service (lib, greeting)
  double-click "greeting" module
Level 2: Types/functions inside greeting (GreetingService, GreetingConfig, User, ApiResponse)
  double-click "GreetingService" type
Level 3: Fields and methods of GreetingService
  click "greet" method → detail panel with code, spec linkage, provenance
```

**Breadcrumb bar** shows the drill-down path: `e2e-repo > greeting-service > greeting > GreetingService`. Each segment is clickable to navigate back up.

**Zoom transition**: When drilling down, the canvas smoothly zooms into the clicked node, fades out unrelated nodes, and renders the children in the same layout style. When navigating back up, the reverse transition plays.

At every level, the lens toggle works the same way — structural shows static topology of that level, evaluative overlays trace data scoped to spans touching nodes at that level.

### Causal Flow Trace (Structural Lens Feature)

When the human selects "Trace from here" on any node (right-click or toolbar button), the canvas enters **trace mode**:

1. The selected node highlights as the starting point
2. All nodes reachable via outgoing `Calls`/`RoutesTo` edges are highlighted in sequence
3. Unreachable nodes dim to 20% opacity
4. The trace path is numbered: ① → ② → ③ showing execution order
5. Each step is annotated with its governing spec (if any)
6. The detail panel shows the full trace as a list, each step clickable

This is system-explorer.md §5 — "What happens when an agent pushes code?" answered visually.

**Data requirement**: The knowledge graph must contain `Calls` edges. This requires enhancing the code extractors (Rust, Python, TypeScript) to analyze function bodies and emit call relationships, not just declarations.

### Data Requirements Summary

| Lens | Data Source | API Endpoint | Status |
|---|---|---|---|
| Structural topology | Graph nodes + edges | `GET /repos/:id/graph` | Built |
| Structural causality | `Calls`/`RoutesTo` edges | Same endpoint | **Need call-graph extraction** |
| Evaluative traces | OTLP spans | `GET /merge-requests/:id/trace` | Built |
| Evaluative span detail | Span payloads | `GET /trace-spans/:id/payload` | Built |
| Observable telemetry | Production OTel | Not yet designed | Future |
| Drill-down children | `Contains` edges | `GET /repos/:id/graph` (filtered) | Built (edges exist) |
| Spec linkage | `GovernedBy` edges | Same endpoint | Built |

### Component Architecture

Replace the current `MoldableView` tabs (Graph, List, Timeline, Flow) with:

```
MoldableView
  ├── Explorer tab (merged Graph + Flow)
  │     ├── LensToggle (Structural / Evaluative / Observable)
  │     ├── ExplorerCanvas (SVG graph layer — always rendered)
  │     ├── EvaluativeOverlay (Canvas 2D particles — rendered when lens=evaluative)
  │     │     └── PlaybackControls (play/pause, scrubber, speed)
  │     ├── NodeBadge (SVG overlay — metrics per node)
  │     └── Breadcrumb (drill-down path)
  ├── List tab (unchanged)
  └── Timeline tab (unchanged)
```

`FlowRenderer` and `FlowCanvas` merge into `EvaluativeOverlay` — a component that only renders when the evaluative lens is active. It consumes node positions and viewBox from ExplorerCanvas (already wired via bindable props).

### Implementation Phases

**Phase 1: Converge tabs + lens toggle**
- Merge Graph and Flow into single Explorer tab
- Add lens toggle (Structural active, Evaluative available, Observable grayed out)
- Evaluative lens shows existing OTLP particle animation
- Remove the separate Flow tab

**Phase 2: Progressive drill-down**
- Double-click → filter to children via `Contains` edges
- Breadcrumb navigation
- Zoom transition animation

**Phase 3: Causal flow traces**
- Enhance extractors to emit `Calls`/`RoutesTo` edges
- "Trace from here" right-click action
- Numbered path visualization with spec annotations

**Phase 4: Evaluative heat map**
- Node coloring by duration/error rate from trace spans
- Edge thickness by call frequency
- Hover tooltips with timing stats (p50, p95)

## Relationship to Other Specs

**Implements**: system-explorer.md §1 (navigable architecture), §3 (three lenses), §5 (flow traces)

**Depends on**: realized-model.md (knowledge graph), human-system-interface.md (detail panel, interaction model)

**Supersedes**: The existing Graph tab (ExplorerCanvas used standalone) and Flow tab (FlowRenderer/FlowCanvas) in MoldableView.
