---
title: "HSI Explorer Default Views (Boundary, Spec Realization, Change, System Trace)"
spec_ref: "human-system-interface.md §3 Default Views"
depends_on:
  - task-065
progress: ready-for-review
coverage_sections:
  - "human-system-interface.md §3 Default Views (Automatic, No LLM)"
commits: ["2620dd8cf17dcfd2fde2972b76f345dd77116b58", "8407bd1cfe42fc1ab324cb7bffd8275bce4c5acd", "1a50c2276ef6c6c7a6e5e4ec092c112fa7703e45", "8e025c2570ee8df5919dfc4b298d9c48978c599e", "3463e2858d8b2cfc022dca05140aa44f041a3d15", "cd37fa928fe948f0ca2b813fb5ab491771b75796", "d4e72e32cd9fad766a2c54973b04d631ab179a3f"]
---

## Spec Excerpt

Default views are generated automatically from the knowledge graph. No LLM needed.

**Boundary View (C4 Progressive Drill-Down):** The default Explorer view. Each level answers "what is this made of?"
- Level 1 — Workspace scope (C4 Container): repos and external deps
- Level 2 — Container (Repo scope): crates/packages within a repo
- Level 3 — Component (Crate scope): modules and public interfaces
- Level 4 — Code (Module scope): types, functions, fields, methods

**Spec Realization View:** Specs on the left, linked graph nodes on the right. Shows "what implements this spec?"

**Change View:** What changed since last visit, at the structural level. Powered by architectural deltas. Uses `last_seen_at` from `X-Gyre-Last-Seen` response header. Queries `GET /repos/:id/graph/timeline?since=<last_seen_at>`.

**System Trace View:** What actually happened — SDLC timeline for an MR. Assembled from existing data (spec lifecycle triggers, agent spawn/complete, conversation turns, gate execution, graph extraction, merge queue events). Endpoint: `GET /api/v1/merge-requests/:id/timeline`.

## Implementation Plan

1. **Boundary View (C4 progressive drill-down):**
   - Level 1 (workspace scope): fetch repos via `GET /api/v1/workspaces/:id/repos` + dep info, render as card grid showing repo name, type count, endpoint count
   - Level 2 (repo scope): fetch graph, group nodes by crate/package module, show dependency arrows
   - Level 3 (crate scope): show modules with public interface summary (pub types, pub functions)
   - Level 4 (module scope): show individual types, functions, fields, methods with spec linkage and churn info
   - Click to drill down, breadcrumb to drill up

2. **Spec Realization View:**
   - Two-panel layout: spec list on left, linked graph nodes on right
   - Left panel: specs with status badges (Approved ✓, Pending, etc.) and task progress
   - Right panel: graph nodes linked to the selected spec via `GovernedBy` edges
   - Links to read/edit spec and view implementation details

3. **Change View:**
   - Read `X-Gyre-Last-Seen` header from recent responses (client caches this value)
   - Query `GET /repos/:id/graph/timeline?since=<last_seen_at>`
   - Render changes as a list: Added (+), Modified (~), Removed (-) nodes
   - Each entry shows: node name, type, containing module, spec reference, authoring agent
   - Clickable entries to navigate to the node in the Boundary View

4. **System Trace View (SDLC timeline):**
   - Render as a vertical timeline
   - Each event row: timestamp, event type, detail (clickable references)
   - Event types: SpecLifecycleTrigger, AgentSpawned, ConversationTurn, GitPush, GateResult, GraphExtraction, MergeQueueEnqueued, Merged
   - Gate results expandable to show individual gate pass/fail
   - Links: [View Conversation], [View Diff], [View Attestation]
   - Data from `GET /api/v1/merge-requests/:id/timeline` (this endpoint may need to be implemented)

5. **View selector in Explorer:**
   - Add a view switcher (dropdown or tabs) in the Explorer toolbar: Boundary | Spec Realization | Change | System Trace
   - Boundary is the default view

## Acceptance Criteria

- [ ] Boundary View renders at all 4 C4 levels with click-to-drill-down
- [ ] Level 1 shows repos as cards with type/endpoint counts
- [ ] Level 4 shows spec linkage and modification metadata
- [ ] Spec Realization View shows spec-to-code mapping with two-panel layout
- [ ] Change View shows added/modified/removed nodes since last visit
- [ ] Change View uses `last_seen_at` timestamp correctly
- [ ] System Trace View renders SDLC timeline for an MR
- [ ] Timeline events are clickable (link to conversations, diffs, attestations)
- [ ] View selector in Explorer toolbar allows switching between views
- [ ] `npm test` passes in `web/`

## Agent Instructions

Read `specs/system/human-system-interface.md` §3 "Default Views" for the full specification of all four views. The ExplorerCanvas component is at `web/src/lib/ExplorerCanvas.svelte`. The knowledge graph data comes from `GET /api/v1/repos/:id/graph`. Check if the timeline endpoint `GET /api/v1/merge-requests/:id/timeline` exists (it may need to be created). The Change View uses `GET /repos/:id/graph/timeline` — verify this endpoint exists. For the `X-Gyre-Last-Seen` header, check if the `last_seen_at` middleware in the server sets this response header. Architectural deltas are defined in `specs/system/realized-model.md` §3. The graph timeline may use `GET /repos/:id/graph/diff` which already exists.

## Shipped

All four HSI §3 default views are generated automatically from real data
(knowledge graph, architectural deltas, SDLC records) — no LLM involved —
behind a view selector (tablist) in the Explorer control bar, with Boundary
as the default.

### Boundary View (C4 progressive drill-down)

- **Level 1 (workspace scope)**: the workspace repo cards now carry
  type/endpoint counts computed from `GET /workspaces/:id/graph` (the
  aggregated knowledge graph; types = type/interface/trait/class/enum
  nodes, endpoints = endpoint nodes) plus cross-repo dependency chips from
  `GET /repos/:id/dependencies` (best-effort — failures leave cards
  without counts instead of breaking the repo list).
- **Levels 2–4**: the existing ExplorerCanvas drill-down (packages →
  modules → code items via Contains edges, breadcrumb drill-up, animated
  camera transitions) — already pinned by
  `ExplorerCanvasDrillDown.test.js` (packages → modules → types →
  methods/fields). Level 4 spec linkage and modification metadata
  (spec_path/confidence, last-modified SHA/agent/time, churn, complexity)
  render in the node detail panel.
- Clicking a Level 1 card selects the repo and drills into its graph.

### Spec Realization View

Two-panel `SpecRealizationView.svelte`: specs on the left (from
`GET /specs`) with status badges (approved ✓ / pending / draft / …),
linked-node counts and task progress (`GET /specs/:path/progress`);
realization nodes on the right, linked via **GovernedBy edges**
(canonical, target resolved through spec nodes or edge metadata),
`spec_path`, and `spec_paths`. "Read / Edit Spec" opens the inline spec
editor; clicking a node navigates to it in the Boundary View. Spec nodes
themselves are excluded from realization.

### Change View

`ChangeView.svelte` queries `GET /repos/:id/graph/timeline?since=…` and
renders Added (+) / Modified (~) / Removed (−) entries parsed from each
delta's `delta_json` (nodes_added / nodes_modified with field_changes /
nodes_removed), showing name, type, containing module (Contains-edge
lookup, qualified-name fallback), spec reference, authoring agent and
timestamp. Modified entries expand to per-field old → new values;
resolvable entries navigate to the node in the Boundary View.

The `since` value implements the spec's client-side header cache: new
`web/src/lib/last-seen.js` captures the `X-Gyre-Last-Seen` response
header centrally in `api.js`'s `request()`, persists it, and snapshots
the pre-visit value once per page load — so "since last visit" is
anchored to the *previous* visit even though the current visit's first
response already stamps a new value (60s server debounce). First-ever
visit falls back to 24h ago, consistent with the Briefing's `since`
fallback.

### System Trace View

`SystemTraceView.svelte` renders the MR SDLC timeline from the existing
`GET /api/v1/merge-requests/:id/timeline` endpoint: MR selector (repo's
MRs), vertical timeline of typed events (SpecLifecycleTrigger,
AgentSpawned, AgentCompleted, GitPush, GateResult, GraphExtraction,
MergeQueueEnqueued, Merged) with timestamps and human-readable details,
gate summary counts, and expandable gate rows showing individual gate
pass/fail. `[View Conversation] [View Diff] [View Attestation]` open
inline sections (conversation SHA resolved from the attestation bundle →
`GET /conversations/:sha`; diff; attestation summary with gate results).

### Backend fix

`parse_delta_counts` in `api/mr_timeline.rs` only accepted bare-number
`nodes_added`/`nodes_modified` and probed nonexistent `added`/`modified`
keys, but `graph_extraction.rs` serialises **arrays** — every real
GraphExtraction event reported 0/0. Now both shapes are counted; the unit
test pins the producer shape.

### Rebase integration (task-078 Explorer Phase 4)

Rebased onto base a4ebdf6f5 (task-078: streaming, keyboard shortcuts,
performance). Single conflict in `ExplorerView.svelte` resolved by keeping
both sides: the `explorerViewMode === 'boundary'` canvas wrapper from this
task plus task-078's `windowEscape={false}` Escape-cascade comment/prop —
ExplorerView owns the window-level Escape cascade, and the canvas now
renders only in Boundary mode behind the view selector. All four view
panels coexist with task-078's shortcuts, streaming and performance
warnings (merge commit 531ca48b7).

### Rebase integration (task-083 Canonical Navigation)

Merged base 637be7187 (task-083: scope-aware content routing, Graph|Briefing
arch sub-tabs at repo scope) into the candidate (merge commit cd37fa928,
integration commit d4e72e32). Text conflict in `ExplorerView.svelte` import
block resolved by keeping task-083's import ordering (NodeDetailPanel after
ExplorerCanvas, Briefing import) plus this task's three view-component
imports — the duplicate NodeDetailPanel import on the candidate side was
dropped (Svelte compile error). `api.js` auto-merged cleanly and retains
both sides' changes (`captureLastSeenHeader` in `request()` and task-083's
`repoId` param on `getWorkspaceBriefing`). Semantically the two tablists
compose: the Briefing sub-tab branch precedes the graph branch, and within
the Graph sub-tab the HSI §3 view selector governs (canvas only in Boundary
mode). `ExplorerDefaultViews.test.js` now scopes its tab queries to the
`explorer-view-selector` tablist — the assertion still pins exactly the
four view tabs in order, it just no longer assumes the Explorer is the only
tablist at repo scope.

### Test evidence

- `web/src/__tests__/last-seen.test.js` — header capture, older/equal
  ignore, pre-visit snapshot anchoring, 24h fallback, corrupt storage.
- `web/src/__tests__/ChangeView.test.js` — `since` propagation, +/~/−
  rendering with full metadata, Boundary navigation on click, field-change
  expansion, empty/error states.
- `web/src/__tests__/SpecRealizationView.test.js` — two-panel layout,
  status badges + counts, linkage via all three sources, task progress,
  spec switching, spec-editor opening, empty state.
- `web/src/__tests__/SystemTraceView.test.js` — timeline rendering, gate
  expansion, View Conversation/Diff/Attestation sections, MR switching,
  empty state.
- `web/src/__tests__/ExplorerDefaultViews.test.js` — view selector tabs
  with Boundary default, per-view swapping (canvas ↔ panels), Change View
  `since` wiring end-to-end through ExplorerView, and Level 1 repo-card
  counts/dependency chips.
- `cargo test -p gyre-server --lib api::mr_timeline` — 13 passed
  (including the new producer-shape test).
- Full `npm test` in `web/` after the task-078 rebase: 76 files, 1771 passed,
  41 skipped (includes task-078's ExplorerViewShortcuts, causal-trace and
  streaming suites alongside the default-view suites above).
- After the task-083 merge: focused vitest run of the ten merge-relevant
  suites (ExplorerDefaultViews, ExplorerViewScope, ExplorerViewShortcuts,
  ExplorerCanvasDrillDown, last-seen, ChangeView, SpecRealizationView,
  SystemTraceView, Briefing, AppShell) — 154 passed.

### CI repair — explorer visual baselines (run 42455312769)

The 11 E2E explorer-visual regressions at candidate head 61e430cbf were
baseline drift, not a rendering defect: the spec-required view selector
tablist makes the concept search bar 2px taller, so `.treemap-canvas-area`
renders 2px shorter (919x397 → 919x395, 579x335 → 579x333, 579x298 →
579x296) and the size mismatch is a hard `toHaveScreenshot` failure.
Commit 8407bd1cf replaced the 11 affected baselines with the CI run's own
actual screenshots (authoritative gate-environment rendering; same repair
as task-083 c69d9fd54). No test logic, locator, or tolerance change; the
toolbar, blast-radius-tiered and full-page baselines are untouched.

Local verification: reproduced the exact CI failure set (11 failed, all
2px size mismatch); local rendering is pixel-identical to the CI actuals
for zoom-level-0 and within 0.20%–1.28% differing pixels for the rest
(gate tolerance 2%); full explorer-visual suite 15/15 passed against the
new baselines; mutation probe — building a dist with the
`explorer-view-tabs` block removed fails zoom_level_0 (919x395 expected vs
919x397 received), so the baselines genuinely pin the new toolbar.
Evidence retained under the attempt's review-evidence directory.

### Baseline regeneration after the task-083 merge

The task-083 merge re-drifted the same 11 baselines: the repo-scope header
now also carries the Graph|Briefing arch sub-tabs row, so
`.treemap-canvas-area` renders 34px shorter (919x395 → 919x361, 579x333 →
579x299, 579x296 → 579x262, 579x466 → 579x432 — same widths, pure vertical
offset; local run at merge head, 11 failed / 4 passed, every failure a
uniform size mismatch). Commit d4e72e32 regenerated the 11 baselines
locally against the merged UI (forced-font fixtures; server built from
merged sources). Post-update full explorer-visual suite: 15/15 passed.
Mutation probe repeated at the merged head: hiding the view-selector
tablist fails zoom_level_0 (919x361 expected vs 919x363 received), so the
regenerated baselines pin the merged toolbar with both tablists. The
toolbar, blast-radius-tiered and full-page baselines are unchanged.
