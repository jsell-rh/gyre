---
title: "Verify structural-lens complexity/churn indicators end-to-end on the live graph renderer (backend implementation owned by the realized-model plan's graph-node-metrics task)"
spec_ref: "Spec gap spec-82658b0b4e08c1502a5f0793 / structural-metrics-verification / b3cd47ade570"
depends_on: []
progress: blocked-on-external-implementation
commits: []
---

## Required behavior

explorer-canvas.md §Structural (default) requires 'Complexity/churn indicators: node size reflects complexity, border thickness reflects churn'. Ownership split (fixed and unconditional): the backend implementation — per-extractor cyclomatic complexity computed from parsed source bodies, churn_count_30d accumulated from persisted ArchitecturalDelta history inside the 30-day window, and the SQLite upsert no-overwrite fix at sqlite/graph.rs:366-367 — is owned solely by the realized-model.md plan's 'graph-node-metrics' task (plan_id b26f707c1c6ea89a961ab82e824808bf1625a3e2b3a0c2312981a90a99f3fef3, status planned, /tmp/stage/other-plans/ef4c1074caa37e617259c45b7ee00719.json, covering realized-model section 2d5dce802e2c98aca99b3559). This task NEVER implements or modifies the backend metric computation, the extractors, or the storage layer, and NEVER reroutes the layout registry. This task's scope is this spec's closure concerns only: (1) after the realized-model graph-node-metrics implementation has landed, verify end-to-end that GET /repos/:id/graph returns populated, branching-sensitive complexity (non-null for function/method nodes with bodies, varying with branching structure) and window-filtered churn_count_30d (> 0 only for nodes with a recorded ArchitecturalDelta modification inside the last 30 days). (2) Renderer-agnostic frontend verification: verify that 'node size reflects complexity, border thickness reflects churn' actually occurs on whichever renderer the layout registry serves for the 'graph' layout AT RUN TIME — the verification must resolve the renderer via getLayout('graph') / the live ExplorerView layout path (ExplorerView defaults activeLayout='graph', web/src/components/ExplorerView.svelte:455), never by hard-mounting ExplorerCanvas in isolation. At base 678a8ac3 that live renderer is ExplorerCanvas (Canvas 2D), where the formulas exist (nodeWeight = Math.max(1, n.complexity || 1) at web/src/lib/ExplorerCanvas.svelte:1813-1815; churnThickness = Math.min(4, 1 + (n?.churn_count_30d || 0) * 0.3) at ExplorerCanvas.svelte:3717), but this must not be assumed at run time. (3) Interaction case for the pending ui-layout work (mirroring the realized-model ordering guard): the ui-layout.md plan's 'explorer-surface-gaps' task (plan_id 2c7b116466535165ce9662e74653ac3a62425278b9986424aef39e8b4ad243ed, status planned, earlier audit_at 1795192041, /tmp/stage/other-plans/c587efa4d6aa9c92fe37f3d517f70505.json) item (4) reroutes the 'graph' layout to the SVG d3-force renderer (SvgGraphCanvas via layout-engines.js), which at base 678a8ac3 contains ZERO complexity/churn indicator rendering. If that rerouting has landed when this task runs and the SVG graph path lacks complexity/churn indicator rendering, that is a defect in this spec's own rendering surface to fix narrowly here (add size-by-complexity and stroke-width-by-churn encoding to the SVG graph path only) — or, if the underlying spec conflict is unresolved, report blocked-on-conflict. Never verify against the orphaned Canvas-2D path, never implement backend metrics locally, and never reroute the layout registry. (4) Conflict escalation: record in the task report the genuine spec-vs-spec conflict between explorer-canvas.md §Component Architecture ('ExplorerCanvas (layout dispatcher — Canvas 2D graph for graph/timeline views)') and ui-layout.md §10 ('The Explorer canvas uses SVG for graph/hierarchical/layered layouts'; engine table: graph → d3-force (Svelte wrapper), timeline → d3-scale + Svelte) — mutually incompatible canonical requirements for the same layouts — so a human decides through the spec lifecycle; it must not be settled implicitly by whichever plan lands first. Note: the Component Architecture section's 'satisfied' disposition (79dcb4bb8b305568aa2d80f1) is valid only while ExplorerCanvas actually serves the 'graph' layout. (5) Record section c3a39b320325eff390c4b56c's coverage to reflect the verified state. Ordering guard: if the realized-model graph-node-metrics work has NOT landed when this task runs, the section stays open and the task reports blocked-on-external-implementation — it must not close the section by any local backend implementation, fixture, or client-side metric fabrication. No frontend re-implementation beyond verification on whichever path currently serves 'graph': the only permitted frontend work is (a) the narrow fix if populated metrics reach the API but the live graph renderer fails to show them due to a formula or field-plumbing defect in that renderer (e.g. the field plumbing from the graph API response into node objects), or (b) the narrow SVG-path indicator addition under interaction case (3). No new tests for already-working behavior (goal out-of-scope); the only test work permitted is the end-to-end probe asserting the newly-populated metrics flow through to rendered node size/border on the live graph path. All other Structural sub-requirements (spec-coverage coloring via specBorderColor, causal flow paths, single-click detail panel, double-click drill-down, edge-click relationship detail, right-click context menu with View spec / View provenance / Trace from here, no particles on the structural lens) are independently verified satisfied at base 678a8ac3 and must not be touched.

## Acceptance Criteria

- [ ] This task ships zero changes to the four language extractors, the graph store/adapters, or the upsert path — per-extractor complexity computation, 30-day churn accumulation from ArchitecturalDelta history, and the upsert no-overwrite fix land exclusively via the realized-model.md plan's graph-node-metrics task (plan_id b26f707c1c6ea89a961ab82e824808bf1625a3e2b3a0c2312981a90a99f3fef3).
- [ ] After that implementation lands and a repo is extracted, GET /repos/:id/graph returns function/method nodes whose complexity is non-null and varies with branching structure (a nested-branch function reports higher complexity than a straight-line one) and whose churn_count_30d is > 0 only for nodes with a recorded ArchitecturalDelta modification inside the last 30 days (a node with modifications at 10 and 60 days ago reports 1; an untouched node reports 0).
- [ ] On the renderer that getLayout('graph') / the live ExplorerView layout path actually resolves to at run time, the populated metrics visibly differentiate rendering — node size above the renderer's minimum for branchy code nodes and border thickness above its baseline only for churned nodes (on the Canvas-2D path: nodeWeight > 1 and borderWidth > 1; on an SVG path: individually addressable node elements whose size/stroke-width scale with complexity/churn) — demonstrated with the real populated metric payload served through the graph API path, not a fixture that sets complexity/churn client-side. A probe that mounts ExplorerCanvas directly while the live 'graph' layout resolves to a different renderer does NOT satisfy this criterion (hollow-closure guard).
- [ ] If the ui-layout explorer-surface-gaps d3-force rerouting has landed when this task runs, the indicator verification of acceptance criterion 3 runs on the SVG d3-force path; if that path lacks complexity/churn indicator rendering, the task either fixes it narrowly (SVG graph path indicator encoding only) or reports blocked-on-conflict — it never verifies against the orphaned Canvas-2D ExplorerCanvas path, never implements backend metrics locally, and never reroutes the layout registry.
- [ ] The task report explicitly records the explorer-canvas.md §Component Architecture vs ui-layout.md §10 rendering-technology conflict (Canvas 2D vs SVG d3-force for 'graph'; Canvas 2D vs d3-scale+Svelte for 'timeline') for escalation through the spec lifecycle, rather than resolving it by landing order or unilateral registry changes.
- [ ] Section c3a39b320325eff390c4b56c's coverage row is updated to the verified state only after the end-to-end probe passes against the landed backend on the live graph renderer; while the realized-model implementation is unlanded, the section remains open (the task reports blocked rather than claiming closure).
- [ ] If the populated metrics reach the API but the live renderer's showing fails due to a defect in this spec's own rendering/field-plumbing surface, the fix is narrowly scoped to that frontend defect (no extractor, storage, or metric-computation changes).

## Verification intent

Seed a repo with a nested-branch function alongside a straight-line one and record two ArchitecturalDelta modifications for one node (one 10 days ago, one 60 days ago); run extraction, then assert via GET /repos/:id/graph that complexity(branchy) > complexity(straight-line) and churn_count_30d == 1 (only the in-window delta counts), and assert via the LIVE graph renderer — resolved at run time through getLayout('graph') / the ExplorerView layout path, not a hard-coded ExplorerCanvas mount — that the branchy node renders above the size minimum and the churned node above the border baseline. This probe must fail at the current base: all four extractors hardcode complexity: None / churn_count_30d: 0 in their production node constructors (rust_extractor.rs:160-161, go_extractor.rs:135-136, python_extractor.rs:136-137, typescript_extractor.rs:129-130), the only non-default metric assignments in crates/ are test fixtures (view_query_resolver.rs:2838 inside #[cfg(test)] mod tests at :2811; sqlite/graph.rs:1221 round-trip fixture), and the upsert at sqlite/graph.rs:366-367 writes the extractor defaults back over any stored value on every re-extraction — so every node returns null complexity and zero churn and renders uniformly, i.e. the specced indicators never occur on any renderer. The probe doubles as the ownership guard: it also fails when the realized-model graph-node-metrics implementation has not landed, and that failure must leave this task open (blocked on the external implementation) rather than triggering any local backend implementation to make it pass. The probe's renderer resolution is itself a failure mode: if the shipped default graph view (the renderer the registry actually serves for 'graph') lacks the indicators — e.g. after the ui-layout explorer-surface-gaps rerouting lands while SvgGraphCanvas still carries no complexity/churn encoding — the probe fails even though ExplorerCanvas in isolation renders them, which is exactly the hollow closure this probe must catch.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/explorer-canvas.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `1a0e6db276139ed1d4e01536577e86c9ae7716780322213d7aaef0c2e11d30d2`.

[
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
]

## Shipped

**Status: blocked-on-external-implementation — no closure claimed; section `c3a39b320325eff390c4b56c` remains open.**

Round 74 (repair round from checkpoint `703d1ec0e`; working tree held only the executor's round-reset of this task file). `origin/main` is **unchanged** at `88202d67` since round 68 — already merged into this branch as `6c87f2c3`; no merge/rebase pending. After `git fetch origin --prune` the remote holds only `origin/main` at that same sha (the worker/task-092/093/095/097/116 refs present in round 73 were pruned; main unchanged, so nothing they contained landed). Deterministic evidence saved to `/tmp/stage/review-evidence/task306-round74-probe.txt`.

### Realized-model graph-node-metrics backend — NOT landed (anywhere reachable)

- `git diff 678a8ac3 HEAD --stat` over the four extractors, `sqlite/graph.rs`, `graph_extraction.rs`, `layoutRegistry.js`, and `SvgGraphCanvas.svelte` is **empty** — byte-identical to the discovery base; the four extractors still hardcode `complexity: None` / `churn_count_30d: 0` at the exact discovery lines (rust:160-161, go:135-136, python:136-137, typescript:129-130) and the upsert still overwrites stored values at sqlite/graph.rs:366-367.
- Task registry: `specs/tasks/` still maxes at task-311; **no task file exists for the realized-model plan's graph-node-metrics task** — it has not been dispatched.

Ordering guard applies — no local backend work is permitted, so the end-to-end indicator verification cannot run against populated metrics. The probe fails as designed (every node returns null complexity / zero churn and renders uniformly), which leaves this task open rather than triggering any local implementation.

### ui-layout d3-force rerouting — NOT landed

`getLayout('graph')` still resolves to ExplorerCanvas (Canvas 2D; layoutRegistry.js 'graph' entry maps `component: ExplorerCanvas` with no `engine` selector), and ExplorerView still defaults `activeLayout = $state('graph')` at :455. SvgGraphCanvas is served only for hierarchical/layered, so SVG-path case (3) does not trigger.

### No narrow frontend defect under permitted work (a)

Canvas indicator formulas (ExplorerCanvas.svelte:1814 `nodeWeight = Math.max(1, n.complexity || 1)`; :3717 `churnThickness = Math.min(4, 1 + (n?.churn_count_30d || 0) * 0.3)`) and the API→renderer field plumbing (api/graph.rs:66/:105 serializing `complexity`/`churn_count_30d` per node) verified intact at this head — once the backend populates the metrics, the live Canvas-2D path activates the formulas with zero frontend changes.

### Spec-vs-spec conflict escalation (acceptance criterion 5, recorded for the spec lifecycle)

There is a genuine, unresolved conflict between two canonical specs over the rendering technology for the same layouts: **explorer-canvas.md §Component Architecture** ("ExplorerCanvas (layout dispatcher — Canvas 2D graph for graph/timeline views)") vs **ui-layout.md §10 Rendering Technology** ("The Explorer canvas uses **SVG** for graph/hierarchical/layered layouts"; Layout Engines table: graph → d3-force Svelte wrapper; timeline → d3-scale + Svelte). Mutually incompatible canonical requirements for 'graph' and 'timeline'. This task does not resolve the conflict — no registry change, no unilateral renderer swap; it must be settled by a human through the spec lifecycle, not by whichever plan lands first. The Component Architecture section's 'satisfied' disposition (79dcb4bb8b305568aa2d80f1) is valid only while ExplorerCanvas actually serves the 'graph' layout, which holds at this head but would be invalidated by the pending ui-layout rerouting.

### Round output

Task file at round 74 with truthful `blocked-on-external-implementation`; coverage row round reference updated (row stays open); 0/7 acceptance criteria ticked. Only `specs/` files changed — no Rust, no web, so no formatting or build work was needed (no cold build of an unchanged crate). Per the assignment constraint, large gyre-server builds/test compilation run in the independent verifier; this round runs no server tests, so nothing is deferred beyond that standing constraint.

### Remaining work to unblock

Land the realized-model graph-node-metrics task (per-extractor cyclomatic complexity from parsed source bodies, 30-day churn accumulated from persisted ArchitecturalDelta history, upsert no-overwrite fix at sqlite/graph.rs:366-367), then re-run the end-to-end probe (branchy/straight-line fixture + 10/60-day delta pair) against whichever renderer `getLayout('graph')` resolves to at that run time, and only then update section `c3a39b320325eff390c4b56c`'s coverage row.
