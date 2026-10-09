---
title: "Specs and Inbox view layouts — spec list, inbox cards, briefing narrative"
spec_ref: "ui-layout.md §6-§8"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "ui-layout.md §6. Specs View Layout"
  - "ui-layout.md §7. Inbox Layout"
  - "ui-layout.md §Item Structure"
  - "ui-layout.md §Action Buttons per Item Type"
  - "ui-layout.md §8. Briefing Layout"
commits: ["56395200e6d791faefa6eb588b2a6fc96d89f734", "722c9c29c5114fcc5bd1963f94ee054e4a42add9", "1f31ed07d12007bf3609e9a7c16378223096c0ab", "3c3d16a72e9ce48a87682cfd6a6f557b12f605db", "fd3c74d138cf11a94d2ee015b1f3c973e68d26ed", "7ce04d6903b99a04934528e856778d59e0712901"]
---

## Spec Excerpt

**Specs View Layout (§6)**: Full-Width layout. Tenant/workspace scope: sortable table (Path, Status, Kind, Owner, Last Updated). Repo scope: spec list with implementation progress (path, status, task progress bar). Click spec → detail panel with tabs: Content, Edit, Progress, Links, History. New spec creation via `[+ New Spec]` → Editor Split. Data from `GET /api/v1/specs?workspace_id=` and per-spec endpoints.

**Inbox Layout (§7)**: Cards with priority badge, description, workspace attribution, timestamp. Accordion expand shows full context (uncertainty text, related spec, agent info) with action buttons per item type (10 priority levels). Expanded cards include inline action buttons: Respond, View Spec, Approve, Reject, Retry, Override, etc.

**Briefing Layout (§8)**: Full-width narrative with sections: COMPLETED, IN PROGRESS, CROSS-WORKSPACE, EXCEPTIONS, METRICS. Action buttons inline in narrative. Entity references (spec, agent, MR names) are clickable → open detail panel. "Ask about this briefing" chat input at bottom.

## Implementation Plan

1. **Specs View compliance** (`SpecDashboard.svelte`):
   - Verify spec list matches spec: Path, Status, Progress bar, Last Activity columns
   - Detail panel tabs: Content, Edit, Progress, Links, History
   - `[+ New Spec]` button opens Editor Split (depends on task-172)
   - `?owner=me` toggle and status filters

2. **Inbox Layout compliance** (`Inbox.svelte`):
   - Verify card structure: priority badge, description, workspace/repo attribution, timestamp
   - Accordion expand with full context
   - Action buttons per all 10 priority levels (per HSI §8 priority table)
   - Workspace attribution badges at tenant scope

3. **Briefing Layout compliance** (`Briefing.svelte`):
   - Verify section structure: COMPLETED, IN PROGRESS, CROSS-WORKSPACE, EXCEPTIONS, METRICS
   - Entity references as clickable links → detail panel
   - "Ask about this briefing" chat input
   - Time range selector

4. **Tests**:
   - Spec list renders with progress bars
   - Inbox accordion expands with correct action buttons
   - Briefing narrative renders all sections

## Acceptance Criteria

- [x] Specs view renders sortable table with implementation progress
- [x] Spec detail panel has Content/Edit/Progress/Links/History tabs
- [x] Inbox cards show priority badge, description, attribution, timestamp
- [x] Inbox accordion shows action buttons for all 10 priority levels
- [x] Briefing renders COMPLETED/IN PROGRESS/CROSS-WORKSPACE/EXCEPTIONS/METRICS sections
- [x] Entity references in briefing are clickable (open detail panel)
- [x] Tests pass

## Agent Instructions

Read `ui-layout.md` §6-§8 for the exact layout specifications. Also reference `human-system-interface.md` §8 for the full inbox priority table (10 item types with their action buttons). The existing `SpecDashboard.svelte`, `Inbox.svelte`, and `Briefing.svelte` are the starting points — this task is about aligning them with the spec's layout requirements, not building from scratch. Check the data source endpoints mentioned in the spec: `GET /api/v1/specs`, `GET /api/v1/users/me/notifications`, `GET /api/v1/workspaces/:id/briefing`.

## Implementation Notes (for review)

**§6 Specs View** — verified existing `SpecDashboard.svelte` against spec: sortable
table (path/status columns tested), status + kind filter pills, `?owner=me`
toggle, repo-scope progress bars with ARIA attributes, `[+ New Spec]` modal
(Editor Split), row click opens the spec detail panel. Spec detail tabs
(Content/Edit/Progress/Links/History) live in `DetailPanel.svelte`
(`computeTabs`) and default spec entities to the Content tab. Covered by
`SpecDashboard.test.js` (34 tests).

**§7 Inbox** — `Inbox.svelte`:
- All 10 HSI §8 priority types now have both component buttons and tests:
  P4 cross_workspace_change (Review Changes/Dismiss), P6 meta_spec_drift
  (View Results/Adjust Meta-spec → navigates to agent rules), P9
  spec_assertion_failure (View Code → repo detail panel/Update Spec), P10
  suggested_link (Confirm/Dismiss — label renamed from "Accept" to match the
  spec's "Confirm") were implemented but untested; tests added.
- **P2 inline spec diff (new)**: expanding a `spec_approval` card now fetches
  the spec-edit/* MR diff via `GET /merge-requests/:id/diff` and renders it
  inline with `SpecDiffView` so the human reviews the change before approving
  (spec: "The expanded accordion shows the spec diff … and a 'View Full Spec'
  link that opens the detail panel Content tab"). The "Open Spec" button was
  renamed "View Full Spec"; it opens the spec detail panel which defaults to
  the Content tab. Regression tests cover the diff fetch/render and the link.
- Legacy body-less spec_approval notifications (title-only, from before the
  server populated the body) still approve/reject by parsing the title and
  fetching the SHA from the spec ledger.
- Tenant-scope workspace attribution badge covered by a new test.
- The api mock in `Inbox.test.js` gained `approveSpec`/`revokeSpec`/`getSpec`
  /`mrDiff`/`workspaces` — the prior round's `beforeEach` referenced
  `approveSpec` before it existed in the mock factory, which failed all 41
  tests; fixed.

**§8 Briefing** — verified existing `Briefing.svelte`: all five sections
render from API data, entity reference links (spec/agent/MR) open the detail
panel, inline action buttons, "Ask about this briefing" chat, time-range
selector (last-visit/24h/custom). Covered by `Briefing.test.js` (29 tests).

**Backend (from earlier commits on this branch, re-verified this round)** —
`save_spec` fans the SpecPendingApproval notification out to workspace
Admin/Developer/Owner members (falling back to the system account) with a
structured body (spec_path, spec_sha blob SHA, mr_id, branch, repo_id) that
the Inbox actions consume; `cargo check -p gyre-server` passes.

**Known pre-existing failures (not this task)** —
`ExplorerCanvas-performance.test.js` timing-sensitive tests and one
`ExplorerCanvas.test.js` ghost-overlay test fail under full-suite load but
pass standalone; reproduced identically on the merge-base baseline
(389267a) in an isolated worktree, so they predate this branch.
