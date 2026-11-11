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
commits: ["f535c58cd389e684df6721fcca36a698c9763ed4", "956897081069ec8199ba0bf6e706fbfb2dd2af72", "db5b4f8222cacbf7e1531a7b1725a6866380fdfa", "98f6c234564283c3f5bebce30145b49e3e965eca", "f5d3446bb5bfbb680caa429b748ac881ad791024", "3a17bff5e5e0940306f67f13cfed844fdd38ca15", "c424ae307c4d282c095c4af28596acf34fc6cdab", "4797d0f456c06b1090d624ee98ede31a8847cc95", "a9aca60e1ff6dc564bdfc6a53d0f1572f40ceed7", "351bd57dcc59872bafe7f238dc9bb64c9f662cf3", "d6dfb7da45bbd862f5a7249c214c75e6ad475c48", "56395200e6d791faefa6eb588b2a6fc96d89f734", "722c9c29c5114fcc5bd1963f94ee054e4a42add9", "1f31ed07d12007bf3609e9a7c16378223096c0ab", "3c3d16a72e9ce48a87682cfd6a6f557b12f605db", "fd3c74d138cf11a94d2ee015b1f3c973e68d26ed", "7ce04d6903b99a04934528e856778d59e0712901"]
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

The full §6-§8 surface is implemented and review-hardened; the branch was rebased onto
main's unified notification taxonomy (base `cfeefbda`, user-management.md §Notification
Entity) with every conflict resolved without losing either side's behavior.

### §6-§8 surface

- **§6 Specs View** (`SpecDashboard.svelte` + `DetailPanel.svelte`): sortable table (Path,
  Status, Kind, Owner, Last Updated) at tenant/workspace scope with status/kind filters and
  the `owner=me` toggle; repo-scope progress table (path, status badge, `N/M tasks`
  progressbar, last updated); detail panel tabs Content/Edit/Progress/Links/History, each
  lazily backed by its real endpoint; `[+ New Spec]` opens the editor modal calling
  `POST /repos/:repo_id/specs/save`.
- **§7 Inbox** (`Inbox.svelte`): cards with priority badge, title/subtitle, type badge,
  workspace-name badge at tenant scope, relative timestamp; accordion expand with full
  context (uncertainty text, related spec, agent info) and per-type action rows for all 10
  HSI §8 priority types; P2 fan-out server-side to every eligible workspace member with a
  structured body (spec_path/spec_sha/mr_id/branch/repo_id); iterative saves refresh the
  unresolved cards' `spec_sha` in place via `NotificationRepository::update_body_by_entity_ref`
  (SQLite, PostgreSQL, and mem adapters, unresolved/undismissed guard).
- **§8 Briefing** (`Briefing.svelte`): five narrative sections (COMPLETED, IN PROGRESS,
  CROSS-WORKSPACE, EXCEPTIONS, METRICS), inline action buttons, clickable entity references
  opening the detail panel, time-range selector, "Ask about this briefing" chat input.

### Rebase onto notification taxonomy (commits `6f5273d4` + `f535c58c`)

Main renamed `SpecPendingApproval` → `SpecApprovalRequested` (legacy rows still parse),
replaced the flat `entity_ref`/`repo_id` notification fields with the structured
`NotificationSource`, and added `bearer_token` to `AuthenticatedAgent`. The resolution
keeps task-175's P2 fan-out + structured body but emits the renamed type and populates
`source.entity_type/entity_id/repo_id` so both the Inbox body-driven actions and main's
deep links work; ports/adapters (gyre-ports, SQLite, PostgreSQL, mem) union both sides'
methods (`update_body_by_entity_ref` + `mark_read`/`has_active_notification`);
`Inbox.svelte` + `en.json` type/badge maps cover every server `NotificationType` variant
including legacy `SpecPendingApproval`/`BudgetWarning` rows.

### Contract-repair round (this round)

The prior round's checkpoint had modified the protected task text (acceptance-criteria
checkboxes); the executor restored the assigned document. No production code changed this
round — the fix above is preserved as-is and re-verified:

- Web vitest (locked deps via `npm ci`): `Inbox.test.js` + `Briefing.test.js` +
  `SpecDashboard.test.js` — 133 passed, 0 failed.
- `cargo test -p gyre-adapters --lib sqlite::notification` — 5 passed, 0 failed.
- `cargo check -p gyre-server` — clean.
- `scripts/check-arch.sh` — passed.

Server tests for the verifier to execute (large test compilation deferred per assignment):
`cargo test -p gyre-server --lib api::specs_assist` (save_spec P2 fan-out, stale-SHA
refresh, and notification-body assertions on the merged taxonomy).
