---
title: "HSI Explorer Three Lenses (Structural, Evaluative, Observable)"
spec_ref: "human-system-interface.md §3 Three Lenses"
depends_on:
  - task-065
progress: ready-for-review
coverage_sections:
  - "human-system-interface.md §3 Three Lenses"
commits: ["7bb23bca676396d8434491cef270f6094a92a36f", "0766a16799c0cd414080d98b0cad5f56e561e7ee", "f51574568c6c3ebdd025d42892ddb27a423129eb"]
---

## Spec Excerpt

Every Explorer view can be overlaid with one of three lenses:

| Lens | What It Shows | Data Source |
|---|---|---|
| **Structural** (default) | Interfaces, boundaries, data shapes, dependencies | Knowledge graph nodes + edges |
| **Evaluative** | Test results, gate outcomes, spec assertion status | Gate results, spec approval ledger |
| **Observable** (future) | SLIs, error rates, latency per endpoint | Production telemetry (not yet implemented) |

The lenses compose — you can view the domain model (structural) with test coverage overlay (evaluative) to see "which types have tests?"

## Implementation Plan

1. **Lens selector in Explorer toolbar:**
   - Add a lens toggle/dropdown to the ExplorerCanvas toolbar
   - Three options: Structural (default), Evaluative, Observable (disabled/greyed out with "Coming soon")
   - Lens selection persists in URL query params (`?lens=evaluative`)

2. **Structural lens (default):**
   - This is the baseline: nodes show types, endpoints, modules with their relationships
   - Data from knowledge graph (existing `GET /api/v1/repos/:id/graph`)
   - Node styling: categorical colors by node_type
   - Edge styling: dependency arrows, interface implementation edges

3. **Evaluative lens overlay:**
   - When activated, overlay gate results and spec status onto existing nodes
   - **Test coverage:** nodes show coverage percentage (green/yellow/red heatmap)
   - **Gate status:** edges colored green (pass) / red (fail) based on most recent gate results
   - **Spec assertion status:** nodes with spec assertions show pass/fail badges
   - **Spec approval:** spec-linked nodes show approval status icon (✓ Approved, ⏳ Pending, ✗ Rejected)
   - Data sources:
     - Gate results from gate evaluation records
     - Spec approval from spec approval ledger
     - Test coverage from `GraphNode.test_coverage` field

4. **Lens composition:**
   - Evaluative overlays compose on top of Structural base
   - The Structural lens provides the graph layout; Evaluative adds visual encodings (colors, badges)
   - A node can show its type (structural) AND its test coverage (evaluative) simultaneously

5. **Observable lens (placeholder):**
   - Render as disabled in the lens selector
   - Tooltip: "Observable lens requires production telemetry integration (future)"
   - When §10 Design Constraints are met, this lens will show SLIs, error rates, latency

6. **Lens-specific node rendering:**
   - Create a lens rendering pipeline that accepts a base node and applies lens-specific decorations
   - Structural: type icon, label, edge arrows
   - Evaluative: coverage badge, gate status ring, spec assertion icon
   - Observable (future): latency badge, error rate indicator

## Acceptance Criteria

- [ ] Lens selector visible in Explorer toolbar (Structural, Evaluative, Observable)
- [ ] Structural lens is the default and shows knowledge graph nodes/edges
- [ ] Evaluative lens overlays test coverage, gate status, and spec approval on nodes
- [ ] Test coverage shown as color-coded badge (green/yellow/red)
- [ ] Gate status shown as edge colors (green=pass, red=fail)
- [ ] Spec approval status shown as icon on spec-linked nodes
- [ ] Lenses compose: Evaluative adds decorations on top of Structural layout
- [ ] Observable lens is disabled with "Coming soon" tooltip
- [ ] Lens selection persisted in URL query params (`?lens=evaluative`)
- [ ] Switching lenses does not re-layout the graph (only changes visual encodings)
- [ ] `npm test` passes in `web/`

## Agent Instructions

Read `specs/system/human-system-interface.md` §3 "Three Lenses" for the spec. Also read `specs/system/explorer-canvas.md` §3 for lens definitions. The ExplorerCanvas component is at `web/src/lib/ExplorerCanvas.svelte`. The knowledge graph data comes from `GET /api/v1/repos/:id/graph`. Gate results may be available from existing gate endpoints. Spec approval data is in the spec approval ledger. The `GraphNode` struct should have a `test_coverage` field — check if it's populated. For the Evaluative lens, you need gate result data per graph node — this may require joining gate results with the graph node's spec_ref. The Observable lens is explicitly future work — just add the disabled option. Check `specs/system/explorer-canvas.md` §4 "Lens Definitions" for any additional lens specification details.

## Shipped

The three-lens architecture (HSI §3) is complete in the Explorer: Structural
(default), Evaluative (real overlays from gate results, the spec approval
ledger, and `GraphNode.test_coverage`), and Observable (disabled placeholder
per spec "future"). The lens substrate (toolbar toggle, structural base,
trace-particle evaluative, observable banner) already existed from task-065;
this task closed the remaining HSI §3 gaps: the evaluative overlays for
test coverage / gate outcomes / spec approval, and URL persistence of the
lens selection.

### Shipped behavior

- **Lens decoration pipeline** (`web/src/lib/lens-rendering.js`): pure
  functions that compose decorations on a base node/edge. Coverage badge
  classification (green >= 70%, yellow >= 30%, red below, from
  `GraphNode.test_coverage`), the node → governing-spec join (`spec_path`,
  N:M `spec_paths`, GovernedBy edge targets), gate status resolution (a
  failing gate on any governing spec fails the node), approval status
  resolution from ledger records (active → approved; rejected → rejected;
  revoked → pending again; spec-linked with no record → pending), and the
  URL helpers (`?lens=evaluative`, structural default drops the param).
- **Evaluative node decorations** (ExplorerCanvas `drawLeafNode`): coverage
  badge (green/yellow/red, % label), gate status ring (green/red outline),
  and approval icon (✓/⏳/✗) — all drawn on top of the unchanged
  structural fill, spec border, and label (lenses compose; the overlay
  never replaces the structural rendering).
- **Gate status edge colors** (ExplorerCanvas `drawEdges`): edges leaving a
  node governed by a spec with a concluded gate run are colored green
  (pass) / red (fail), with the latest result per gate deciding the
  outcome. Precomputed per node so the render loop stays O(1) per edge.
- **Data loading** (ExplorerView `loadEvaluativeData`): gate outcomes
  joined from this repo's recent MRs (queried with `repository_id` —
  `GET /api/v1/merge-requests` only accepts repository_id /
  workspace_id / status — with a 50-MR recency window applied
  client-side; `spec_ref` "path@sha" → per-spec most recent MR with
  concluded gates) and the spec approval ledger
  (`GET /api/v1/specs/approvals`), both best-effort, non-blocking.
- **URL persistence**: lens selection is restored from `?lens=evaluative`
  deep links on mount and synced via `history.replaceState` on switch
  (Back is not polluted with lens toggles; other params and the `#drill=`
  hash are preserved).
- **Ledger API**: `SpecApprovalResponse` serializes `rejected_at` /
  `rejected_reason` so a rejected approval (✗) is distinguishable from a
  revoked one (back to ⏳ pending) — server round-trip test included.
- **Observable lens**: disabled (`aria-disabled`, tooltip "requires
  production telemetry integration") per spec — future work.

### Test evidence

- `web/src/__tests__/lens-rendering.test.js` — 27 unit tests: coverage
  thresholds and badge colors, node→spec join (spec_path, spec_paths,
  GovernedBy targets, soft-deleted edges ignored), gate pass/fail
  resolution (fail wins), approval status from ledger history, composed
  decorations, URL param read/write.
- `web/src/__tests__/ExplorerCanvas.test.js` — 8 lens component tests
  (within the 157-test suite): evaluative legend entries, coverage badge
  drawn green at 85% (and NOT in structural lens), gate edge/ring colors
  (differential red/green stroke assertions), approval icons ✓/⏳/✗ (and
  absent in structural lens), and lens switching preserves node label
  positions exactly (no re-layout).
- `web/src/__tests__/ExplorerViewLensUrl.test.js` — 6 tests: structural
  default has no param, switching to evaluative persists
  `?lens=evaluative`, switching back drops it, deep-link restore, the
  observable lens disabled state + tooltip, and the evaluative gate-data
  load querying `mergeRequests({ repository_id })` (repo-scoped, not the
  server-ignored `repo_id`).
- `cargo test -p gyre-server --lib api::gates` — 7 passed (incl.
  `list_spec_approvals_includes_rejection_fields`).

### Review repair: repo-scoped gate-status query (round 16)

The review round (repair id `4bc383b9ab8241cfafa02ab4af25c73e`) found
that `loadEvaluativeData` (from product commit 0766a167) queried
`api.mergeRequests({ repo_id, per_page })`, but `GET
/api/v1/merge-requests` only accepts `repository_id` / `workspace_id` /
`status` (`ListMrsQuery`, crates/gyre-server/src/api/merge_requests.rs) —
`repo_id` and `per_page` are silently ignored, so the handler's unscoped
arm returned ALL tenant MRs. Because the gate-status join is keyed by
spec path alone, gate outcomes from OTHER repos' MRs were attributed to
the viewed repo's nodes in any multi-repo tenant with shared spec paths
(every repo carries specs/system/*.md), including false-green gate
rings/edges. Fixed in product-surface commit
`f51574568c6c3ebdd025d42892ddb27a423129eb`:

- **Query with `repository_id`** (as every other frontend call site —
  DetailPanel, RepoMode, RepoSettings, ExplorerCodeTab — already does),
  so the MR list is scoped to the viewed repo and the path-keyed join
  only ever sees this repo's gate outcomes.
- **50-MR recency window applied client-side** (`.slice(0, 50)` after the
  most-recent-first sort): the endpoint has no `per_page` support, so the
  originally intended window is now actually enforced.
- **Regression test** (`ExplorerViewLensUrl.test.js`, "loads evaluative
  gate data repo-scoped"): asserts `api.mergeRequests` is called with
  `{ repository_id }`. Verified differential — the test fails against
  the previous `repo_id` query (1 failed | 5 passed when the fix is
  stashed) and passes with it, so the param contract is now guarded.
- **Out of scope, left untouched**: the identical `repo_id` misuse in
  `loadTraceData` (ExplorerView.svelte, pre-existing from task-065's
  #627, flagged by the reviewer as outside this candidate) — it needs
  its own attributed fix.

Focused probes on the current head after `npm ci` (logs under
/tmp/stage/review-evidence): `npx vitest run` on ExplorerViewLensUrl —
6/6 passed (exit 0); lens-rendering + ExplorerViewLargeGraph +
ExplorerCanvas + ExplorerViewShortcuts — 192/192 passed (exit 0).
Prior repair rounds (rebase resolution be494b399, gates.rs lint fix
7bb23bca) are retained in git history. Acceptance criteria left
unchecked for independent review.
