# Gyre Dashboard UI

The Svelte SPA at `GET /*` is served from `web/dist/`. Access at `http://localhost:3000`.

> **Nav rework (PRs #589-#599):** The UI was restructured from a sidebar-driven 6-view model to a **two-mode, no-sidebar** model — Workspace Home (dashboard) and Repo Mode (horizontal tabs). The old sidebar (`/inbox`, `/briefing`, `/explorer`, etc.) is replaced. See `specs/system/ui-navigation.md` for the full spec.

---

## Application Shell

### Top Bar (always visible)

```
[≡] Payments ▾          [⌘K Search]       [🔔 3] [avatar]
```

- **Workspace selector** (`Payments ▾`): Clicking the name navigates to Workspace Home; the arrow opens the workspace switcher dropdown.
- **Search** (`⌘K`): Global command palette — specs, types, agents, repos, concepts.
- **Decisions badge** (`🔔 3`): Count of unresolved items requiring human judgment. Clicking navigates to Workspace Home with the Decisions section expanded.
- **User avatar**: Profile (notification prefs, API tokens, judgment ledger) and sign-out.

**In Repo Mode**, the top bar adds context:

```
[←] Payments / payment-api      [⌘K Search]  [🔔 1] [avatar]
```

- **Back arrow**: Returns to Workspace Home.
- **Decisions badge**: Shows count for this repo only in repo mode.

### Status Bar (bottom, always visible)

WebSocket connection indicator, trust level, budget usage progress bar, presence avatars (workspace scope).

### Keyboard Shortcuts

`?` opens the keyboard shortcuts modal. Key bindings:
- `⌘K` — global search
- `g` then `h` — go to workspace home
- `g` then `r` — go to repos list
- `g` then `5` — open settings (in repo mode)
- `i` — focus decisions/inbox
- `b` — open briefing
- Arrow keys — navigate list items; `Enter` — open selected

---

## Mode 1: Workspace Home

The workspace home is a **dashboard** — the landing page after selecting a workspace. No sidebar. Sections are scrollable cards:

### Sections

**Decisions** (top — most important)
- Unresolved items requiring human judgment, ordered by priority (gate failures, spec approvals, agent clarifications, divergence alerts, budget warnings, etc.)
- Inline Approve/Reject/Retry buttons per item. "View all" link.
- Data: `GET /api/v1/users/me/notifications` filtered to workspace.
- Empty state: "No decisions needed — system is running autonomously."

**Repos**
- Lists all repos with: name, active spec count, active agent count, health status (● healthy, ⚠ gate failure, ○ idle).
- Click a repo to enter Repo Mode.
- "+ New Repo" and "Import" buttons.
- Data: `GET /api/v1/workspaces/:id/repos` (with health summary).

**Briefing**
- LLM-synthesized narrative of what happened since last visit.
- Time-range selector. "Ask a question" opens SSE Q&A chat.
- Data: `GET /api/v1/workspaces/:id/briefing`.

**Specs**
- Cross-repo spec overview: all specs in this workspace with repo attribution, status, and progress.
- Click a spec to enter its repo in Repo Mode with the spec detail panel open.
- Data: `GET /api/v1/specs?workspace_id=:id`.

**Agent Rules**
- Effective meta-spec cascade for this workspace — tenant (inherited, 🔒) + workspace rules.
- Stale-pin warning badge if any specs in any repo have outdated meta-spec pins.
- "Manage rules" opens meta-spec management.
- Data: `GET /api/v1/meta-specs?scope=Workspace&scope_id=:id` merged with tenant meta-specs.

**Architecture** (collapsible, collapsed by default)
- Workspace-scope graph: repos as nodes, cross-repo dependencies as edges.
- Expand via "Show architecture" to see the canvas.

### Workspace Settings

Gear icon (⚙) in the workspace header opens a full-page settings view with tabs: General, Trust & Policies, Teams, Budget, Compute, Audit.

---

## Mode 2: Repo Mode

Clicking a repo from Workspace Home enters Repo Mode. The top bar shows `[←] Workspace / Repo` with horizontal tabs below:

```
[ Specs ][ Architecture ][ Decisions ][ Code ][ ⚙ ]
```

### Specs Tab

- Spec list for this repo. Click to open a slide-in `DetailPanel` with:
  - **Info tab**: status, approval history, links to implementing tasks/agents, Approve/Revoke/Reject buttons.
  - **History tab**: ledger timeline.
  - **Links tab**: cross-workspace spec links.
  - **Architecture mini canvas** (`ArchPreviewCanvas.svelte`): live architecture preview directly in the spec detail panel — shows nodes linked to this spec, calls `GET /api/v1/repos/{id}/graph/predict` to project expected structural changes. Bidirectional: clicking a graph node navigates to that node in the Architecture tab.
- LLM-assisted inline editor with `POST /api/v1/specs/{path}/assist` SSE.
- **Editor Split** (`EditorSplit.svelte`): pop-out side-by-side view — spec editor on the left, Architecture canvas on the right. Opened via a split-view icon in the spec detail panel. Enables writing a spec while watching its predicted architectural impact.

### Architecture Tab

- `ExplorerCanvas.svelte` (d3-force + ELK layouts): SVG node graph of the repo's knowledge graph.
- 9 node type shapes with distinct colors (package/module/type/interface/function/endpoint/component/table/constant).
- **Edge type labels**: hover-to-show labels on edges (calls, contains, imports, implements, etc.).
- Click node to open `DetailPanel` with file path, spec linkage, visibility, doc comment, complexity, churn.
- **Bidirectional nav**: clicking a node with a `spec_path` shows a "Go to spec" button that opens the Specs tab with that spec's detail panel. Spec → Architecture nav is via the Architecture mini canvas in the spec detail.
- Controls: lens/view selector, LLM "Ask" SSE chat, filter panel (node type, complexity, churn), Code tab.
- `POST /api/v1/repos/{id}/graph/predict` — structural predictions surfaced in canvas and mini canvas.

### Decisions Tab

- Items requiring judgment for this repo specifically (gate failures, spec approvals, MR reviews).
- Inline action buttons. Subset of Workspace Home Decisions filtered to `repo_id`.

### Code Tab

- File browser / diff viewer for the repo. Syntax-highlighted diffs. Merge request list.

### Repo Settings (⚙ tab)

- Gate definitions, ABAC policy, quality gates, mirroring config, archive/delete.

---

## Key Components

| Component | Role |
|---|---|
| `WorkspaceHome.svelte` | Workspace home dashboard — 5 section cards |
| `RepoMode.svelte` | Repo mode shell — horizontal tab routing |
| `ExplorerCanvas.svelte` | Architecture graph (d3-force + ELK, 9 node types) |
| `ArchPreviewCanvas.svelte` | Architecture mini canvas in spec detail panel |
| `EditorSplit.svelte` | Side-by-side spec editor + architecture canvas |
| `DetailPanel.svelte` | Slide-in right panel for entity drill-in |
| `InlineChat.svelte` | SSE-driven LLM chat widget |
| `FlowCanvas.svelte` | Canvas 2D animated agent flow graph (workspace Explorer) |
| `PresenceAvatars.svelte` | Live presence avatars in status bar |
| `ScopeBreadcrumb.svelte` | Top bar workspace/repo context breadcrumb |
| `MoldableView.svelte` | View-mode switcher (graph / list / timeline / flow) |

---

## Design System

Red Hat brand CSS variables (`web/src/lib/design-system.css`) — dark theme with `gray-95` (#151515) background, `red-50` (#ee0000) primary. Component library: `Button`, `Badge`, `Card`, `Table`, `Input`, `Modal`, `Toast`, `Tabs`, `Skeleton`, `EmptyState`, `Breadcrumb`, `SearchBar`, `NodeBadge`, `StatusBadge`, `DiffSuggestion`.

---

## Auth Token UI

Auth status in topbar user avatar dropdown. Shows token kind, agent ID, task ID, scope, and expiry. Personal API tokens managed at `GET/POST /api/v1/users/me/tokens`.
