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

The vision (§1-§5) defines what humans do: decide what to build, set direction, maintain understanding, handle exceptions, discover and encode. The navigation should map to these activities, not to database tables.

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
- **Decisions badge**: In repo mode, shows count for THIS repo only. Clicking opens the Decisions tab.

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
- Items include both actionable decisions (gate failures, spec approvals) and informational alerts (trust suggestions, spec assertions). All use the HSI §8 priority system. The name "Decisions" emphasizes that this surface is for human judgment, even when some items are informational — the human decides whether to act on them or dismiss them.
- When empty: shows "No decisions needed — system is running autonomously." This is the ideal state.

**Specs** (cross-repo spec overview):
- Shows all specs across all repos in the workspace, with repo attribution on each row.
- Columns: Repo, Path, Status (draft/pending/approved/implemented), Progress, Last activity.
- Click a spec → enters the repo that owns it (repo mode, Specs tab, detail panel open for that spec).
- Filter by status, repo, owner (`?owner=me` toggle).
- This is the workspace-level spec oversight surface — answers "what's the status of all my directives across repos?"
- Data source: `GET /api/v1/specs?workspace_id=:id` (existing, returns specs across repos in workspace).

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

**Agent Rules** (workspace-level meta-specs):
- Summary of active meta-specs (personas, principles, standards, process norms) at the workspace level.
- Shows required meta-specs prominently (these apply to every agent in every repo).
- "Manage rules" opens the meta-spec management surface (§4).
- Data source: `GET /api/v1/meta-specs?scope=Workspace&scope_id=:id` (per `agent-runtime.md` §2).

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

### Tab: Specs (default, landing tab)

The primary tab. Shows the spec registry for this repo with implementation progress.

**Content:**
- Spec list with columns: Path, Status (draft/pending/approved/implemented), Progress (0/5 tasks), Last activity
- Click a spec → detail panel slides in from right (HSI existing detail panel pattern) showing:
  - Content (spec text, editable with LLM assist)
  - Progress (tasks, agents, MRs linked to this spec)
  - Meta-spec bindings (which meta-specs are bound, pinned versions) — the binding editor shows available meta-specs from the workspace registry inline (no need to navigate to the Agent Rules page). Required meta-specs are shown as locked. The user selects optional meta-specs and pins versions directly in the spec detail panel.
  - Links (cross-workspace spec links)
  - History (approval ledger, version history)
  - Ask Why (interrogation agent for the implementing agent)
- `+ New Spec` button (opens spec editor with LLM assist)
- Filter: `?owner=me` toggle, status filter, search

**This tab closes the spec-to-execution gap.** You see a spec AND its implementation progress in one place. No switching to Explorer to find what code was produced.

### Tab: Architecture

The system explorer for this repo. Shows the realized architecture (knowledge graph).

**Content:**
- Graph canvas (C4 progressive drill-down per `system-explorer.md`)
- Three sub-tabs in the control bar: **Graph** (default), **Timeline**, and **Briefing**
- Control bar: Lens selector (Structural/Evaluative/Observable), view selector, search (`/`), Ask input
- Ghost overlays for structural prediction (HSI §3, Phase 1)
- Flow view available via view selector (when trace data exists)
- **Briefing sub-tab**: Full repo-scoped narrative view (not a collapsed panel — a full content area) with time range selector and "Ask a question" Q&A capability. Same structure as workspace home briefing but scoped to this repo via `?repo_id=` parameter. This gives the briefing proper space for the narrative + Q&A chat, rather than cramming it above the graph.

**Agent discovery:** The Architecture tab is the primary surface for finding agents. Active agent count is shown per graph node (repo boundary view shows agent badges on nodes). Clicking an agent badge opens the agent detail panel with Pause/Stop/Message controls (HSI §4). The workspace orchestrator is also reachable from the workspace home's Repos section (clicking the agent count on a repo row opens the agent list for that repo in a modal). This ensures agents are always discoverable without a dedicated Agent tab — agents are visible in the context of the architecture they're modifying.

**This tab is "understand what the system IS."** The graph, the timeline, the flow visualization, the LLM-generated views — all here.

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
│  │ Agent Rules              │  │ ┌─ Impact ──────────┐ │   │
│  │  ○ test-coverage  v1  │  │ │ 4 specs affected   │ │   │
│  │                        │  │ │ 2 repos impacted   │ │   │
│  │ [+ New Meta-Spec]      │  │ └────────────────────┘ │   │
│  │                        │  │                        │   │
│  │                        │  │ [Preview] [Approve]    │   │
│  └────────────────────────┘  └────────────────────────┘   │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

**Left panel — Registry:**
- List of all meta-specs in this workspace + ALL tenant meta-specs (required ones shown as locked, optional ones available for workspace-level binding or spec-level binding)
- Grouped by kind (Persona, Principle, Standard, Process)
- Shows name, version, approval status indicator
- "+ New Meta-Spec" button
- Click to select → loads in editor

**Right panel — Editor:**
- Prompt text editor (the meta-spec content)
- Approval status with Approve/Reject buttons (human-only)
- Required toggle (workspace admin only, per `agent-runtime.md` §2)
- Version history (expandable, shows all previous versions with diff)
- Impact panel (blast radius — which repos and specs are affected)
- Preview button → launches preview loop (meta-spec-reconciliation preview)
- Inline LLM chat for editing assistance

**This is the "power tool" for encoding judgment.** Impact is always visible, not hidden behind a tab. The preview loop is one click away. The approval workflow is inline.

---

## 5. Navigation Flows

### First Visit

1. User authenticates
2. Lands on workspace selector (if multiple workspaces) or workspace home (if one workspace)
3. Workspace home shows repos, decisions, briefing
4. User clicks a repo → enters repo mode
5. Subsequent visits restore last workspace (from `localStorage`) and land on workspace home. Last repo is pre-selected in the dropdown but not auto-entered — the user sees the workspace overview first.

### Daily Flow

1. Open app → **always workspace home first** (even if last repo is stored). This ensures workspace-scoped decisions (with `repo_id: NULL`) are never missed. The last-used repo is remembered and pre-selected in the repo dropdown, one click away. If there are zero unresolved decisions, the user can click their repo immediately.
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
| `Esc` | Close detail panel / back to workspace home | Always |
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
/                                          → workspace selector (or redirect to default)
/workspaces/:slug                          → workspace home
/workspaces/:slug/settings                 → workspace settings
/workspaces/:slug/agent-rules                → meta-spec management
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

**URL convention:** Workspace identified by slug, repo identified by name (unique within workspace per `platform-model.md` §1). Repos are nested under a `/r/` segment to avoid collision with reserved workspace paths (`settings`, `standards`, `decisions`). This produces readable URLs: `/workspaces/payments/r/payment-api/specs`.

**Reserved workspace paths:** `settings`, `agent-rules`, `decisions`, `r`. These are workspace-level pages or URL segments and cannot be used as repo names. The server rejects repo creation with these names (400 error).

**Profile URL:** `/profile` is preserved outside the `/workspaces/...` hierarchy (it is user-scoped, not workspace-scoped).

**Migration from old URLs:** The server should support legacy URL redirects. Old HSI-style URLs (`/repos/:uuid/explorer`, `/workspaces/:uuid/inbox`) are redirected (301) to the new structure by looking up the workspace slug and repo name from the UUID. The `/profile` URL is preserved as-is (outside the `/workspaces/...` hierarchy). This ensures existing bookmarks and CI integrations continue to work.

---

## 8. Responsive Design

### Desktop (≥1024px)
Full layout as described above. Workspace home sections stack vertically. Repo mode tabs are horizontal.

### Tablet (768-1024px)
Same layout, detail panels become full-width overlays instead of side panels.

### Mobile (<768px)
- Top bar: hamburger icon opens a navigation drawer listing workspace home sections (Decisions, Repos, Briefing, Agent Rules) as links — not full content, just navigation to scroll anchors on the workspace home page
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
- **Briefing**: Cross-workspace narrative (client-side aggregation: calls briefing per workspace, merges sections). Each item shows source workspace.

This view answers: "What needs me across my entire organization?" It's the zoomed-out orientation page for users who manage multiple workspaces.

**Tenant administration** (user management, compute target management, tenant budget) is accessed via a gear icon on the cross-workspace view header. Only visible to tenant Admin role users. Tabs: Users, Compute Targets, Budget, Audit.

The workspace selector dropdown shows workspaces the user is a member of. Workspace creation is available in the dropdown as a "+ New Workspace" option (visible to users with tenant Admin role).

---

## Relationship to Existing Specs

**Supersedes:**
- `human-system-interface.md` §1 (Navigation Model) — the six-item sidebar, scope indicator, nav scope table, deep links, keyboard navigation
- `ui-layout.md` §1 (Application Shell) — sidebar dimensions, topbar layout, entrypoint flow

**Amends:**

| Spec | Amendment |
|---|---|
| `human-system-interface.md` §1 | Navigation model replaced by this spec. The nav scope table (what each nav item shows at each scope) is replaced by §2-§3 of this spec. Deep link URLs change to the new structure (§7). Keyboard shortcuts updated (§6). |
| `ui-layout.md` §1 | Application shell changes: no persistent sidebar, topbar layout updated. Content area layouts (§2-§4 of ui-layout) remain valid. Status bar unchanged. |
| `ui-layout.md` §2 | Full-Width layout used by workspace home. Split layout used by repo mode tabs + detail panel. Canvas+Controls used by Architecture tab. Editor Split used by meta-spec management. All layouts preserved; the views that USE them change. |
| `human-system-interface.md` §8 | Inbox becomes "Decisions" throughout. Priority types and notification system unchanged — only the UI surface name and location change. |
| `human-system-interface.md` §9 | Briefing becomes a section in workspace home (not a standalone nav item). Briefing detail, Q&A, and data sources unchanged. |
| `repo-lifecycle.md` §1 | Repo management moves from "Admin → Repos tab" to workspace home Repos section (create/import) and repo settings tab (configure/archive/delete). |

**Depends on:**
- `vision.md` — the five human activities that drive the navigation structure
- `human-system-interface.md` §2-§12 — feature definitions that this spec arranges into the new navigation
- `ui-layout.md` §2-§10 — layout patterns used by the new views
- `agent-runtime.md` §2 — meta-spec registry API used by the Agent Rules section
- `repo-lifecycle.md` — repo CRUD used by workspace home Repos section
