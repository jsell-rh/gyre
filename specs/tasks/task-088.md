---
title: "HSI Explorer Default Views (Boundary, Spec Realization, Change, System Trace)"
spec_ref: "human-system-interface.md §3 Default Views"
depends_on:
  - task-065
progress: not-started
coverage_sections:
  - "human-system-interface.md §3 Default Views (Automatic, No LLM)"
commits: []
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
