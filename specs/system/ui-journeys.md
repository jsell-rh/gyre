# UI Journeys & Navigation

> **Status: Draft.** This spec defines the user journeys, navigation model, and scoping hierarchy for Gyre's UI. It replaces the current entity-centric sidebar with a journey-oriented navigation scoped to tenant, workspace, and repo.

## Problem

The current UI has 21 sidebar items that mirror the server's API surface: Agents, Tasks, Projects, Repos, Merge Queue, MCP Catalog, Compose, Analytics, Costs, Audit, Spec Approvals, Specs, Admin, Settings, Workspaces, Personas, Budget, Dependencies, Spec Graph, Profile.

This is architecture-driven navigation. The user doesn't wake up thinking "I need to check the MCP catalog." They think:
- "What happened overnight?"
- "What needs my attention?"
- "How is this feature progressing?"
- "I want to change how agents handle error handling."

The navigation should serve these activities, not expose the implementation's entity model.

Additionally, the current UI has no clear scoping model. The workspace selector exists but most views ignore it. Tasks, agents, and MRs are all shown tenant-wide with no indication of which workspace they belong to.

## Design

### 1. Scoping Model

The UI operates at three scope levels. The scope is always visible and determines what data is shown.

```
Tenant (cross-workspace visibility)
  └── Workspace (governance boundary, daily work context)
        └── Repo (where specs and code live)
```

#### Scope Indicator

A persistent breadcrumb in the topbar shows the current scope:

```
┌────────────────────────────────────────────────────┐
│ Acme Corp  ▸  Payments  ▸  payment-api             │
│ [tenant]      [workspace]   [repo]                  │
└────────────────────────────────────────────────────┘
```

Each segment is clickable — click "Payments" to zoom out to workspace scope, click "Acme Corp" to zoom out to tenant scope. The breadcrumb is always present on every view.

#### What's Available at Each Scope

| Scope | Views | Why |
|---|---|---|
| **Tenant** | Inbox, Briefing, Admin | Cross-workspace visibility: you need to see everything that needs attention and everything that happened, across all workspaces |
| **Workspace** | System Explorer, Meta-specs, Dependencies, Budget | Most daily work: you're managing a group of related repos that form a system |
| **Repo** | Specs (write/approve/track), Code (branches/commits/MRs), Activity | Specs are repo-bound. Implementation happens here. |

#### Scope Transitions

- **Tenant → Workspace**: click a workspace in the selector, or click a workspace reference anywhere in the UI
- **Workspace → Repo**: click a repo node in the System Explorer, or select from the repo list
- **Any → Tenant**: click the tenant segment in the breadcrumb
- **Deep links**: `/workspaces/{id}` lands at workspace scope, `/repos/{id}` lands at repo scope

When scope narrows, the sidebar updates to show views available at that scope. When scope widens, detail views collapse and summary views appear.

### 2. Navigation Structure

Five primary nav items, contextual to scope:

#### Tenant Scope

```
Sidebar:
  Inbox           ← action queue (approvals, failures, alerts)
  Briefing        ← "what happened" narrative
  Workspaces      ← workspace list / switcher
  Admin           ← users, compute, tenant budget, audit
```

#### Workspace Scope

```
Sidebar:
  Explorer        ← System Explorer (realized architecture)
  Meta-specs      ← persona editor, principles, preview loop
  Dependencies    ← cross-repo dependency graph
  Budget          ← workspace budget and usage

  Repos:          ← expandable list
    payment-api
    ledger-service
    billing-gateway
```

#### Repo Scope

```
Sidebar:
  Specs           ← write, approve, track implementation
  Code            ← branches, commits, MRs, merge queue
  Activity        ← agent work, gate results, recent changes
```

#### Where Did the Old Views Go?

| Old View | New Location |
|---|---|
| Dashboard | Replaced by **Inbox** (tenant) + **Explorer** (workspace) |
| Agents | Drill-down from Explorer or Activity — not a standalone view |
| Tasks | Visible within **Specs** (implementation progress per spec) |
| Projects | Replaced by **Workspaces** |
| Merge Queue | Visible within **Code** (repo scope) and **Inbox** (failures) |
| MCP Catalog | Moved to **Admin** (developer reference) |
| Compose | Moved to **Admin** (operational tool) |
| Analytics | Collapsed into **Briefing** (tenant) and **Budget** (workspace) |
| Costs | Collapsed into **Budget** (workspace) |
| Audit | Moved to **Admin** |
| Spec Approvals | Collapsed into **Inbox** (pending approvals) and **Specs** (approval status) |
| Spec Dashboard | Replaced by **Specs** (repo scope) + spec nodes in **Explorer** |
| Settings | Moved to **Admin** |
| Personas | Replaced by **Meta-specs** (workspace scope) |
| Spec Graph | Visible within **Explorer** as a concept view |
| Dependencies | Preserved at workspace scope |
| Budget | Preserved at workspace scope |
| Profile | User menu dropdown (top-right) |
| Activity Feed | Replaced by **Briefing** (narrative) and **Activity** (repo scope) |

Entity views (agent detail, task detail, MR detail) still exist as **contextual drill-downs** — click an agent name anywhere → slide-in panel with agent info. But they're not primary navigation.

### 3. Journey 1: "What Needs Me?" (Inbox)

**Scope:** Tenant (cross-workspace)
**When:** Every time the human opens Gyre

The Inbox is an **action queue** — items that require human attention, ordered by priority. When the queue is empty, the system is healthy and the human has nothing to do. That's the goal state.

#### Action Types (Priority Order)

| Action Type | Source | Required Human Action |
|---|---|---|
| **Spec pending approval** | Spec registry | Approve or reject (inline) |
| **Gate failure** | Merge queue gate results | Review diff + gate output, decide: retry / override / close |
| **Meta-spec drift alert** | Reconciliation controller | Review reconciliation results or adjust meta-spec |
| **Budget warning** | Budget enforcement | Increase limit or reprioritize work |
| **Reconciliation complete** | Reconciliation controller | Review summary: which specs changed, which were no-op |
| **Suggested spec link** | Knowledge graph (low-confidence) | Confirm or dismiss suggested spec ↔ code linkage |
| **Spec assertion failure** | Knowledge graph + spec assertions | Structural invariant violated — fix code or update spec |

#### Inline Actions

Most actions can be completed **without leaving the Inbox**:
- Spec approval: expand → read spec content → [Approve] / [Reject] / [Open in Explorer]
- Gate failure: expand → see MR diff + gate output → [Retry] / [Override] / [Close MR]
- Budget warning: expand → see usage chart → [Increase Limit] / [Pause Low-Priority Work]

Only complex actions (deep investigation, spec editing) navigate away — to the Explorer or Specs view.

#### Inbox Badge

The Inbox nav item shows a count badge of pending actions. When the user has handled everything, the badge disappears. Notifications also push to the user's configured channel (email, Slack webhook — future).

### 4. Journey 2: "What Happened?" (Briefing)

**Scope:** Tenant (cross-workspace), filterable by workspace
**When:** Morning check-in, after being away

The Briefing is a **narrative summary** of what the system accomplished since the human's last visit. Not an event log — a story told at the spec level.

#### Briefing Structure

```
Since your last visit (14 hours ago):

COMPLETED
  ✓ Payment retry logic (spec: payment-retry.md)
    3 MRs merged across payment-api. All gates passed.
    Agent: worker-12, persona: backend-dev v4.

  ✓ Reconciliation: persona v3 → v4 in payments workspace
    8 specs evaluated. 2 needed code changes (merged). 6 already compliant.

IN PROGRESS
  ◐ Auth refactor (spec: identity-security.md)
    3 of 5 sub-specs complete. 2 agents active.
    ETA: ~4 hours based on current velocity.

  ◐ Search improvements (spec: search.md)
    Agent blocked: dependency on vector-index crate not yet available.
    → Action: approve dependency addition? [Approve] [Reject]

EXCEPTIONS
  ✗ Gate failure: billing-service MR #47
    cargo test failed (3 tests). Agent retried once, still failing.
    → Action: [View Diff] [View Test Output] [Override] [Close MR]

METRICS
  12 MRs merged | 47 agent runs | $23.40 compute cost
  Budget: 67% of daily limit consumed
```

#### Data Sources

The briefing is generated from:

| Data Source | What It Provides |
|---|---|
| Spec registry + task rollup | "Spec X implemented" (all tasks done, all MRs merged) |
| Architectural deltas (knowledge graph) | Structural changes: new types, modified interfaces |
| Gate results | Failures and retries |
| Reconciliation records | Meta-spec change propagation status |
| Budget usage | Cost and consumption vs limits |
| Agent activity | Run counts, active agents, blocked agents |

The briefing can be **template-generated** (structured queries, deterministic) or **LLM-synthesized** (richer narrative, grounded in the same data). Both modes are available.

#### Time Scoping

- Default: "since your last visit" (tracked via `lastSeenAt` on user profile)
- Selectable: last 24h, last 7d, custom range
- Filterable: by workspace, by spec, by agent

### 5. Journey 3: "Build Something New" (Spec Authoring)

**Scope:** Repo
**When:** Human wants a new feature or change

1. Navigate to workspace → select repo → **Specs** view
2. Click [New Spec] → inline markdown editor opens
3. Write the spec
4. The explorer panel (if visible) shows ghost overlay: predicted structural impact of the new spec
5. Click [Approve] → spec enters approval flow
6. On approval, the system auto-creates tasks
7. The spec view shows implementation progress:

```
specs/system/vector-search.md
Status: Approved ✓  |  Implementation: 2/5 tasks complete

Tasks:
  ✓ TASK-201: Add VectorIndex type to gyre-domain
  ✓ TASK-202: Implement VectorPort trait
  ◐ TASK-203: Add vector search endpoint (agent active)
  ○ TASK-204: Update SearchBar component
  ○ TASK-205: Integration tests

MRs:
  ✓ MR #51: VectorIndex + VectorPort (merged)
  ◐ MR #52: Search endpoint (in review, gates running)
```

The spec is the tracking unit. Tasks and MRs are subordinate — visible here, but the human tracks at the spec level.

### 6. Journey 4: "Something Broke" (Exception Handling)

**Scope:** Starts at Tenant (Inbox), drills to Repo
**When:** Gate failure, spec assertion violation, budget alert

1. Gate failure appears in Inbox
2. Click → expands inline showing:
   - MR diff (side-by-side)
   - Gate output (test failures, lint errors)
   - Spec reference (what the MR was implementing)
   - Agent info (who produced this, which persona)
3. Human decides:
   - **Retry**: re-run gates (transient failure?)
   - **Override**: merge despite failure (human judgment)
   - **Request changes**: creates a task for the agent to fix the issue, with the gate output as context
   - **Close MR**: reject the approach entirely
4. If deeper investigation needed: [Open in Explorer] navigates to the repo's realized architecture, highlighting the affected modules

### 7. Journey 5: "Change How We Build" (Meta-spec Iteration)

**Scope:** Workspace
**When:** Human wants to update a persona, principle, or standard

1. Navigate to workspace → **Meta-specs** view
2. See the bound meta-spec set: personas, principles, standards with version indicators
3. Click a persona → inline editor opens
4. Edit the persona text
5. Select target specs from repos in this workspace (right panel: browse by repo, pick specs you know)
6. Click [Preview] → agents spawn with the draft persona, implement the selected specs on throwaway branches
7. The diff appears in the bottom panel: existing code vs. preview output
8. Adjust the persona, preview again
9. When satisfied, [Publish] → approval flow → reconciliation begins
10. Reconciliation progress visible in the workspace overview

This is the preview loop from meta-spec-reconciliation.md, surfaced as a first-class UI workflow.

### 8. Journey 6: "Set Things Up" (Admin)

**Scope:** Tenant
**When:** Infrequent — workspace creation, user management, compute targets

The Admin view is a tabbed panel (similar to today's admin panel):

| Tab | Contents |
|---|---|
| Users | User list, invite, role assignment, SCIM status |
| Workspaces | Create workspace, set tenant-level budget ceilings |
| Compute | Compute targets (local, Docker, SSH), tunnel management |
| Audit | Full audit log with filters (for compliance, not daily use) |
| SIEM | SIEM forwarding targets |
| Snapshots | Backup/restore |
| MCP Tools | Tool catalog (developer reference) |
| BCP | Recovery objectives, drill execution |

Admin is tucked away — not prominent in daily navigation. It's for operators, not users.

### 9. Mobile / Narrow Considerations

The Inbox and Briefing are the most important views for mobile/narrow viewports:
- **Inbox**: action cards, stacked vertically, swipeable (approve/dismiss)
- **Briefing**: narrative text, readable without canvas interaction
- **Explorer**: not optimized for mobile (canvas interaction requires pointer). Show a simplified list view of the knowledge graph at narrow widths.

### 10. Keyboard Navigation

| Shortcut | Action |
|---|---|
| `Cmd+K` | Global search (entities, specs, types, concepts) |
| `Cmd+I` | Jump to Inbox |
| `Cmd+E` | Jump to Explorer (workspace scope) |
| `Cmd+B` | Jump to Briefing |
| `Esc` | Close detail panel / go up one scope level |
| `/` | Focus search within current view |
| `?` | Show keyboard shortcut reference |

## Relationship to Existing Specs

**Supersedes:**
- All Dashboard UI descriptions in CLAUDE.md / AGENTS.md (sidebar groups, view descriptions, navigation model)
- `specs/system/activity-dashboard.md` — replaced by Briefing + Explorer
- M8 (Frontend Excellence) sidebar structure — replaced by scoped navigation
- M9 (Functional UI) CRUD modals — replaced by inline actions in Inbox and Explorer
- M28 (UI Cohesion) workspace selector — evolved into the scoping model with breadcrumb

**Depends on:**
- **realized-model.md** — Explorer and Briefing consume the knowledge graph
- **system-explorer.md** — Explorer is the primary workspace-scope view
- **meta-spec-reconciliation.md** — Meta-specs journey uses the preview loop
- **platform-model.md** — tenant/workspace/repo hierarchy defines scoping
- **spec-registry.md** — Specs journey uses the approval flow and ledger
- **user-management.md** — Inbox uses notification system; Briefing uses `lastSeenAt`

**Extends:**
- **platform-model.md** — the scoping model (tenant → workspace → repo) is now visually enforced in the UI, not just an API concept
- **spec-lifecycle.md** — spec implementation progress tracking is now a first-class UI element, not just auto-created tasks
