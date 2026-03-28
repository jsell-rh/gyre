# UI Navigation Model

> **Status: Draft.** This spec redefines Gyre's navigation structure. It **supersedes `human-system-interface.md` §1** (the "stable sidebar, adaptive content" model and the six-item sidebar) and **amends `ui-layout.md` §1** (the application shell). All other HSI sections (§2-§12) remain valid — this spec changes only how the user navigates between them.

## Why a New Navigation Model

The original HSI navigation model (six sidebar items: Inbox, Briefing, Explorer, Specs, Meta-specs, Admin) has a fundamental problem: **it groups by data type, not by user activity.**

The result:
- The same sidebar buttons mean different things at different scopes ("Admin" at workspace scope vs repo scope)
- Specs and Explorer are disconnected — you can't see a spec's implementation without switching views
- Meta-specs feels like "just another view" when it's actually the primary encoding mechanism
- Admin is a dumping ground for everything that doesn't fit elsewhere
- The human feels like a system operator, not a director of agents

The vision ("What Humans Do" §1-§5) defines what humans do: decide what to build, set direction, maintain understanding, handle exceptions, discover and encode. The navigation should map to these activities, not to database tables.

## Design Principles

1. **Repo is home.** Specs live in repos. Agents work in repos. Gates run on repo MRs. The human's daily context is a specific repo. Navigation should reflect this — like GitHub, the repo is the unit of work. Unlike GitHub, the work is directing agents via specs, not writing code.

2. **Two distinct modes.** Workspace mode (orientation: "what needs me across all repos?") and repo mode (focused work: "direct this repo's development"). These modes look and feel different because the activities are different.

3. **GitHub muscle memory.** Repo list → click repo → horizontal tabs. Developers already know this flow. Adapt the pattern; don't invent a new one.

4. **Decisions are always visible.** The notification bell in the top bar shows the count globally. Workspace home leads with decisions. Repo view has a Decisions tab. Human judgment is never more than one click away.

5. **No sidebar.** The workspace home is a dashboard. The repo view has horizontal tabs. There's no persistent sidebar that needs to morph between scopes — the two contexts are visually distinct pages.

---

## 1. Application Shell

### Top Bar (always visible, all modes)

```
┌──────────────────────────────────────────────────────────┐
│ [≡] Payments ▾          [⌘K Search]       [🔔 3] [avatar]│
└──────────────────────────────────────────────────────────┘
```

- **[≡] Hamburger** (mobile only): Toggles the workspace home sections on narrow viewports.
- **Workspace selector** (`Payments ▾`): Dropdown to switch workspaces. Shows workspace name. Clicking the name (not the arrow) navigates to the workspace home. Clicking the arrow opens the switcher dropdown.
- **Search** (`⌘K`): Global search — specs, types, concepts, agents, repos. Opens a command palette overlay. Same as HSI §1.
- **Decisions badge** (`🔔 3`): Count of unresolved items requiring human judgment across all repos in this workspace. Clicking navigates to the workspace home with the Decisions section expanded. Uses the notification system (not raw MR/spec counts).
- **User avatar**: Dropdown with Profile (HSI §12), Sign out.

**In repo mode**, the top bar adds a back arrow and repo context:

```
┌──────────────────────────────────────────────────────────┐
│ [←] Payments / payment-api      [⌘K Search]  [🔔 1] [av]│
└──────────────────────────────────────────────────────────┘
```

- **[←] Back arrow**: Returns to workspace home. Always visible in repo mode.
- **Workspace / Repo path**: Shows context. Clicking "Payments" returns to workspace home (same as back arrow). The repo name is not clickable (you're already there).
- **Decisions badge**: In repo mode, shows count for THIS repo only (excludes workspace-scoped notifications with `repo_id: NULL` per HSI §1). This is intentional — workspace-scoped items are visible on the workspace home, which the user sees first on every app open. Clicking opens the Decisions tab.

### Status Bar (bottom, always visible)

```
┌──────────────────────────────────────────────────────────┐
│ [WebSocket ●] [Trust: Guided] [Budget: 67%] [Presence]   │
└──────────────────────────────────────────────────────────┘
```

Same as `ui-layout.md` §1. Trust level, budget usage, WebSocket status, presence avatars for the current workspace.

---

## 2. Workspace Home

The workspace home is a **dashboard**, not a sidebar-driven view. It's the landing page after selecting a workspace. It answers: "What needs my attention, and where should I focus?"

```
┌──────────────────────────────────────────────────────────┐
│ [≡] Payments ▾          [⌘K Search]       [🔔 3] [avatar]│
├──────────────────────────────────────────────────────────┤
│                                                          │
│  ┌─ Decisions (3) ──────────────────────────────────┐   │
│  │ ⚠ Gate failure in payment-api (retry-logic)      │   │
│  │ ✋ Spec approval: user-service/auth-refactor.md   │   │
│  │ ? Agent clarification: billing-api                │   │
│  │                                          [View all]│   │
│  └──────────────────────────────────────────────────┘   │
│                                                          │
│  ┌─ Repos ──────────────────────────────────────────┐   │
│  │ payment-api    3 specs active  2 agents  ● healthy│   │
│  │ user-service   1 spec active   1 agent   ● healthy│   │
│  │ billing-api    blocked         0 agents  ⚠ gate   │   │
│  │                                                    │   │
│  │ [+ New Repo]  [Import]                             │   │
│  └──────────────────────────────────────────────────┘   │
│                                                          │
│  ┌─ Briefing ───────────────────────────────────────┐   │
│  │ Since yesterday: retry-logic spec implemented and │   │
│  │ merged. Auth refactor 60% complete, 3/5 tasks     │   │
│  │ done. 1 gate failure in payment-api needs you.    │   │
│  │                                    [Ask a question]│   │
│  └──────────────────────────────────────────────────┘   │
│                                                          │
│  ┌─ Specs ──────────────────────────────────────────┐   │
│  │ payment-api  retry-logic.md      ✅ Merged       │   │
│  │ user-service auth-refactor.md    🔄 60%          │   │
│  │ billing-api  error-handling.md   📝 Draft        │   │
│  └──────────────────────────────────────────────────┘   │
│                                                          │
│  ┌─ Agent Rules ──────────────────────────────────────┐   │
│  │ 6 meta-specs active (2 required)                  │   │
│  │ conventional-commits (principle, required)  ✓ v3  │   │
│  │ security (persona)                          ✓ v2  │   │
│  │                                     [Manage rules]│   │
│  └──────────────────────────────────────────────────┘   │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

### Sections

**Decisions** (top — most important):
- Shows unresolved items requiring human judgment, ordered by priority (HSI §8 priority table).
- Each item shows: type icon, description, which repo, inline action buttons (Approve/Reject/Retry).
- "View all" expands to a full-page decisions list with filtering by type, repo, priority.
- Data source: `GET /api/v1/users/me/notifications` (the notification system, not raw MR/spec queries).
- Surfaces ALL 10 notification types from HSI §8 (agent clarification, spec approval, gate failure, cross-workspace spec change, conflicting interpretations, meta-spec drift, budget warning, trust suggestion, spec assertion, suggested spec link). Items include both actionable decisions and informational alerts. The name "Decisions" emphasizes that this surface is for human judgment, even when some items are informational — the human decides whether to act on them or dismiss them.
- **Trust-level filtering:** The workspace's trust level controls which priority levels are shown, per HSI §2 "Inbox priorities shown" row. At Guided/Autonomous trust, priority-10 items (suggested links) are excluded. This is client-side filtering on the notification query (same as the old Inbox). Custom trust shows all priorities.
- When empty: shows "No decisions needed — system is running autonomously." This is the ideal state.

**Specs** (cross-repo spec overview):
- Shows all specs across all repos in the workspace, with repo attribution on each row.
- Columns: Repo, Path, Status (draft/pending/approved/implemented), Progress, Last activity.
- Click a spec → enters the repo that owns it (repo mode, Specs tab, detail panel open for that spec).
- Filter by status, repo, owner (`?owner=me` toggle).
- This is the workspace-level spec oversight surface — answers "what's the status of all my directives across repos?"
- Data source: `GET /api/v1/specs?workspace_id=:id` (existing, returns specs across repos in workspace).

**Architecture** (workspace-level graph — optional, expandable):
- A collapsible section showing the workspace-scope architectural graph: repos as nodes, cross-repo dependencies as edges (per HSI §3 / ui-layout §5 Boundary View).
- Collapsed by default — the workspace home is dashboard-first. Click "Show architecture" to expand.
- When expanded, shows the graph canvas with the same controls as the repo-mode Architecture tab but scoped to workspace (repos as top-level nodes).
- This preserves the cross-repo dependency visualization from the old Explorer at workspace scope.

**Repos** (pick where to focus):
- Lists all repos in the workspace with health indicators.
- Each row shows: repo name, active spec count, active agent count, health status (● healthy, ⚠ gate failure, ○ idle).
- Click a repo → enter repo mode (§3).
- "+ New Repo" and "Import" buttons for repo creation (per `repo-lifecycle.md` §2).
- Data source: `GET /api/v1/workspaces/:id/repos` (existing, extended with health summary).

**Briefing** (narrative context):
- LLM-synthesized narrative of what happened since the user's last visit.
- Time range selector: Since last visit (default), Last 24h, Last 7d, Custom.
- "Ask a question" opens the briefing Q&A chat (HSI §9).
- Data source: `GET /api/v1/workspaces/:id/briefing` (existing).

**Agent Rules** (meta-spec cascade summary):
- Shows the **full effective meta-spec set** for this workspace — not just workspace-level rules, but the complete cascade:
  - **Tenant (inherited)**: Required tenant meta-specs shown with a 🔒 lock icon and "Tenant" badge. These apply to every agent in every workspace. Cannot be edited here (managed by tenant admins via cross-workspace view §10).
  - **Workspace**: Workspace-level meta-specs (both required and optional). Required ones shown with 🔒. Optional ones available for spec-level binding.
  - **Effective set**: Combined view showing what agents in this workspace will actually receive. Required tenant + required workspace = the mandatory prompt set. Optional ones are available for spec authors to select.
- "Manage rules" opens the meta-spec management surface (§4) for workspace-level editing.
- "View tenant rules" link navigates to the cross-workspace Agent Rules section for browsing (and editing, if tenant admin).
- Data source: `GET /api/v1/meta-specs?scope=Workspace&scope_id=:id` merged with `GET /api/v1/meta-specs?scope=Global&required=true` for the inherited tenant rules.

### Workspace Settings

Accessed via a **gear icon** (⚙) in the workspace header, not a sidebar item:

```
Payments ▾  ⚙
```

Clicking ⚙ opens workspace settings as a full-page view with tabs:
- **General**: Workspace name, description, default compute target
- **Trust & Policies**: Trust level selector (HSI §2), ABAC policy editor (HSI §2a), policies ↔ trust cross-links
- **Teams**: Members, roles, invitations
- **Budget**: Workspace budget configuration, per-repo breakdown
- **Compute**: Compute target selection from tenant list
- **Audit**: Activity log for this workspace — spec approvals, gate overrides, trust level changes, agent activity, policy evaluations. Filterable by entity type, user, date range.

Back arrow returns to workspace home.

---

## 3. Repo Mode

Entering a repo (by clicking it in the workspace home, or via search, or via a decision item link) transitions to repo mode. The top bar shows the repo context and back arrow. Horizontal tabs appear below the top bar.

```
┌──────────────────────────────────────────────────────────┐
│ [←] Payments / payment-api      [⌘K Search]  [🔔 1] [av]│
├──────────────────────────────────────────────────────────┤
│ [ Specs ][ Architecture ][ Decisions ][ Code ][ ⚙ ]     │
├──────────────────────────────────────────────────────────┤
│                                                          │
│  Tab content here                                        │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

### Repo Header

Above the tabs, a compact repo header shows at-a-glance status:

```
payment-api                    2 agents active  Budget: 45%  [Clone URL 📋]
```

The agent count is **clickable** — opens an agent list panel (slide-in detail panel pattern) showing all agents in this repo with status, task, and Pause/Stop/Message controls (HSI §4). This is the primary agent discovery surface in repo mode — no need to navigate the graph to find agents.

### Tab: Specs (default, landing tab)

The primary tab. Shows the spec registry for this repo with implementation progress.

**Content:**
- Spec list with columns: Path, Status (draft/pending/approved/implemented), Progress (0/5 tasks), Last activity
- Click a spec → detail panel slides in from right (HSI existing detail panel pattern) showing:
  - Content (spec text, editable with LLM assist)
  - Progress (tasks, agents, MRs linked to this spec)
  - Meta-spec bindings — shows the **effective prompt set** for this spec's implementation:
    - **Required (locked)**: Tenant-required and workspace-required meta-specs, shown with 🔒 icon. Cannot be removed. These are what every agent implementing this spec will receive.
    - **Bound (author's selections)**: Optional meta-specs the spec author explicitly selected from the tenant/workspace registry, with pinned version numbers. The binding editor shows available meta-specs inline (no need to navigate to the Agent Rules page) grouped by kind (Persona, Principle, Standard, Process).
    - **Stale pins**: If a bound meta-spec has a newer approved version than the pinned one, a warning badge shows "v3 pinned, v5 available" with a one-click "Update pin" action.
    - The binding editor is the spec author's primary tool for directing HOW agents implement this spec (vision §2: "Set direction on how to build it").
  - Preview — inline preview controls for this spec:
    - "Predict" button → fast ghost overlay (2-5 seconds, structural prediction via `POST /repos/:id/graph/predict`)
    - "Preview" button → thorough preview (spawns agent on throwaway branch, shows real code diff + architecture delta). Uses the Editor Split layout (ui-layout.md §9).
    - Preview results shown inline in the detail panel — architecture diff and code diff tabs.
  - Links (cross-workspace spec links)
  - History (approval ledger, version history)
  - Ask Why (interrogation agent for the implementing agent)
- `+ New Spec` button (opens spec editor with LLM assist)
- Filter: `?owner=me` toggle, status filter, search

**This tab closes the spec-to-execution gap.** You see a spec AND its implementation progress in one place. No switching to Explorer to find what code was produced.

### Tab: Architecture (Moldable Development Surface)

The system explorer for this repo. This is where **moldable development** lives — the user shapes their view of the system to match how they think about it. The architecture is not a fixed diagram; it's a queryable, composable, LLM-augmented exploration surface.

**Canvas and Controls:**
- Graph canvas with pan, zoom, C4 progressive drill-down (per `system-explorer.md`)
- Control bar:
  - **Lens selector**: Structural (default) / Evaluative / Observable. Each lens overlays different data on the same graph. Structural shows boundaries and dependencies. Evaluative shows test results, gate outcomes, spec coverage, risk metrics. Observable shows production telemetry (future). Lenses compose — you can view the domain model (structural) with test coverage overlay (evaluative).
  - **View selector**: Boundary View (default), Spec Realization, Change View, saved views (user-curated), LLM-generated views (ephemeral). The view selector is a dropdown listing built-in views, then user's saved views, then a "Generate view..." option.
  - **Search** (`/`): Canvas-local search, highlights matching nodes.
  - **Ask input**: Natural language → `POST /workspaces/:workspace_id/explorer-views/generate`. LLM translates the question into a view spec (data query + layout + encoding) and renders it immediately. Generated views are ephemeral — the user can save explicitly via the saved views CRUD. Examples: "How does authentication work?", "Show me the payment retry flow", "What has the highest churn in the last 30 days?"
- **In-view filter panel** (200px, collapsible left panel): Category filters — Boundaries, Interfaces, Data, Specs. Toggle via filter icon in the control bar. Not part of the sidebar — inside the Architecture content area.

**View Spec Grammar (per `ui-layout.md` §4):**
Every view (built-in, saved, or generated) is a declarative JSON specification with four layers: Data (what to query), Layout (how to arrange — graph, hierarchical, layered, list, timeline, side-by-side, diff, flow), Encoding (how to visualize — color, size, border, opacity, labels), and Highlight (what to emphasize). The grammar is the shared language between the user, saved views, and the LLM. Users can inspect and edit the view spec directly if they want fine-grained control.

**Saved Views:**
- Users curate views by adjusting filters, layout, encoding, and save them for reuse.
- Stored per workspace, shareable with all workspace members.
- CRUD via `GET/POST/PUT/DELETE /workspaces/:workspace_id/explorer-views`.
- Built-in saved views shipped with every workspace: API Surface, Domain Model, Security Boundary, Test Coverage.
- The view selector dropdown shows: built-in views → user's saved views → "Generate view..." option.

**Ghost Overlays (Phase 1 — structural prediction):**
When editing a spec (from the Specs tab detail panel → Edit), the Architecture canvas shows **ghost nodes** — predicted structural changes rendered as dotted outlines with color-coded meaning (green = new, yellow = modified, red = removed). These appear within 2-5 seconds of editing via `POST /repos/:id/graph/predict`. Ghost overlays give fast, probabilistic feedback; the full preview loop (spawn agent on throwaway branch) gives certain feedback.

**Flow View:**
Available via the view selector when trace data exists. Animated particle visualization (Vizceral-inspired) showing how data flows through the system during test execution. Particles travel along edges between graph nodes, colored by test case. Playback controls: play/pause/step/speed/scrub/test-selector. Data from OTel traces captured by `TraceCapture` gate (per HSI §3a). When no trace data exists, the flow view shows an empty state explaining how to enable trace capture.

**Sub-tabs in the control bar:**
- **Graph** (default): The moldable graph canvas with all the above features.
- **Timeline**: Architectural history scrubber. Shows `ArchitecturalDelta` records on a horizontal timeline. Scrub to see how the architecture evolved. Click a delta marker to see commit SHA, agent, spec, and change summary.
- **Briefing**: Full repo-scoped narrative view with time range selector and "Ask a question" Q&A. Same structure as workspace home briefing but scoped to this repo via `?repo_id=` parameter (amends HSI §9: add optional `repo_id` field to `POST /workspaces/:id/briefing/ask` request body).

**Agent discovery:** Active agent count shown per graph node (boundary view shows agent badges). Clicking an agent badge opens the agent detail panel with Pause/Stop/Message controls (HSI §4). Also accessible from the repo header agent count (§3 Repo Header).

**This tab is "understand what the system IS and how it changes."** It is the primary surface for vision §3 (maintain understanding), §4 (structure is discovered), and §5 (feedback loop — the Observe/Understand steps).

### Tab: Decisions

Judgment items for this repo only.

**Content:**
- Same as the workspace-home Decisions section, but filtered to `repo_id = this repo`
- Shows: gate failures, spec approvals pending, agent clarification requests, divergence alerts
- Inline actions: Approve/Reject specs, Retry/Override gates, Respond to agents
- Each item expands to show full context (diff, gate output, agent uncertainty)

**This tab is "what needs my judgment in this repo?"**

### Tab: Code

Transparency and debugging view. Branches, commits, MRs, merge queue, clone URL.

**Content:**
- Sub-tabs: Branches, Merge Requests, Merge Queue
- Clone URL displayed prominently at the top (copyable)
- Commit log with agent attribution (which agent, which spec, which task)
- MR detail with diff, gate results, attestation bundle
- This is the `ExplorerCodeTab` component (already built, not wired)

**This tab is "verify what agents actually did."** Not the primary view — you go here when you need to inspect the implementation details or clone the repo locally.

### Tab: ⚙ (Settings)

Repo-level configuration. Not labeled "Admin" or "Settings" — just a gear icon.

**Content:**
- **General**: Repo name, description, default branch, max concurrent agents
- **Gates**: Gate chain configuration (test commands, lint, agent review, trace capture)
- **Policies**: Spec enforcement policies (require spec_ref, require approval, stale spec warning)
- **Budget**: Repo-level budget allocation (cannot exceed workspace)
- **Audit**: Repo-scoped activity log — agent activity, MR events, gate results, spec approvals for this repo. Filterable by event type and date range.
- **Danger Zone**: Archive repo, delete repo (per `repo-lifecycle.md` §4)

---

## 4. Meta-Spec Management

Accessed via "Manage rules" in the workspace home's Agent Rules section. Opens as a full-page view (like workspace settings) with back arrow to workspace home.

This is the **creative surface** for encoding organizational judgment (vision §2, §5). It is NOT a read-only catalog.

**Layout: Editor Split** (per `ui-layout.md` §4):

```
┌──────────────────────────────────────────────────────────┐
│ [←] Payments / Agent Rules        [⌘K Search]  [🔔] [av]  │
├──────────────────────────────────────────────────────────┤
│                                                          │
│  ┌─ Registry ────────────┐  ┌─ Editor ──────────────┐   │
│  │                        │  │                        │   │
│  │ Personas               │  │ security (persona)     │   │
│  │  ● security      v2   │  │ Status: Approved       │   │
│  │  ● default-worker v1  │  │                        │   │
│  │                        │  │ [prompt text area]     │   │
│  │ Principles             │  │                        │   │
│  │  ● conventional... v3  │  │ Required: [toggle]     │   │
│  │                        │  │                        │   │
│  │ Standards                │  │ ┌─ Impact ──────────┐ │   │
│  │  ○ test-coverage  v1  │  │ │ 4 specs affected   │ │   │
│  │                        │  │ │ 2 repos impacted   │ │   │
│  │ [+ New Meta-Spec]      │  │ └────────────────────┘ │   │
│  │                        │  │                        │   │
│  │                        │  │ [Preview] [Approve]    │   │
│  └────────────────────────┘  └────────────────────────┘   │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

**Left panel — Registry (cascade view):**
- Shows the **full meta-spec cascade** visible from this workspace:
  - **Tenant (inherited)**: Tenant-level meta-specs grouped by kind. Required ones shown with 🔒 — always injected, cannot be edited here. Optional tenant meta-specs shown with "Tenant" badge — available for workspace-level binding or spec-level binding. Editing tenant meta-specs requires navigating to the cross-workspace Agent Rules surface (link provided).
  - **Workspace**: Workspace-level meta-specs grouped by kind. Editable by workspace admins.
  - **Effective set summary**: At the top of the registry, a compact summary: "Agents in this workspace receive: 2 required tenant rules + 1 required workspace rule + spec-level selections."
- Grouped by kind within each scope level (Persona, Principle, Standard, Process)
- Shows name, version, approval status indicator (✓ approved, ⏳ pending, ✗ rejected)
- "+ New Meta-Spec" button (creates at workspace scope)
- Click to select → loads in editor

**Right panel — Editor:**
- Prompt text editor (the meta-spec content)
- **Scope and inheritance indicator**: Shows which scope this meta-spec belongs to (Tenant/Workspace) and whether it's required. For tenant meta-specs, the editor is read-only at workspace level (tenant admins edit via cross-workspace view).
- Approval status with Approve/Reject buttons (human-only, per `agent-runtime.md` §2 approval lifecycle — editing resets to Pending)
- Required toggle (workspace admin only for workspace-scoped; tenant admin only for tenant-scoped)
- Version history (expandable, shows all previous versions with inline diff between versions)
- **Impact panel (always visible, not behind a tab)**:
  - Blast radius: which repos and specs are affected by this meta-spec
  - Stale pins: which specs still pin an older version of this meta-spec
  - Drift status: which code was produced under an older version (from attestation `meta_specs_used` records)
- **Preview loop** (per `meta-spec-reconciliation.md` §5):
  1. Edit the prompt text in the editor
  2. Select 1-3 real specs from repos in the workspace (spec selector checklist)
  3. Click "Preview" → agents spawn with the draft meta-spec on throwaway branches
  4. Progress shown inline (per-spec: running/complete indicators)
  5. Results shown as: Architecture delta (structural impact) + Code diff (actual code changes)
  6. Iterate: edit again → preview again → repeat until satisfied (typically 3-8 iterations)
  7. Click "Publish" → commits the meta-spec change, triggers approval workflow
  8. Preview branches are ephemeral (auto-deleted after 24 hours)
- Inline LLM chat for editing assistance ("make this stricter about null handling", "add error retry guidance")
- **Reconciliation status**: After publishing and approving a required meta-spec change, shows reconciliation progress — which repos have been re-implemented, which are pending, which are in progress.

**This is the "power tool" for encoding judgment** (vision §2: "Set direction on how to build it" and §5: "Discover and encode"). Impact and drift are always visible. The preview loop is the primary interaction — edit, preview, iterate, publish. The approval workflow is inline. The reconciliation progress shows the cascade propagating through the system.

---

## 5. Navigation Flows

### First Visit

1. User authenticates
2. Lands on workspace selector (if multiple workspaces) or workspace home (if one workspace)
3. Workspace home shows repos, decisions, briefing
4. User clicks a repo → enters repo mode
5. Subsequent visits restore last workspace (from `localStorage`) and land on workspace home. Last repo is pre-selected in the dropdown but not auto-entered — the user sees the workspace overview first.

### Daily Flow

1. Open app → **always workspace home first** (even if last repo is stored). This ensures workspace-scoped decisions (with `repo_id: NULL`) are never missed. The last-used repo is remembered and highlighted in the workspace home Repos section, one click away. If there are zero unresolved decisions, the user can click their repo immediately.
2. Glance at Decisions count in top bar — any urgent items?
3. If decisions exist: handle them (approve specs, retry gates)
4. Click repo to focus → Specs tab shows implementation progress
5. Write or edit specs as needed
6. Check Architecture tab for structural understanding
7. Zoom out to workspace home to see cross-repo status

### Exception Flow

1. Decision badge shows `🔔 3`
2. Click badge → workspace home, Decisions section expanded
3. "Gate failure in payment-api" → click → enters repo mode, Decisions tab, gate failure detail
4. Review diff, gate output → Retry or Override
5. Back arrow → workspace home

### Meta-Spec Editing Flow

1. Workspace home → Agent Rules section → "Manage rules"
2. Select a meta-spec from registry
3. Edit prompt text
4. See impact panel update (blast radius)
5. Click Preview → agents run on throwaway branches
6. Review preview results
7. Click Approve → published, agents use new version going forward
8. Back arrow → workspace home

---

## 6. Keyboard Shortcuts

| Shortcut | Action | Context |
|---|---|---|
| `⌘K` | Global search (command palette) | Always |
| `Esc` | Close detail panel (if open). If no panel open in repo mode, navigate to workspace home. Priority: panel close first, then home navigation. | Always |
| `/` | Focus search within current view (suppressed during text input) | Always |
| `?` | Show keyboard shortcut reference (suppressed during text input) | Always |
| `g h` | Go to workspace home | Always (GitHub-style two-key sequence: press `g`, then within 500ms press the second key. Suppressed during text input.) |
| `g s` | Go to workspace settings | Always |
| `g a` | Go to Agent Rules management (`/workspaces/:slug/agent-rules`) | Always |
| `g 1` | Go to Specs tab in current repo | Repo mode only (no-op at workspace home) |
| `g 2` | Go to Architecture tab | Repo mode only |
| `g 3` | Go to Decisions tab | Repo mode only |
| `g 4` | Go to Code tab | Repo mode only |

**Design choice:** Tab shortcuts use the `g` prefix (not `⌘1-4`) to avoid context-dependent behavior. `⌘K` and `Esc` work everywhere. The `g` prefix sequences are suppressed when any text input is focused. The 500ms timeout ensures accidental `g` presses in normal typing don't trigger navigation.

---

## 7. URL Structure

Every state is URL-addressable for deep linking and sharing:

```
/                                          → workspace selector (or redirect to default workspace)
/all                                       → cross-workspace view (tenant scope)
/all/settings                              → tenant administration (Users, Compute, Budget, Audit)
/workspaces/:slug                          → workspace home
/workspaces/:slug/settings                 → workspace settings
/workspaces/:slug/agent-rules                → meta-spec management
/workspaces/:slug/specs                    → workspace-level cross-repo specs list
/workspaces/:slug/decisions                → full decisions list
/workspaces/:slug/r/:repo                    → repo mode, Specs tab (default)
/workspaces/:slug/r/:repo/specs              → repo Specs tab
/workspaces/:slug/r/:repo/architecture       → repo Architecture tab
/workspaces/:slug/r/:repo/decisions          → repo Decisions tab
/workspaces/:slug/r/:repo/code               → repo Code tab
/workspaces/:slug/r/:repo/settings           → repo Settings tab
/workspaces/:slug/r/:repo/specs?path=auth.md → specific spec detail
/workspaces/:slug/r/:repo/architecture?detail=node:uuid → specific node detail
/profile                                   → user profile (HSI §12, outside workspace hierarchy)
```

**URL convention:** Workspace identified by slug, repo identified by name (unique within workspace per `platform-model.md` §1). Repos are nested under a `/r/` segment to avoid collision with reserved workspace paths (`settings`, `agent-rules`, `decisions`). This produces readable URLs: `/workspaces/payments/r/payment-api/specs`.

**Reserved workspace paths:** `settings`, `agent-rules`, `decisions`, `specs`, `r`. These are workspace-level pages or URL segments and cannot be used as repo names. The server rejects repo creation with these names (400 error).

**Reserved workspace slugs:** `all`, `profile`. The slug `all` is the cross-workspace view. The slug `profile` would collide with `/profile`. The server rejects workspace creation with these slugs (400 error).

**Profile URL:** `/profile` is preserved outside the `/workspaces/...` hierarchy (it is user-scoped, not workspace-scoped).

**Migration from old URLs:** The server should support legacy URL redirects. Old HSI-style URLs (`/repos/:uuid/explorer`, `/workspaces/:uuid/inbox`) are redirected (301) to the new structure by looking up the workspace slug and repo name from the UUID. The `/profile` URL is preserved as-is (outside the `/workspaces/...` hierarchy). This ensures existing bookmarks and CI integrations continue to work.

---

## 8. Responsive Design

### Desktop (≥1024px)
Full layout as described above. Workspace home sections stack vertically. Repo mode tabs are horizontal.

### Tablet (768-1024px)
Same layout, detail panels become full-width overlays instead of side panels.

### Mobile (<768px)
- Top bar: hamburger icon opens a navigation drawer listing workspace home sections (Decisions, Specs, Repos, Briefing, Agent Rules) as links — not full content, just navigation to scroll anchors on the workspace home page
- Repo mode: tabs become a **scrollable horizontal strip** pinned below the top bar (not a bottom tab bar — bottom bars conflict with mobile browser chrome)
- Detail panels become full-screen modals
- Meta-spec editor: left/right panels stack vertically (registry above, editor below)
- Graph canvas: falls back to list view (already implemented in ExplorerCanvas)

---

## 9. What This Replaces

### From HSI §1 (Navigation Model)

| Old (HSI §1) | New (this spec) | Rationale |
|---|---|---|
| 6-item sidebar (Inbox, Briefing, Explorer, Specs, Meta-specs, Admin) | Workspace home dashboard + repo horizontal tabs | Sidebar grouped by data type; new model groups by user activity |
| "Stable sidebar, adaptive content" | Two distinct modes (workspace home, repo mode) | Same buttons with different content was confusing |
| Breadcrumb as primary scope control | Workspace dropdown + repo click + back arrow | Breadcrumb scope model was hidden; new model is explicit |
| Scope: tenant → workspace → repo (content adapts) | Workspace home (overview) → repo (focused tabs) | Three scope levels with adaptive content was unpredictable |
| Admin nav item (dumping ground) | Gear icons for workspace/repo settings | Settings live in context, not in a nav item |
| Meta-specs as sidebar nav item | "Agent Rules" section in workspace home + full management page | Meta-specs promoted from "just another view" to workspace-level governance |
| Inbox badge counts raw MRs + specs | Decisions badge counts notifications | One data source (notification system), not two |

### From ui-layout.md §1 (Application Shell)

| Old (ui-layout §1) | New (this spec) | Rationale |
|---|---|---|
| Sidebar (240px, collapsible to 48px) | No persistent sidebar | Two-mode design doesn't need a sidebar |
| Topbar with breadcrumb + user menu | Topbar with workspace selector + search + decisions + user | Breadcrumb replaced by simpler workspace/repo display |
| Content area adapts to nav + scope | Workspace home is a dashboard; repo mode uses tabs | Content is predictable per mode |

### Preserved (not changed by this spec)

- HSI §2 (Trust Gradient) — moves to workspace settings
- HSI §2a (Policies ↔ Trust integration) — moves to workspace settings
- HSI §3 (Explorer / Architecture) — becomes the Architecture tab
- HSI §4 (Agent Communication) — accessible from spec detail panel and Decisions tab
- HSI §5 (Conversation Provenance) — unchanged
- HSI §6 (Cross-Workspace Spec Links) — unchanged
- HSI §7 (Multi-Human Collaboration) — presence moves to status bar (unchanged)
- HSI §8 (Inbox Detail / Priority Types) — becomes the Decisions system
- HSI §9 (Briefing Detail) — becomes workspace home Briefing section
- HSI §10 (Observable Lens) — unchanged
- HSI §11 (CLI/MCP Parity) — unchanged
- HSI §12 (User Profile) — accessible from avatar dropdown

---

## 10. Cross-Workspace View

When the workspace selector shows "All Workspaces" (or when a user selects the tenant name), the workspace home transforms into a **cross-workspace dashboard**. This is the tenant-scope equivalent — it aggregates data across all workspaces the user has access to.

**Cross-workspace home sections:**
- **Decisions**: All unresolved items across all workspaces, with workspace attribution badges on each item
- **Workspaces**: List of workspaces with health, agent count, budget usage (replaces the Repos section). Click to enter a workspace. "+ New Workspace" button for tenant admins.
- **Specs**: All specs across all workspaces with workspace/repo attribution. Same columns as workspace-home Specs section. Answers "what's the status of all my directives across the entire org?"
- **Briefing**: Cross-workspace narrative (client-side aggregation: calls briefing per workspace, merges sections). Each item shows source workspace.
- **Agent Rules**: Tenant-level meta-spec catalog — browse all personas, principles, standards, process norms across the tenant. Shows which are marked required at tenant level. For tenant admins: "Manage tenant rules" link opens a tenant-level editing surface (same layout as workspace §4, but scoped to tenant — can create/edit tenant-level meta-specs and toggle `required` flag). Non-admin users see read-only view.

This view answers: "What needs me across my entire organization?" It's the zoomed-out orientation page for users who manage multiple workspaces.

**Tenant administration** is accessed via a gear icon on the cross-workspace view header. Only visible to tenant Admin role users. Tabs: Users, Compute Targets, Budget, Audit, Health, Jobs. Additional operational tabs from `platform-model.md` §9 (SIEM, Network, Snapshots, Retention, BCP) are deferred to future milestones and not included in this navigation model — amend `platform-model.md` §9 to mark them as deferred.

The workspace selector dropdown shows workspaces the user is a member of. Workspace creation is available in the dropdown as a "+ New Workspace" option (visible to users with tenant Admin role).

---

## Relationship to Existing Specs

**Supersedes:**
- `human-system-interface.md` §1 (Navigation Model) — the six-item sidebar, scope indicator, nav scope table, deep links, keyboard navigation
- `ui-layout.md` §1 (Application Shell) — sidebar dimensions, topbar layout, entrypoint flow
- `docs/ui.md` — navigation model, keyboard shortcuts, sidebar description (operational docs, must be rewritten to match this spec)

**Amends:**

| Spec | Amendment |
|---|---|
| `human-system-interface.md` §1 | Navigation model replaced by this spec. The nav scope table (what each nav item shows at each scope) is replaced by §2-§3 of this spec. Deep link URLs change to the new structure (§7). Keyboard shortcuts updated (§6). |
| `ui-layout.md` §1 | Application shell changes: no persistent sidebar, topbar layout updated. Content area layouts (§2-§4 of ui-layout) remain valid. Status bar unchanged. |
| `ui-layout.md` §2 | Full-Width layout used by workspace home and repo Decisions/Code tabs. Split layout used by repo Specs tab + detail panel. Canvas+Controls used by Architecture tab. Editor Split used by meta-spec management. All layouts preserved; the views that USE them change. |
| `ui-layout.md` §5 | The Explorer's Code sub-tab (Architecture/Code toggle in the control bar) is moved to a separate repo-mode tab. The Architecture tab no longer has a Code sub-tab — Code is a peer tab. |
| `human-system-interface.md` §8 | Inbox becomes "Decisions" throughout. Priority types and notification system unchanged — only the UI surface name and location change. |
| `human-system-interface.md` §9 | Briefing becomes a section in workspace home (not a standalone nav item) and a sub-tab in the Architecture tab at repo scope. Q&A endpoint amended to accept optional `repo_id` in request body. Briefing detail and data sources unchanged. |
| `repo-lifecycle.md` §1 | Repo management moves from "Admin → Repos tab" to workspace home Repos section (create/import) and repo settings tab (configure/archive/delete). |

**Depends on:**
- `vision.md` — the five human activities that drive the navigation structure
- `human-system-interface.md` §2-§12 — feature definitions that this spec arranges into the new navigation
- `ui-layout.md` §2-§10 — layout patterns used by the new views
- `agent-runtime.md` §2 — meta-spec registry API used by the Agent Rules section
- `repo-lifecycle.md` — repo CRUD used by workspace home Repos section
