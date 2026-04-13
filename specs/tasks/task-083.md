---
title: "HSI Nav Item Scope Content Matrix"
spec_ref: "human-system-interface.md §1.5"
depends_on:
  - task-082
progress: not-started
coverage_sections:
  - "human-system-interface.md §1.5 What Each Nav Item Shows at Each Scope"
commits: []
---

## Spec Excerpt

| Nav Item | Tenant Scope | Workspace Scope | Repo Scope |
|---|---|---|---|
| **Inbox** | Action queue across all workspaces | Action queue for this workspace | Action queue for this repo (filtered by `repo_id`) |
| **Briefing** | Narrative across all workspaces (client-side aggregation) | Narrative for this workspace | Narrative for this repo |
| **Explorer** | Workspace cards with summary stats (card grid, not graph canvas) | Realized architecture (C4 progressive drill-down) | Repo-level architecture detail |
| **Specs** | Spec registry across all workspaces | Specs across repos in workspace | Specs in this repo + implementation progress |
| **Meta-specs** | Persona/principle/standard catalog | Persona editor, preview loop, reconciliation progress | (redirects to workspace scope) |
| **Admin** | Users, compute, tenant budget, audit, workspace creation | Workspace settings, budget, trust level, teams, Policies, Repos | Repo settings, gates, policies, danger zone |

Key notes:
- Explorer at tenant scope is a **card grid** (workspace cards), not a graph canvas
- Explorer at repo scope has two tabs: **Architecture** (default — C4 graph) and **Code** (branches, commits, MRs, merge queue)
- Meta-specs at repo scope redirects to workspace scope
- Inbox repo-scope filters by `repo_id` on notifications; workspace-scoped notifications with `repo_id: NULL` only visible at workspace scope

## Implementation Plan

1. **Create scope-aware content routing** for each nav item:
   - Each nav item component receives the current scope (tenant/workspace/repo) and renders accordingly
   - The scope is derived from the URL route and breadcrumb state

2. **Inbox adaptation:**
   - Tenant scope: `GET /api/v1/users/me/notifications` (no workspace filter)
   - Workspace scope: `GET /api/v1/users/me/notifications?workspace_id=<id>`
   - Repo scope: query notifications with repo-scoped filter

3. **Briefing adaptation:**
   - Tenant scope: client-side aggregation calling `GET /api/v1/workspaces/:id/briefing` per workspace
   - Workspace scope: `GET /api/v1/workspaces/:id/briefing`
   - Repo scope: `GET /api/v1/workspaces/:id/briefing?repo_id=<id>`

4. **Explorer adaptation:**
   - Tenant scope: workspace card grid (fetch from `GET /api/v1/workspaces` + budget stats)
   - Workspace scope: C4 graph (existing ExplorerCanvas)
   - Repo scope: repo-level graph with Architecture/Code tabs

5. **Specs adaptation:**
   - Tenant scope: spec registry across workspaces
   - Workspace scope: specs filtered by workspace repos
   - Repo scope: specs for specific repo + progress

6. **Meta-specs adaptation:**
   - Tenant scope: catalog view
   - Workspace scope: persona editor, preview loop
   - Repo scope: redirect to workspace scope

7. **Admin adaptation:**
   - Tenant scope: users, compute, budget, audit, + New Workspace
   - Workspace scope: settings, budget, trust level, teams, Policies, Repos
   - Repo scope: repo settings, gates, policies, danger zone

## Acceptance Criteria

- [ ] Each nav item renders different content based on scope (tenant/workspace/repo)
- [ ] Explorer at tenant scope shows workspace card grid (not graph canvas)
- [ ] Explorer at repo scope shows Architecture and Code tabs
- [ ] Meta-specs at repo scope redirects to workspace scope
- [ ] Inbox at repo scope filters notifications by `repo_id`
- [ ] Briefing at tenant scope aggregates per-workspace briefings client-side
- [ ] Admin at tenant scope includes workspace creation button
- [ ] Admin at workspace scope includes Policies tab and Repos tab
- [ ] Content transitions are smooth (no full page reload on scope change)
- [ ] `npm test` passes in `web/`

## Agent Instructions

Read `specs/system/human-system-interface.md` §1.5 (What Each Nav Item Shows at Each Scope) for the full 6×3 matrix. This task depends on task-082 (stable sidebar). Check the existing App.svelte routing to understand how scope is currently determined. The scope indicator (breadcrumb) in the topbar determines the current scope. Key design rule: sidebar items never change — only the content area adapts. For "contextual drill-downs" (Task Board, Agent List, MR Detail, etc.), these are accessed by clicking entity references, not sidebar items. The Code tab at repo scope is part of the Explorer, not a separate nav item.
