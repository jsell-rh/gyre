---
title: "Publish reviewed spec backlog for specs/system/explorer-canvas.md"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: []
---

## Required behavior

Publish exactly the independently reviewed missing-work plan below. Only create the assigned task documents and add their exact section bindings. Preserve every existing task, spec, registry entry, coverage status, product file and gate. Task creation is not implementation. Re-read the canonical spec and challenge the audit, decomposition, dependencies and duplicate-work analysis before approving.

{
  "document": "specs/system/explorer-canvas.md",
  "plan_id": "1a0e6db276139ed1d4e01536577e86c9ae7716780322213d7aaef0c2e11d30d2",
  "audit": {
    "base": "678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce",
    "requirements": "b3cd47ade570702ae98b999a63952889fc6f3bb3e82411d8484be14b74723846",
    "sections": [
      {
        "id": "c898aece5ee9729dbfd81ff8",
        "decision": "context",
        "requirement_quote": "Replaces the existing Graph and Flow tabs in MoldableView with a unified canvas implementing the three-lens architecture from `system-explorer.md` §3 and `vision.md` Principle 5 (Feedback Loop).",
        "detail": "Root status note declaring the spec Draft and its purpose (supersede the split Graph/Flow tabs). It is rationale/status prose, not an implementable requirement; the implementable content lives in the Design children (One Canvas Three Lenses, Lens Definitions, Progressive Drill-Down, Causal Flow Trace, Component Architecture), each audited separately below. The declared replacement has in fact happened in production: no MoldableView tab component exists anymore (only the unrelated Moldable{Type,Trait,Endpoint,Spec}View detail views under web/src/lib/moldable/), the 'graph' and 'flow' ViewSpec layouts both resolve to the unified ExplorerCanvas component, and the live surface is ExplorerView rendered by RepoMode's architecture tab (web/src/components/RepoMode.svelte).",
        "evidence": [
          {
            "file": "web/src/lib/layoutRegistry.js",
            "quote": "// task-065 unified cutover: the separate FlowRenderer tab was merged",
            "detail": "The layout registry maps both the 'graph' and 'flow' ViewSpec layouts to the same ExplorerCanvas component; the comment records that the separate FlowRenderer tab was merged into ExplorerCanvas, i.e. the root's declared supersession of the split tabs is the shipped state."
          }
        ]
      },
      {
        "id": "3154a390cece235c8771ad44",
        "decision": "context",
        "requirement_quote": "The spec says these should be one view with lens toggles — the topology is constant, the overlay changes.",
        "detail": "The Problem section is diagnostic rationale about the pre-unification implementation (split Graph/Flow tabs, no causality, no drill-down, misaligned positions). It carries no independent requirement beyond what the Design sections mandate; every listed problem is addressed by audited Design children: the tabs are merged into one ExplorerCanvas with a lens toggle (One Canvas, Three Lenses), causal traversal exists (Causal Flow Trace), progressive drill-down exists (Progressive Drill-Down), and a single layout pipeline per ViewSpec removes the dual-position problem. No separate gap can be counted here without double-counting the children.",
        "evidence": [
          {
            "file": "web/src/lib/layoutRegistry.js",
            "quote": "// task-065 unified cutover: the separate FlowRenderer tab was merged",
            "detail": "Production registry comment confirming the premise of the Problem section (two separate tabs rendering the same canvas) no longer exists: flow/trace data renders through the unified canvas's evaluative lens and the 'flow' layout resolves to the same ExplorerCanvas component."
          }
        ]
      },
      {
        "id": "7ad702933bafd31b29308528",
        "decision": "context",
        "requirement_quote": "## Design",
        "detail": "The Design heading is a pure container with no body text of its own; every normative requirement under it is accounted for exactly once by its children (One Canvas Three Lenses, Lens Definitions + the three lens subsections, Progressive Drill-Down, Causal Flow Trace, Data Requirements Summary, Component Architecture, Implementation Phases), each audited below.",
        "evidence": []
      },
      {
        "id": "f5f5393be738bc4ed3c240ae",
        "decision": "satisfied",
        "requirement_quote": "The Explorer canvas is a single interactive surface. The knowledge graph topology (nodes + edges) is always visible. A lens toggle in the toolbar controls what data is overlaid.",
        "detail": "ExplorerCanvas.svelte is one interactive surface: the toolbar renders a lens checkbox group where Structural is the always-on locked base layer, Evaluative is an additive overlay checkbox that flips the lens state, and Observable is a disabled future toggle. The topology is always visible — evaluative data (particles, badges, heat) is computed and drawn only when lens === 'evaluative', layered on top of the same structural node/edge rendering (the structural fill, spec border and label remain visible underneath). Lens state is bindable and wired through the embedding ExplorerView/RepoMode surface.",
        "evidence": [
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "<div class=\"lens-group\" role=\"group\" aria-label=\"Lens selector\" data-testid=\"lens-checkbox-group\">",
            "detail": "The toolbar hosts the lens toggle group; the Structural checkbox is checked+disabled (always-on base), the Evaluative checkbox binds lens === 'evaluative' and toggles the lens state, satisfying 'a lens toggle in the toolbar controls what data is overlaid'."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "if (!traceData?.spans?.length || lens !== 'evaluative') return null;",
            "detail": "Evaluative overlay data (particles built from OTLP trace spans) is only produced when the evaluative lens is active — on the structural lens the same canvas keeps rendering only the knowledge-graph topology, so the topology is always visible and the overlay changes with the lens."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "checked={lens === 'evaluative'}",
            "detail": "The Evaluative lens checkbox reflects and drives the lens state on the single unified canvas surface (Graph and Flow converged; there is no second tab)."
          }
        ]
      },
      {
        "id": "a39c7c63d96ae396b45ee794",
        "decision": "context",
        "requirement_quote": "### Lens Definitions",
        "detail": "Container heading introducing the three lens subsections; it has no body text of its own. Its requirements are fully accounted for by the children Structural (default), Evaluative, and Observable (future), each audited separately below.",
        "evidence": []
      },
      {
        "id": "c3a39b320325eff390c4b56c",
        "decision": "gap",
        "requirement_quote": "Complexity/churn indicators: node size reflects complexity, border thickness reflects churn",
        "detail": "GAP (this sub-requirement only): the specced complexity/churn indicators never occur in production. The frontend rendering contract exists — node effective weight is Math.max(1, n.complexity || 1) (ExplorerCanvas.svelte:1814) and border width is Math.min(4, 1 + (n?.churn_count_30d || 0) * 0.3) (ExplorerCanvas.svelte:3717) — but the data side is inert: ALL four language extractors hardcode `complexity: None, churn_count_30d: 0` in their production node constructors (rust_extractor.rs:160-161, go_extractor.rs:135-136, python_extractor.rs:136-137, typescript_extractor.rs:129-130); the only code assigning non-null complexity or non-zero churn is test fixtures (view_query_resolver.rs `mod tests` setters at :4681/:5172/:6373, graph_diff.rs/spec_assertions.rs test mutation); graph_extraction.rs:811 itself documents these as \"metrics not extracted by the RustExtractor (complexity, churn, coverage)\"; and the SQLite upsert (sqlite/graph.rs:366-367) overwrites any stored value with the extractor's hardcoded None/0 on every re-extraction. An exhaustive grep of crates/ for churn_count_30d/complexity assignments finds no production cyclomatic computation or increment-from-history anywhere (GET /repos/:id/graph/risks at api/graph.rs:1132 merely re-exports the stored zero as churn_rate). Consequently every node renders at uniform size (nodeWeight = 1) and uniform 1px border (churnThickness = 1): a hardcoded/absent value standing in for a looked-up one. Cross-spec reuse (not duplicate ownership): this gap is already owned by the realized-model.md plan (plan_id b26f707c1c6ea89a961ab82e824808bf1625a3e2b3a0c2312981a90a99f3fef3, status planned, /tmp/stage/other-plans/ef4c1074caa37e617259c45b7ee00719.json) task key 'graph-node-metrics' (real cyclomatic complexity per extractor, 30-day churn accumulated from persisted ArchitecturalDelta history, upsert-overwrite fix); its single implementation also closes this explorer-canvas.md Structural indicator requirement because the frontend formulas already exist and activate once the metrics are populated. All OTHER Structural sub-requirements are independently verified satisfied in production at this base: spec-coverage coloring via specBorderColor (green #22c55e governed / amber #eab308 heuristic / red #ef4444 none), causal flow paths via Calls/RoutesTo edges (Causal Flow Trace section), single-click detail panel with moldable type/trait/endpoint views, double-click drill-down via Contains edges, edge-click relationship detail, and the right-click context menu with View spec / View provenance / Trace from here; no particles on the structural lens (particle data is gated to lens === 'evaluative'), the only animated element being the HSI-mandated active-agent glow pulse from a declared dependency spec.",
        "evidence": [
          {
            "file": "crates/gyre-domain/src/rust_extractor.rs",
            "quote": "complexity: None,",
            "detail": "The Rust extractor's production make_node constructor hardcodes complexity to None (and the adjacent line churn_count_30d: 0) for every node it emits — no cyclomatic computation exists anywhere in the crate."
          },
          {
            "file": "crates/gyre-domain/src/python_extractor.rs",
            "quote": "complexity: None,",
            "detail": "Same hardcode in the Python extractor's make_node (lines 136-137: complexity: None, churn_count_30d: 0); go_extractor.rs:135-136 and typescript_extractor.rs:129-130 are identical, so all four language extractors emit zero metrics."
          },
          {
            "file": "crates/gyre-server/src/graph_extraction.rs",
            "quote": "metrics not extracted by the",
            "detail": "The production extraction pipeline's own doc comment on diff_nodes (line 810-811) documents complexity, churn and coverage as metrics not extracted by the extractors — the pipeline knows the data side does not exist."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/graph.rs",
            "quote": "graph_nodes::complexity.eq(row.complexity),",
            "detail": "The SQLite upsert writes row.complexity / row.churn_count_30d (line 366-367) back from the extractor-produced node on every re-extraction, overwriting any previously stored value with the hardcoded None/0 — so even manually stored metrics cannot survive a re-extraction."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "return Math.max(1, n.complexity || 1);",
            "detail": "The rendering formula exists (node effective weight from complexity, ExplorerCanvas.svelte:1814) but with complexity always null in production data it evaluates to 1 for every node — uniform node size, so 'node size reflects complexity' never occurs."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "const churnThickness = Math.min(4, 1 + (n?.churn_count_30d || 0) * 0.3);",
            "detail": "The border-thickness formula exists (ExplorerCanvas.svelte:3717) but with churn_count_30d always 0 in production data it evaluates to a uniform 1px border — 'border thickness reflects churn' never occurs."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "if (conf === 'high') return '#22c55e';",
            "detail": "Satisfied sub-requirement evidence: specBorderColor implements the green/amber/red spec-coverage coloring (green for high-confidence/GovernedBy-governed, amber #eab308 for heuristic-only, red #ef4444 for no spec)."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "<button class=\"ctx-item\" role=\"menuitem\" onclick={() => contextMenuAction('trace')} data-testid=\"ctx-trace\">",
            "detail": "Satisfied sub-requirement evidence: the right-click context menu contains 'Trace from here' alongside 'View spec' (shown when node.spec_path exists) and 'View provenance', plus moldable view actions — the full context-menu contract of this section."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "doc_comment: `${edgeHit.edgeType",
            "detail": "Satisfied sub-requirement evidence: clicking an edge opens a relationship detail in the detail panel carrying the edge type and source/target qualified names — the specced 'Click an edge → shows the relationship detail'."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "if (!traceData?.spans?.length || lens !== 'evaluative') return null;",
            "detail": "Satisfied sub-requirement evidence: particles exist only in the evaluative lens — on the structural (default) lens no particle animation is produced, satisfying 'No particles. No animation. Pure structure.' with respect to flow animation."
          }
        ]
      },
      {
        "id": "102a88fbd46e42ad0e9c5aea",
        "decision": "satisfied",
        "requirement_quote": "Playback controls: play/pause, step (span-by-span, HSI §3a), scrubber, speed (0.25x–10x — the union of this range and HSI §3a's 1x/10x/step-through), test-case selector (ui-layout.md §4)",
        "detail": "The Evaluative lens is fully implemented as an overlay on the structural canvas. Data source: OTLP spans fetched via GET /merge-requests/:id/trace (api.js mrTrace; route registered in api/mod.rs). Overlays: particles animated along edges from real trace spans (flow-particles.js), node heat colored blue→red by duration, per-node badges showing span count, error rate and mean duration as chips, edge thickness proportional to trace call frequency, red glow on nodes with failed spans and red error particles. PlaybackControls renders play/pause, step-to-next-span, a range-input scrubber, a speed selector with 0.25x/0.5x/1x/2x/5x/10x (the required 0.25x–10x union), and a test-case selector filter plus per-test legend. Interaction: structural interactions still work (same canvas), particle clicks emit a span detail node carrying operation/duration/status/attributes/input_summary/output_summary, node clicks add an Evaluative tab in NodeDetailPanel listing spans sorted by duration, and node hover shows a tooltip with p50/p95/error-rate computed from per-node span statistics.",
        "evidence": [
          {
            "file": "web/src/lib/api.js",
            "quote": "mrTrace: (id) => request(`/merge-requests/${id}/trace`),",
            "detail": "The evaluative data source is the specced endpoint: the frontend fetches OTLP trace spans via GET /merge-requests/:id/trace and feeds them to the canvas as traceData."
          },
          {
            "file": "web/src/components/PlaybackControls.svelte",
            "quote": "<option value=\"0.25\">0.25x</option>",
            "detail": "Playback speed selector includes 0.25x (with 0.5/1/2/5 and 10x), covering the required 0.25x–10x range; the same bar renders play/pause, a step button, a range-input scrubber, and a test-case filter select with a per-test legend."
          },
          {
            "file": "web/src/components/PlaybackControls.svelte",
            "quote": "<option value=\"10\">10x</option>",
            "detail": "The 10x upper bound of the specced speed range is present in the playback controls."
          },
          {
            "file": "web/src/components/PlaybackControls.svelte",
            "quote": "<input type=\"range\" min=\"0\" max=\"1000\" value={Math.round(evalScrubber * 1000)}",
            "detail": "The scrubber is a real range input bound to the playback position; the adjacent step button (title 'Step to next span') and test-case select implement the remaining playback controls."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "// Spec §4: node badges show span count, error rate, AND mean duration —",
            "detail": "In the evaluative lens the draw loop renders a chip row per node with span count, error-rate percentage, and mean duration, exactly the three badge metrics the section lists."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "lineWidth = 1.5 + (freq / traceMaxFreq) * 4.5;",
            "detail": "Edge thickness is proportional to OTLP trace span frequency (normalized against the max traced frequency), implementing 'Edge thickness: proportional to call frequency in traces'."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "// Red glow on nodes with failed spans (evaluative lens)",
            "detail": "Nodes whose spans include status 'error' get a red shadow-glow stroke in the evaluative lens, implementing 'Failed spans: red glow on nodes'."
          },
          {
            "file": "web/src/lib/flow-particles.js",
            "quote": "export const ERROR_PARTICLE_COLOR = '#ef4444';",
            "detail": "Particles belonging to error spans render red (ERROR_PARTICLE_COLOR vs SUCCESS_PARTICLE_COLOR), implementing 'error particles'."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "input_summary: span.input_summary,",
            "detail": "Clicking a particle emits a span detail node carrying span_id, duration_us, status, attributes, input_summary and output_summary to the detail panel — the specced particle-click span view."
          },
          {
            "file": "web/src/lib/NodeDetailPanel.svelte",
            "quote": "// ── Evaluative tab: per-node span list and stats ──",
            "detail": "NodeDetailPanel adds an Evaluative tab (shown when lens === 'evaluative' and the node has spans) listing all spans that touched the node with aggregate p50/p95/mean/error-rate stats, satisfying the node-click evaluative tab requirement."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "const p95 = percentile(sorted, 0.95);",
            "detail": "Per-node span statistics compute p50 and p95 (plus error rate) from the trace spans; these feed the hover tooltip, which renders p50/p95/error-rate lines — the specced hover timing stats."
          }
        ]
      },
      {
        "id": "5b9d8d1087862c3088714f68",
        "decision": "satisfied",
        "requirement_quote": "Grayed out in UI with label: \"Requires production telemetry integration.\"",
        "detail": "The Observable lens is future/disabled exactly as specified. The toolbar toggle is a non-activatable checkbox: the input is disabled, the wrapper is styled at opacity 0.45 with not-allowed cursor and italic '(requires production telemetry integration)' label text, and interacting with it surfaces a dismissible notice banner explaining that the Observable lens requires an OpenTelemetry collector (GYRE_OTLP_ENDPOINT) instead of activating anything. normalizeLens in lens-rendering.js also collapses any non-'evaluative' lens value (including 'observable') back to 'structural', so the lens can never actually activate.",
        "evidence": [
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "title=\"Observable lens is disabled — requires production telemetry integration\"",
            "detail": "The Observable toggle is disabled in the toolbar, carrying the disable reason as its title; its visible label text is 'Observable (requires production telemetry integration)'."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": ".lens-check-observable { opacity: 0.45; font-style: italic; cursor: not-allowed; }",
            "detail": "The Observable toggle is visually grayed out (45% opacity, not-allowed cursor), implementing 'Grayed out in UI'."
          },
          {
            "file": "web/src/components/ObservableBanner.svelte",
            "quote": "Observable lens requires an OpenTelemetry collector.",
            "detail": "Interacting with the disabled toggle shows a status banner stating the Observable lens requires a production OpenTelemetry collector — the lens stays disabled until production telemetry integration exists."
          },
          {
            "file": "web/src/lib/lens-rendering.js",
            "quote": "return value === 'evaluative' ? 'evaluative' : 'structural';",
            "detail": "normalizeLens collapses every non-'evaluative' lens value — including 'observable' — back to 'structural', so the disabled Observable lens can never actually activate through the URL parameter or state either."
          }
        ]
      },
      {
        "id": "b9dbe439e682ea3f849cefd9",
        "decision": "satisfied",
        "requirement_quote": "**Breadcrumb bar** shows the drill-down path: `e2e-repo > greeting-service > greeting > GreetingService`. Each segment is clickable to navigate back up.",
        "detail": "Progressive drill-down is fully implemented. treeData builds parent→children maps exclusively from 'contains' edges. Double-clicking a container node calls drillInto: it starts a fade of unrelated nodes (drillFadeAlpha animating toward ~0.12), records drillStartCam and eases the camera (ease-out cubic lerp) onto the clicked node, then swaps the scope to the node's children and pushes a breadcrumb segment. The Breadcrumb component renders the path (repo root label via rootLabel, then package > module > type) and every segment navigates back up via navigateBreadcrumb, which truncates the breadcrumb and plays the reverse smooth zoom-out transition. Leaf double-click opens the detail panel instead of drilling. The lens toggle works at every level: particles and span stats are keyed to the current level's node ids. A repo-scope drill (double-click a repo node at workspace scope) is also supported via the onScopeDrill callback.",
        "evidence": [
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "if (etype !== 'contains') continue;",
            "detail": "The drill-down hierarchy (childToParent/parentToChildren maps) is built solely from Contains edges of the knowledge graph, so drilling follows the specced containment structure."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "// Progressive drill-down via Contains edges (explorer-canvas.md §Progressive Drill-Down):",
            "detail": "onDblClick routes double-clicks on containers to drillInto, which filters the view to the drilled node's children; the adjacent lines update the breadcrumb and start the transition, and leaf nodes open the detail panel instead."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "drillStartCam = { x: cam.x, y: cam.y, zoom: cam.zoom };",
            "detail": "The zoom transition records the pre-drill camera and eases toward the target (fit-to-node) camera with an ease-out cubic interpolation in the render loop — the specced smooth zoom into the clicked node; navigateBreadcrumb sets the same field to play the reverse transition when navigating back up."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "let drillFadeAlpha = $state(1.0); // 1.0 = fully visible, fading to 0.15",
            "detail": "Unrelated nodes fade out during the drill transition (animated toward ~0.12 alpha in the render loop) and fade back in on reverse navigation — the specced 'fades out unrelated nodes' behavior in both directions."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "function navigateBreadcrumb(index) {",
            "detail": "Breadcrumb navigation: each clickable segment truncates the path back to that ancestor and animates a smooth zoom-out camera transition to the parent (or a fit-all when returning to root), implementing 'Each segment is clickable to navigate back up' with the reverse transition."
          }
        ]
      },
      {
        "id": "d97f8f2e468a98f6af9c2d50",
        "decision": "satisfied",
        "requirement_quote": "**Data requirement**: The knowledge graph must contain `Calls` edges. This requires enhancing the code extractors (Rust, Python, TypeScript) to analyze function bodies and emit call relationships, not just declarations.",
        "detail": "The causal flow trace loop is fully real. Activation: right-click 'Trace from here' (primary context-menu section) and a toolbar Trace button for the selected node, both via startTraceFromNode, which issues a focus view query over outgoing calls/routes_to edges (depth 15) with dim_unmatched 0.2. Traversal: causal-trace.js computeTraceOrder performs a cycle-safe BFS over outgoing Calls/RoutesTo edges with deterministic sibling ordering; the trace effect computes tracePathOrder/tracePathEdges from it. Rendering: the selected start node gets step ①, reachable nodes are highlighted (tiered colors) and numbered in BFS execution order via red circle badges, unreachable nodes dim to 20% opacity, and each badge is annotated with its governing spec (spec_path or GovernedBy edge target). Detail panel: the trace steps are attached to the detail node (_traceSteps) and NodeDetailPanel renders the full trace as a clickable list wired to camera focus (onTraceStepNavigate). Exit affordances: toolbar Clear trace, right-click Clear trace, Escape (window-level listener). When the graph has no causal edges, a warning banner is shown instead of a degenerate trace. Data requirement: Calls edges are extracted in production by a two-pass pipeline — Pass 1 syntax extraction (the Rust extractor in gyre-domain walks function bodies, deduplicates existing Calls edges and emits caller→target Calls edges) and Pass 2 semantic extraction via language type checkers (SubprocessCallGraphExtractor dispatches Rust to rust-analyzer, Python to the pyright-based python-callgraph.sh driver, TypeScript to ts-callgraph.mjs, Go to go-callgraph), wired into the production push/extraction flow as a background task that resolves and persists the Calls edges into the graph store.",
        "evidence": [
          {
            "file": "web/src/lib/causal-trace.js",
            "quote": "export function computeTraceOrder(nodes, edges, rootId) {",
            "detail": "Pure BFS traversal over outgoing calls/routes_to edges from the start node, producing a 1-based execution-ordered step map and traversed path edges — the core of trace mode steps 1–2 (start node highlights, reachable nodes highlighted in sequence), cycle-safe with deterministic sibling ordering."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "emphasis: { tiered_colors: ['#ef4444', '#f97316', '#eab308', '#22c55e', '#94a3b8'], dim_unmatched: 0.2 },",
            "detail": "startTraceFromNode issues the trace view query with dim_unmatched 0.2 — unreachable nodes dim to 20% opacity exactly as step 3 requires — and highlights matched (reachable) nodes with tiered colors."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "ctx.fillText(String(stepNum), bx, by);",
            "detail": "The draw loop renders numbered step badges (red circles with white step numbers in BFS order) on every traced node — step 4's numbered path ① → ② → ③ showing execution order."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "const govSpec = governingSpecFor(n, edges);",
            "detail": "Each trace step badge is annotated with its governing spec (resolved from spec_path or GovernedBy edges) — step 5's spec annotation."
          },
          {
            "file": "web/src/lib/NodeDetailPanel.svelte",
            "quote": "{#if node._traceSteps?.length > 0}",
            "detail": "The detail panel shows the full trace as a step list (step count badge, per-step entries) where each step is clickable and navigates the canvas camera to that node — step 6's clickable trace list."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "if (!hasCausalEdges(edges)) {",
            "detail": "startTraceFromNode guards against graphs with zero Calls/RoutesTo edges by showing a warning banner ('Call graph not available — run code sync to extract call edges') instead of entering a degenerate trace."
          },
          {
            "file": "crates/gyre-adapters/src/call_graph.rs",
            "quote": "Language::Python => match find_script(\"python-callgraph.sh\") {",
            "detail": "Pass 2 semantic call-graph extraction covers Python via the pyright-based python-callgraph.sh driver script (scripts/python-callgraph.sh, scripts/python-callgraph-pyright.mjs), emitting caller→callee CallEdge pairs — the Python extractor enhancement the data requirement names."
          },
          {
            "file": "crates/gyre-adapters/src/call_graph.rs",
            "quote": "Language::TypeScript => match find_script(\"ts-callgraph.mjs\") {",
            "detail": "Pass 2 covers TypeScript via the ts-callgraph.mjs LSP driver emitting caller→callee edges (complementing the Pass 1 typescript_extractor API-call-site Calls edges), satisfying the TypeScript part of the data requirement."
          },
          {
            "file": "crates/gyre-server/src/graph_extraction.rs",
            "quote": "let extractor = gyre_adapters::call_graph::SubprocessCallGraphExtractor::new();",
            "detail": "The Pass 2 call-graph extraction is wired into the production graph-extraction pipeline (a non-blocking background task after push) that resolves and persists Calls edges into the graph store — the knowledge graph really contains Calls edges in production, not just in tests."
          },
          {
            "file": "crates/gyre-domain/src/rust_extractor.rs",
            "quote": "let edge = self.make_edge(EdgeType::Calls, caller_id, target_id);",
            "detail": "Pass 1 syntax extraction for Rust analyzes function bodies and emits Calls edges from caller to resolved target (after deduplicating existing Calls edges), satisfying the Rust part of the data requirement that extractors emit call relationships, not just declarations."
          }
        ]
      },
      {
        "id": "6b11385a777ef660b38d6e8a",
        "decision": "context",
        "requirement_quote": "| Structural causality | `Calls`/`RoutesTo` edges | Same endpoint | **Need call-graph extraction** |",
        "detail": "This section is a reference/tracking table mapping lenses to data sources, endpoints, and status-at-time-of-writing; it states no behavior beyond what the lens sections already require (each row's data path is audited under Structural/Evaluative/Causal Flow Trace/Component Architecture). Spot-checked every row against production: GET /repos/:id/graph is registered; Calls/RoutesTo extraction now exists (Pass 1 syntax + Pass 2 LSP call-graph pipeline — the 'Need call-graph extraction' status is obsolete in the spec's own tracking terms, the extraction is built); GET /merge-requests/:id/trace and GET /trace-spans/:span_id/payload are registered and consumed by the frontend; Contains and GovernedBy edges flow through the same graph endpoint. No independent gap is countable here without double-counting the lens sections.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": ".route(\"/api/v1/repos/:id/graph\", get(graph::get_repo_graph))",
            "detail": "The structural-topology data path row is real: the knowledge-graph endpoint the table lists exists and serves nodes/edges (including Contains and GovernedBy)."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/merge-requests/:id/trace\",",
            "detail": "The evaluative-traces row's endpoint is registered in production and consumed by the frontend evaluative lens (api.js mrTrace)."
          },
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/trace-spans/:span_id/payload\",",
            "detail": "The evaluative span-detail row's endpoint is registered in production (traces.rs get_span_payload serves span payloads with tenant scoping)."
          }
        ]
      },
      {
        "id": "79dcb4bb8b305568aa2d80f1",
        "decision": "satisfied",
        "requirement_quote": "Replace the current `MoldableView` tabs (Graph, List, Timeline, Flow) with:",
        "detail": "The component architecture is realized in production. The old MoldableView tab component no longer exists (the live surface is ExplorerView, imported and rendered by web/src/components/RepoMode.svelte); its tab set was replaced by the layout registry: 'graph' and 'flow' both resolve to the unified ExplorerCanvas (merged Graph+Flow — the separate FlowRenderer is gone, its particle logic lives in the shared pure module flow-particles.js), 'list' renders the shell's VirtualNodeList, 'timeline' renders through the canvas with TimelineScrubber. ExplorerCanvas is the layout dispatcher: when the active ViewSpec layout is 'flow' it delegates node/edge/particle rendering to FlowCanvas (SVG node/edge layer, clickable and accessible, with per-node SVG badge chips) while keeping the chrome (toolbar, lens toggle, breadcrumb); on non-flow views it renders the graph on its Canvas 2D context and switches to a WebGL2 particle overlay above 100 concurrent particles via particle-layer.js (WEBGL_PARTICLE_THRESHOLD = 100, ParticleLayer created on a dedicated overlay element). PlaybackControls (play/pause/step/scrubber/speed/test selector), EvaluativeOverlay (evaluative metric selector toolbar group, rendered when lens === 'evaluative'), NodeBadge (canvas-drawn metric chips in the 2D path, SVG flow-badge chips in FlowCanvas) and Breadcrumb (drill-down path) all exist and are composed inside ExplorerCanvas exactly as the tree mandates.",
        "evidence": [
          {
            "file": "web/src/lib/layoutRegistry.js",
            "quote": "// task-065 unified cutover: the separate FlowRenderer tab was merged",
            "detail": "The tab replacement happened: the registry maps 'graph' and 'flow' to the same unified ExplorerCanvas component (FlowRenderer merged into the evaluative lens particle animation with shared pure logic in flow-particles.js), and 'list'/'timeline' remain as layouts — the Explorer/List/Timeline structure with no separate Flow tab."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "const isFlowLayout = $derived(layout === 'flow');",
            "detail": "ExplorerCanvas is the layout dispatcher: when the active ViewSpec layout is 'flow' it delegates node/edge/particle rendering to FlowCanvas (the render branch renders FlowCanvas for isFlowLayout) while keeping the toolbar/lens/breadcrumb chrome, exactly as the section specifies."
          },
          {
            "file": "web/src/lib/particle-layer.js",
            "quote": "export const WEBGL_PARTICLE_THRESHOLD = 100;",
            "detail": "The particle overlay switches from Canvas 2D to WebGL2 above 100 concurrent particles, the threshold the section mandates."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "particleLayer = new ParticleLayer(el, { className: 'particle-overlay-canvas' });",
            "detail": "ExplorerCanvas creates the WebGL2-capable ParticleLayer on a dedicated overlay element above its Canvas 2D context (a canvas permanently keeps its first context type, hence the separate element), used when evalParticles exceed the threshold."
          },
          {
            "file": "web/src/lib/FlowCanvas.svelte",
            "quote": "<text class=\"flow-badge-text\" x={chip.w / 2} y=\"9\">{chip.label}</text>",
            "detail": "FlowCanvas renders per-node metric badge chips as SVG inside its node/edge layer — the NodeBadge 'SVG in FlowCanvas' half of the contract."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "<EvaluativeOverlay",
            "detail": "The EvaluativeOverlay component (evaluative metric selector toolbar group) is rendered inside ExplorerCanvas when the evaluative lens is active — the tree's EvaluativeOverlay node."
          },
          {
            "file": "web/src/lib/layoutRegistry.js",
            "quote": "component: null, // list renders as the shell's virtualized node list (VirtualNodeList)",
            "detail": "The List view survives as a registry layout rendered by the shell's VirtualNodeList, and 'timeline' likewise renders via the canvas TimelineScrubber — the 'List tab (unchanged)' and 'Timeline tab (unchanged)' branches of the component tree."
          }
        ]
      },
      {
        "id": "4636ee636427c777c40e2480",
        "decision": "context",
        "requirement_quote": "**Phase 1: Converge tabs + lens toggle**",
        "detail": "Implementation Phases is a phased-rollout plan (Phases 1–4), not a requirement beyond the features themselves; every phase's feature is mandated and audited under the design sections (Phase 1 → One Canvas Three Lenses / Component Architecture; Phase 2 → Progressive Drill-Down; Phase 3 → Causal Flow Trace; Phase 4 → Evaluative). All four phases' features are present in production code as evidenced in those sections, so no separate gap exists here. Anchored with the Phase 1 tab-convergence evidence.",
        "evidence": [
          {
            "file": "web/src/lib/layoutRegistry.js",
            "quote": "// task-065 unified cutover: the separate FlowRenderer tab was merged",
            "detail": "Phase 1 (merge Graph and Flow into a single Explorer tab, remove the separate Flow tab) is the shipped state: both layouts resolve to the unified ExplorerCanvas with the lens toggle, and the separate Flow tab no longer exists."
          }
        ]
      },
      {
        "id": "9414b4073a25f768b59ce846",
        "decision": "context",
        "requirement_quote": "**Implements**: system-explorer.md §1 (navigable architecture), §3 (three lenses), §5 (flow traces)",
        "detail": "Cross-reference section mapping this spec to system-explorer.md and realized-model.md/human-system-interface.md; it declares relationships, not implementable requirements. The referenced dependencies are covered by their own canonical specs and audits (system-explorer.md and realized-model.md have separate audit assignments); the 'Supersedes' claim (Graph tab / Flow tab in MoldableView) is verified under the Component Architecture section above.",
        "evidence": []
      }
    ],
    "gaps": [
      {
        "id": "c3a39b320325eff390c4b56c",
        "decision": "gap",
        "requirement_quote": "Complexity/churn indicators: node size reflects complexity, border thickness reflects churn",
        "detail": "GAP (this sub-requirement only): the specced complexity/churn indicators never occur in production. The frontend rendering contract exists — node effective weight is Math.max(1, n.complexity || 1) (ExplorerCanvas.svelte:1814) and border width is Math.min(4, 1 + (n?.churn_count_30d || 0) * 0.3) (ExplorerCanvas.svelte:3717) — but the data side is inert: ALL four language extractors hardcode `complexity: None, churn_count_30d: 0` in their production node constructors (rust_extractor.rs:160-161, go_extractor.rs:135-136, python_extractor.rs:136-137, typescript_extractor.rs:129-130); the only code assigning non-null complexity or non-zero churn is test fixtures (view_query_resolver.rs `mod tests` setters at :4681/:5172/:6373, graph_diff.rs/spec_assertions.rs test mutation); graph_extraction.rs:811 itself documents these as \"metrics not extracted by the RustExtractor (complexity, churn, coverage)\"; and the SQLite upsert (sqlite/graph.rs:366-367) overwrites any stored value with the extractor's hardcoded None/0 on every re-extraction. An exhaustive grep of crates/ for churn_count_30d/complexity assignments finds no production cyclomatic computation or increment-from-history anywhere (GET /repos/:id/graph/risks at api/graph.rs:1132 merely re-exports the stored zero as churn_rate). Consequently every node renders at uniform size (nodeWeight = 1) and uniform 1px border (churnThickness = 1): a hardcoded/absent value standing in for a looked-up one. Cross-spec reuse (not duplicate ownership): this gap is already owned by the realized-model.md plan (plan_id b26f707c1c6ea89a961ab82e824808bf1625a3e2b3a0c2312981a90a99f3fef3, status planned, /tmp/stage/other-plans/ef4c1074caa37e617259c45b7ee00719.json) task key 'graph-node-metrics' (real cyclomatic complexity per extractor, 30-day churn accumulated from persisted ArchitecturalDelta history, upsert-overwrite fix); its single implementation also closes this explorer-canvas.md Structural indicator requirement because the frontend formulas already exist and activate once the metrics are populated. All OTHER Structural sub-requirements are independently verified satisfied in production at this base: spec-coverage coloring via specBorderColor (green #22c55e governed / amber #eab308 heuristic / red #ef4444 none), causal flow paths via Calls/RoutesTo edges (Causal Flow Trace section), single-click detail panel with moldable type/trait/endpoint views, double-click drill-down via Contains edges, edge-click relationship detail, and the right-click context menu with View spec / View provenance / Trace from here; no particles on the structural lens (particle data is gated to lens === 'evaluative'), the only animated element being the HSI-mandated active-agent glow pulse from a declared dependency spec.",
        "evidence": [
          {
            "file": "crates/gyre-domain/src/rust_extractor.rs",
            "quote": "complexity: None,",
            "detail": "The Rust extractor's production make_node constructor hardcodes complexity to None (and the adjacent line churn_count_30d: 0) for every node it emits — no cyclomatic computation exists anywhere in the crate."
          },
          {
            "file": "crates/gyre-domain/src/python_extractor.rs",
            "quote": "complexity: None,",
            "detail": "Same hardcode in the Python extractor's make_node (lines 136-137: complexity: None, churn_count_30d: 0); go_extractor.rs:135-136 and typescript_extractor.rs:129-130 are identical, so all four language extractors emit zero metrics."
          },
          {
            "file": "crates/gyre-server/src/graph_extraction.rs",
            "quote": "metrics not extracted by the",
            "detail": "The production extraction pipeline's own doc comment on diff_nodes (line 810-811) documents complexity, churn and coverage as metrics not extracted by the extractors — the pipeline knows the data side does not exist."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/graph.rs",
            "quote": "graph_nodes::complexity.eq(row.complexity),",
            "detail": "The SQLite upsert writes row.complexity / row.churn_count_30d (line 366-367) back from the extractor-produced node on every re-extraction, overwriting any previously stored value with the hardcoded None/0 — so even manually stored metrics cannot survive a re-extraction."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "return Math.max(1, n.complexity || 1);",
            "detail": "The rendering formula exists (node effective weight from complexity, ExplorerCanvas.svelte:1814) but with complexity always null in production data it evaluates to 1 for every node — uniform node size, so 'node size reflects complexity' never occurs."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "const churnThickness = Math.min(4, 1 + (n?.churn_count_30d || 0) * 0.3);",
            "detail": "The border-thickness formula exists (ExplorerCanvas.svelte:3717) but with churn_count_30d always 0 in production data it evaluates to a uniform 1px border — 'border thickness reflects churn' never occurs."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "if (conf === 'high') return '#22c55e';",
            "detail": "Satisfied sub-requirement evidence: specBorderColor implements the green/amber/red spec-coverage coloring (green for high-confidence/GovernedBy-governed, amber #eab308 for heuristic-only, red #ef4444 for no spec)."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "<button class=\"ctx-item\" role=\"menuitem\" onclick={() => contextMenuAction('trace')} data-testid=\"ctx-trace\">",
            "detail": "Satisfied sub-requirement evidence: the right-click context menu contains 'Trace from here' alongside 'View spec' (shown when node.spec_path exists) and 'View provenance', plus moldable view actions — the full context-menu contract of this section."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "doc_comment: `${edgeHit.edgeType",
            "detail": "Satisfied sub-requirement evidence: clicking an edge opens a relationship detail in the detail panel carrying the edge type and source/target qualified names — the specced 'Click an edge → shows the relationship detail'."
          },
          {
            "file": "web/src/lib/ExplorerCanvas.svelte",
            "quote": "if (!traceData?.spans?.length || lens !== 'evaluative') return null;",
            "detail": "Satisfied sub-requirement evidence: particles exist only in the evaluative lens — on the structural (default) lens no particle animation is produced, satisfying 'No particles. No animation. Pure structure.' with respect to flow animation."
          }
        ]
      }
    ],
    "source_evidence": [
      {
        "file": "web/src/lib/layoutRegistry.js",
        "sha256": "3f3323b42832d5a36d02a008895bf0a584546b938af505020f569ce5027d1602"
      },
      {
        "file": "web/src/lib/ExplorerCanvas.svelte",
        "sha256": "593408c6040082976f53f350822af0eafa176ab74b13e275cb1d19065529eb39"
      },
      {
        "file": "crates/gyre-domain/src/rust_extractor.rs",
        "sha256": "fdb82ae7cba34d5c1989f23953ebee8ddba1929b528a7458f27464384f23a76e"
      },
      {
        "file": "crates/gyre-domain/src/python_extractor.rs",
        "sha256": "38ee8a4119eb4b0e1ed2024d428571209cf7b12f994ee73a2cdfe0b394162d2d"
      },
      {
        "file": "crates/gyre-server/src/graph_extraction.rs",
        "sha256": "1a64bacf9185670f8c577fe0debc0a0fb25db65a52464003071fd876ad1c6ba0"
      },
      {
        "file": "crates/gyre-adapters/src/sqlite/graph.rs",
        "sha256": "30734de5c4ace8c9b7c6698da8d1e6f4e376f4b22baf8df921994941103dab1f"
      },
      {
        "file": "web/src/lib/api.js",
        "sha256": "de3d8147c3ebaad027847809ebcf72a5ad6286e6983544f715931c307c927740"
      },
      {
        "file": "web/src/components/PlaybackControls.svelte",
        "sha256": "adc23ede65ecf5940c77682f8f8dc3328d7af6cd1b2295f7ca7b156711f4ba85"
      },
      {
        "file": "web/src/lib/flow-particles.js",
        "sha256": "fb8e68dd56c7ec41aa50cbb5342359f9a24dfff0b7dc7d0cca1e144dc562fe76"
      },
      {
        "file": "web/src/lib/NodeDetailPanel.svelte",
        "sha256": "ae704828d736e7d2fdbd91964986f44f4f29e821d172ac6af839b0b6ad44bdeb"
      },
      {
        "file": "web/src/components/ObservableBanner.svelte",
        "sha256": "9f3dbdff87e43fb4402cfeac836f870dafed1653fe0dc529bf5ec1cc6df71aeb"
      },
      {
        "file": "web/src/lib/lens-rendering.js",
        "sha256": "3acec7bb14c2686be3078167e076139e6b63acfc038371306a72418bb536b451"
      },
      {
        "file": "web/src/lib/causal-trace.js",
        "sha256": "072e5fced93c41a76d50b7878e79422f831febe2c91768b405640f7dc86464bd"
      },
      {
        "file": "crates/gyre-adapters/src/call_graph.rs",
        "sha256": "71054c64624059526ab79cda3ad34dd2f0ac24d32f806687ba5a5cec176db2d2"
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "sha256": "036b32975f4110d6aa35cef408441c0be5ecb4cef4a9a7504eee9caf08a8bc35"
      },
      {
        "file": "web/src/lib/particle-layer.js",
        "sha256": "fb5834f70c8f5e5d2df78f329d6e950bd879ccb8ce545f360f5563bc70ff57b1"
      },
      {
        "file": "web/src/lib/FlowCanvas.svelte",
        "sha256": "918ce5755fce388edb8da0f409e851c9e16b0a9ae14597035070990557334b6c"
      }
    ]
  },
  "plan": {
    "audit": "97d0fc325807b848c6ef43e52aa33d5ef25a175cc0ad733f4d2f86d0b4430303",
    "tasks": [
      {
        "key": "structural-metrics-verification",
        "title": "Verify structural-lens complexity/churn indicators end-to-end on the live graph renderer (backend implementation owned by the realized-model plan's graph-node-metrics task)",
        "covers": [
          "c3a39b320325eff390c4b56c"
        ],
        "required_behavior": "explorer-canvas.md §Structural (default) requires 'Complexity/churn indicators: node size reflects complexity, border thickness reflects churn'. Ownership split (fixed and unconditional): the backend implementation — per-extractor cyclomatic complexity computed from parsed source bodies, churn_count_30d accumulated from persisted ArchitecturalDelta history inside the 30-day window, and the SQLite upsert no-overwrite fix at sqlite/graph.rs:366-367 — is owned solely by the realized-model.md plan's 'graph-node-metrics' task (plan_id b26f707c1c6ea89a961ab82e824808bf1625a3e2b3a0c2312981a90a99f3fef3, status planned, /tmp/stage/other-plans/ef4c1074caa37e617259c45b7ee00719.json, covering realized-model section 2d5dce802e2c98aca99b3559). This task NEVER implements or modifies the backend metric computation, the extractors, or the storage layer, and NEVER reroutes the layout registry. This task's scope is this spec's closure concerns only: (1) after the realized-model graph-node-metrics implementation has landed, verify end-to-end that GET /repos/:id/graph returns populated, branching-sensitive complexity (non-null for function/method nodes with bodies, varying with branching structure) and window-filtered churn_count_30d (> 0 only for nodes with a recorded ArchitecturalDelta modification inside the last 30 days). (2) Renderer-agnostic frontend verification: verify that 'node size reflects complexity, border thickness reflects churn' actually occurs on whichever renderer the layout registry serves for the 'graph' layout AT RUN TIME — the verification must resolve the renderer via getLayout('graph') / the live ExplorerView layout path (ExplorerView defaults activeLayout='graph', web/src/components/ExplorerView.svelte:455), never by hard-mounting ExplorerCanvas in isolation. At base 678a8ac3 that live renderer is ExplorerCanvas (Canvas 2D), where the formulas exist (nodeWeight = Math.max(1, n.complexity || 1) at web/src/lib/ExplorerCanvas.svelte:1813-1815; churnThickness = Math.min(4, 1 + (n?.churn_count_30d || 0) * 0.3) at ExplorerCanvas.svelte:3717), but this must not be assumed at run time. (3) Interaction case for the pending ui-layout work (mirroring the realized-model ordering guard): the ui-layout.md plan's 'explorer-surface-gaps' task (plan_id 2c7b116466535165ce9662e74653ac3a62425278b9986424aef39e8b4ad243ed, status planned, earlier audit_at 1795192041, /tmp/stage/other-plans/c587efa4d6aa9c92fe37f3d517f70505.json) item (4) reroutes the 'graph' layout to the SVG d3-force renderer (SvgGraphCanvas via layout-engines.js), which at base 678a8ac3 contains ZERO complexity/churn indicator rendering. If that rerouting has landed when this task runs and the SVG graph path lacks complexity/churn indicator rendering, that is a defect in this spec's own rendering surface to fix narrowly here (add size-by-complexity and stroke-width-by-churn encoding to the SVG graph path only) — or, if the underlying spec conflict is unresolved, report blocked-on-conflict. Never verify against the orphaned Canvas-2D path, never implement backend metrics locally, and never reroute the layout registry. (4) Conflict escalation: record in the task report the genuine spec-vs-spec conflict between explorer-canvas.md §Component Architecture ('ExplorerCanvas (layout dispatcher — Canvas 2D graph for graph/timeline views)') and ui-layout.md §10 ('The Explorer canvas uses SVG for graph/hierarchical/layered layouts'; engine table: graph → d3-force (Svelte wrapper), timeline → d3-scale + Svelte) — mutually incompatible canonical requirements for the same layouts — so a human decides through the spec lifecycle; it must not be settled implicitly by whichever plan lands first. Note: the Component Architecture section's 'satisfied' disposition (79dcb4bb8b305568aa2d80f1) is valid only while ExplorerCanvas actually serves the 'graph' layout. (5) Record section c3a39b320325eff390c4b56c's coverage to reflect the verified state. Ordering guard: if the realized-model graph-node-metrics work has NOT landed when this task runs, the section stays open and the task reports blocked-on-external-implementation — it must not close the section by any local backend implementation, fixture, or client-side metric fabrication. No frontend re-implementation beyond verification on whichever path currently serves 'graph': the only permitted frontend work is (a) the narrow fix if populated metrics reach the API but the live graph renderer fails to show them due to a formula or field-plumbing defect in that renderer (e.g. the field plumbing from the graph API response into node objects), or (b) the narrow SVG-path indicator addition under interaction case (3). No new tests for already-working behavior (goal out-of-scope); the only test work permitted is the end-to-end probe asserting the newly-populated metrics flow through to rendered node size/border on the live graph path. All other Structural sub-requirements (spec-coverage coloring via specBorderColor, causal flow paths, single-click detail panel, double-click drill-down, edge-click relationship detail, right-click context menu with View spec / View provenance / Trace from here, no particles on the structural lens) are independently verified satisfied at base 678a8ac3 and must not be touched.",
        "acceptance": [
          "This task ships zero changes to the four language extractors, the graph store/adapters, or the upsert path — per-extractor complexity computation, 30-day churn accumulation from ArchitecturalDelta history, and the upsert no-overwrite fix land exclusively via the realized-model.md plan's graph-node-metrics task (plan_id b26f707c1c6ea89a961ab82e824808bf1625a3e2b3a0c2312981a90a99f3fef3).",
          "After that implementation lands and a repo is extracted, GET /repos/:id/graph returns function/method nodes whose complexity is non-null and varies with branching structure (a nested-branch function reports higher complexity than a straight-line one) and whose churn_count_30d is > 0 only for nodes with a recorded ArchitecturalDelta modification inside the last 30 days (a node with modifications at 10 and 60 days ago reports 1; an untouched node reports 0).",
          "On the renderer that getLayout('graph') / the live ExplorerView layout path actually resolves to at run time, the populated metrics visibly differentiate rendering — node size above the renderer's minimum for branchy code nodes and border thickness above its baseline only for churned nodes (on the Canvas-2D path: nodeWeight > 1 and borderWidth > 1; on an SVG path: individually addressable node elements whose size/stroke-width scale with complexity/churn) — demonstrated with the real populated metric payload served through the graph API path, not a fixture that sets complexity/churn client-side. A probe that mounts ExplorerCanvas directly while the live 'graph' layout resolves to a different renderer does NOT satisfy this criterion (hollow-closure guard).",
          "If the ui-layout explorer-surface-gaps d3-force rerouting has landed when this task runs, the indicator verification of acceptance criterion 3 runs on the SVG d3-force path; if that path lacks complexity/churn indicator rendering, the task either fixes it narrowly (SVG graph path indicator encoding only) or reports blocked-on-conflict — it never verifies against the orphaned Canvas-2D ExplorerCanvas path, never implements backend metrics locally, and never reroutes the layout registry.",
          "The task report explicitly records the explorer-canvas.md §Component Architecture vs ui-layout.md §10 rendering-technology conflict (Canvas 2D vs SVG d3-force for 'graph'; Canvas 2D vs d3-scale+Svelte for 'timeline') for escalation through the spec lifecycle, rather than resolving it by landing order or unilateral registry changes.",
          "Section c3a39b320325eff390c4b56c's coverage row is updated to the verified state only after the end-to-end probe passes against the landed backend on the live graph renderer; while the realized-model implementation is unlanded, the section remains open (the task reports blocked rather than claiming closure).",
          "If the populated metrics reach the API but the live renderer's showing fails due to a defect in this spec's own rendering/field-plumbing surface, the fix is narrowly scoped to that frontend defect (no extractor, storage, or metric-computation changes)."
        ],
        "negative_probe": "Seed a repo with a nested-branch function alongside a straight-line one and record two ArchitecturalDelta modifications for one node (one 10 days ago, one 60 days ago); run extraction, then assert via GET /repos/:id/graph that complexity(branchy) > complexity(straight-line) and churn_count_30d == 1 (only the in-window delta counts), and assert via the LIVE graph renderer — resolved at run time through getLayout('graph') / the ExplorerView layout path, not a hard-coded ExplorerCanvas mount — that the branchy node renders above the size minimum and the churned node above the border baseline. This probe must fail at the current base: all four extractors hardcode complexity: None / churn_count_30d: 0 in their production node constructors (rust_extractor.rs:160-161, go_extractor.rs:135-136, python_extractor.rs:136-137, typescript_extractor.rs:129-130), the only non-default metric assignments in crates/ are test fixtures (view_query_resolver.rs:2838 inside #[cfg(test)] mod tests at :2811; sqlite/graph.rs:1221 round-trip fixture), and the upsert at sqlite/graph.rs:366-367 writes the extractor defaults back over any stored value on every re-extraction — so every node returns null complexity and zero churn and renders uniformly, i.e. the specced indicators never occur on any renderer. The probe doubles as the ownership guard: it also fails when the realized-model graph-node-metrics implementation has not landed, and that failure must leave this task open (blocked on the external implementation) rather than triggering any local backend implementation to make it pass. The probe's renderer resolution is itself a failure mode: if the shipped default graph view (the renderer the registry actually serves for 'graph') lacks the indicators — e.g. after the ui-layout explorer-surface-gaps rerouting lands while SvgGraphCanvas still carries no complexity/churn encoding — the probe fails even though ExplorerCanvas in isolation renders them, which is exactly the hollow closure this probe must catch.",
        "dependencies": [],
        "existing_task": null
      }
    ]
  },
  "assigned_names": {
    "structural-metrics-verification": "task-306",
    "_publication": "task-307",
    "_base": "58affdc6ee439fe638337bab457dd9a79d903f0c"
  }
}

## Acceptance Criteria

- [ ] Every planned task faithfully implements its assigned canonical requirements.
- [ ] No duplicate active assignment or missing dependency is introduced.
- [ ] No product behavior, spec requirement, coverage status or enforcement is changed.
- [ ] New tasks remain not-started until this backlog publication merges.

## Shipped

Task definitions and explicit bindings only; no product implementation claimed.
