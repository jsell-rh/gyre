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
commits: ["c424ae307c4d282c095c4af28596acf34fc6cdab", "4797d0f456c06b1090d624ee98ede31a8847cc95", "a9aca60e1ff6dc564bdfc6a53d0f1572f40ceed7", "351bd57dcc59872bafe7f238dc9bb64c9f662cf3", "d6dfb7da45bbd862f5a7249c214c75e6ad475c48", "56395200e6d791faefa6eb588b2a6fc96d89f734", "722c9c29c5114fcc5bd1963f94ee054e4a42add9", "1f31ed07d12007bf3609e9a7c16378223096c0ab", "3c3d16a72e9ce48a87682cfd6a6f557b12f605db", "fd3c74d138cf11a94d2ee015b1f3c973e68d26ed", "7ce04d6903b99a04934528e856778d59e0712901"]
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

- [ ] Specs view renders sortable table with implementation progress
- [ ] Spec detail panel has Content/Edit/Progress/Links/History tabs
- [ ] Inbox cards show priority badge, description, attribution, timestamp
- [ ] Inbox accordion shows action buttons for all 10 priority levels
- [ ] Briefing renders COMPLETED/IN PROGRESS/CROSS-WORKSPACE/EXCEPTIONS/METRICS sections
- [ ] Entity references in briefing are clickable (open detail panel)
- [ ] Tests pass

## Agent Instructions

Read `ui-layout.md` §6-§8 for the exact layout specifications. Also reference `human-system-interface.md` §8 for the full inbox priority table (10 item types with their action buttons). The existing `SpecDashboard.svelte`, `Inbox.svelte`, and `Briefing.svelte` are the starting points — this task is about aligning them with the spec's layout requirements, not building from scratch. Check the data source endpoints mentioned in the spec: `GET /api/v1/specs`, `GET /api/v1/users/me/notifications`, `GET /api/v1/workspaces/:id/briefing`.

## Shipped

Recovered from an interrupted assignment (checkpoint 30f286baf33140dd94b1091c72dd0f6e): the prior
rounds had built the full §6-§8 surface; this round audited every acceptance criterion against the
spec, fixed one real defect, and closed the missing test coverage for the server-side change.

**§6 Specs View** (`SpecDashboard.svelte` + `DetailPanel.svelte`): workspace/tenant scope renders
the sortable table (Path, Status, Kind, Owner, Last Updated) with status/kind filter pills and the
`owner=me` toggle; repo scope renders the progress table (path, status badge, `N/M tasks`
progressbar, last updated) with per-spec `GET /specs/:path/progress` rollups. Row click opens the
spec detail panel whose tab set is Content/Edit/Progress/Links/History (DetailPanel `computeTabs`,
each tab lazily backed by its real endpoint: content, progress, links, history-repo). `[+ New
Spec]` opens the editor modal (markdown editor + preview split) that calls `POST
/repos/:repo_id/specs/save` on a `spec-edit/*` branch.

**§7 Inbox** (`Inbox.svelte`): cards render priority badge (`P1`-`P10`), title/subtitle, type
badge, workspace-name badge at tenant scope, and relative timestamp; accordion expand reveals the
full context (message, gate name/command, conflict nodes, related spec/agent/MR reference links)
and the per-type action row. All 10 HSI §8 priority types render their specced buttons with real
API calls: P1 Respond/View Spec/Open in Explorer/Dismiss, P2 Approve/Reject/Open Spec + inline
spec-edit/* MR diff (`mrDiff`) before approving, P3 View Diff/View Output/Retry/Override (`submitReview`
approved)/Close MR (`mrStatus` closed), P4 Review Changes/Dismiss, P5 View Both/Pick A/Pick B
(`revertMr` of the losing merged MR)/Reconcile (`createTask` reconciliation task), P6 View
Results/Adjust Meta-spec, P7 Increase Limit/Pause Work (repo merge-queue pause or workspace-wide
`pause_requested` agent messages), P8 Increase Trust/Dismiss, P9 View Code/Update Spec, P10
Confirm/Dismiss. PascalCase server wire types normalize to snake_case before rendering. Dismissed
items hidden by default with the "Show Dismissed" toggle.

**Server fan-out (HSI §8 P2)** (`specs_assist.rs save_spec`): the spec-pending-approval
notification now fans out to every workspace Owner/Admin/Developer member (previously a single
hardcoded "system" recipient) with a structured body (spec_path, spec_sha blob SHA, mr_id,
mr_title, branch, repo_id) that the Inbox approve action consumes directly; the "system" fallback
fires only when a workspace has no eligible members.

**§8 Briefing** (`Briefing.svelte`): full-width narrative with the five section headers
(COMPLETED / IN PROGRESS / CROSS-WORKSPACE / EXCEPTIONS / METRICS), inline action buttons per
item (Respond to agent, View spec, Review changes/Dismiss, View Diff/View Output/Override/Close
MR), entity references as clickable links opening the detail panel (spec, agent, MR), time-range
selector (last visit/24h/7d/30d/custom with since epoch passed to `GET
/workspaces/:id/briefing`), and the "Ask about this briefing" InlineChat at the bottom. Tenant
scope aggregates all workspaces with per-item workspace badges.

**Defect fixed this round**: `web/src/locales/en.json` was missing 6 `decisions.*` keys referenced
by Inbox.svelte (view_mr, view_agent, resume_queue, resuming_queue, queue_resumed,
resume_failed) — those buttons rendered raw key text. Key-extraction audit now reports zero
missing keys for Inbox/Briefing/SpecDashboard.

**Test evidence** (focused probes; full gates owned by verification):
- `cd web && npx vitest run src/__tests__/Inbox.test.js src/__tests__/Briefing.test.js
  src/__tests__/SpecDashboard.test.js src/__tests__/DetailPanel.test.js` — 141 passed, 0 failed.
  Covers: accordion behavior, all 10 priority types' action buttons incl. the PascalCase
  wire-format matrix, tenant-scope workspace badges, P2 inline diff before approve, P3
  override/close-MR API calls, P5 revert arbitration, P7 pause paths, briefing sections,
  entity-ref detail-panel navigation, time-range selector, spec table columns/sorting/filters,
  repo-scope progressbar aria attributes, spec detail-panel tab set.
- `cargo test -p gyre-server --offline --lib api::specs_assist::tests::save_spec` — 8 passed,
  0 failed, including the new `save_spec_fans_out_approval_notification_to_eligible_members`
  (Admin+Developer get exactly one notification with the structured body, Viewer gets none,
  system fallback unused when members exist) and the strengthened
  `save_spec_creates_mr_for_existing_repo` (body fields + fallback recipient).
- `cargo check -p gyre-server --offline` — clean. `cd web && npx vite build` — clean (dist
  rebuild not committed, per task-210 convention 08361ee0).

Sandbox restriction: TCP listener probe unsupported (errno 95), so no local server/browser
end-to-end run in this sandbox; component tests + production build stand in. Probe commands,
exit codes, and the pre-existing task-210 attribution failure on origin/main are recorded in
`/tmp/stage/review-evidence/task-175-probes.md`.
