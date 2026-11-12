# Gyre Dashboard UI

The Svelte SPA at `GET /*` is served from `web/dist/`. Access at `http://localhost:3000`.

> **Nav rework (PRs #589-#599):** The UI was restructured from a sidebar-driven 6-view model to a **two-mode** model — Workspace Home (dashboard) and Repo Mode (horizontal tabs). See `specs/system/ui-navigation.md` for that spec. The permanent sidebar described by `human-system-interface.md` §1.3 is **not shipped** — the canonical navigation is `specs/system/ui-navigation.md` (Principle 5: no persistent sidebar). On narrow viewports a hamburger toggles a drawer that links to the workspace-home sections.

---

## Application Shell

### Shell Layout (no persistent sidebar)

There is **no persistent sidebar** (`specs/system/ui-navigation.md` Principle 5). The shell is: Top Bar + content area + Status Bar. Navigation is via the top bar (workspace selector, search, decisions badge), the workspace-home sections, and the repo-mode horizontal tabs. On narrow viewports, the hamburger (`[≡]`) opens a drawer linking to the workspace-home sections.

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
- `⌘K` (or `/`) — global search / command palette
- `Esc` — close detail panel, modal, or dropdown; in repo mode with nothing open, return to workspace home
- `g` then `h` — go to workspace home
- `g` then `s` — go to workspace settings
- `g` then `a` — go to Agent Rules management
- `g` then `1`–`8` — repo-mode tabs: `g 1` Specs, `g 2` Architecture, `g 3` Decisions, `g 4` Code, `g 5` Tasks, `g 6` MRs, `g 7` Agents, `g 8` Settings (repo mode only; no-op at workspace home)

All single-letter and `g`-sequence shortcuts are suppressed while typing in a text input. `g`-sequences expire 500ms after the `g` keypress.

---

## Mode 1: Workspace Home

The workspace home is a **dashboard** — the landing page after selecting a workspace. Sections are scrollable cards:

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
[ Specs ][ Tasks ][ MRs ][ Agents ][ Architecture ][ Dependencies ][ Decisions ][ Code ][ ⚙ ]
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
- **Sub-tabs: Graph | Briefing** — the Briefing sub-tab is the repo-scoped narrative (same shape as the workspace Briefing section, narrowed server-side via `?repo_id=` on `GET /api/v1/workspaces/:id/briefing` and `POST .../briefing/ask`); deep-linkable via `?subTab=briefing`.
- `POST /api/v1/repos/{id}/graph/predict` — structural predictions surfaced in canvas and mini canvas.
- **Default views** (HSI §3 / ui-navigation.md §3; the view selector is a **dropdown** in the Explorer control bar listing built-in views, then the workspace's saved views, then a "Generate view…" option that focuses the Ask/generate chat; Boundary is the default):
  - **Boundary** — C4 progressive drill-down: Level 1 = the workspace-scope realized-architecture graph (ui-layout.md §5: repos as nodes with type/endpoint count sub-labels from the workspace knowledge graph, cross-repo dependencies as edges, double-click drills to the repo's crate level); Levels 2–4 = the canvas drill-down (packages → modules → code items) with breadcrumb drill-up.
  - **Spec Realization** — two-panel view: spec list (status badge, linked-node count, task progress) ↔ graph nodes linked via GovernedBy edges / `spec_path`. "Read / Edit Spec" opens the inline spec editor; clicking a node navigates to it in Boundary.
  - **Change** — architectural deltas since your last visit (`GET /repos/{id}/graph/timeline?since=…`, `since` from the client-cached `X-Gyre-Last-Seen` response header; 24h fallback on first visit). Added (+) / Modified (~) / Removed (−) entries with type, containing module, spec ref, authoring agent; entries navigate to the node in Boundary.
  - **System Trace** — MR SDLC timeline from `GET /api/v1/merge-requests/{id}/timeline`: MR selector, typed event rows, expandable gate pass/fail, and View Conversation / View Diff / View Attestation inline sections.
- **Lens selector** (ui-navigation.md §3): a **checkbox group** in the canvas toolbar — Structural (checked + locked, always the base layer), Evaluative (additive overlay toggle: gate outcomes, spec-coverage badges, trace particles, and a **Risk** heat metric from `GET /repos/{id}/graph/risks`, churn × complexity), Observable (locked off until production telemetry integration; clicking surfaces the notice banner).
- **In-view filter panel** (ui-navigation.md §3): a collapsible 200px left panel inside the Architecture content area (toggled by the filter icon in the control bar, `NOT` the app sidebar). Category checkboxes — Boundaries, Interfaces, Data, Specs (with counts) — plus a **Concepts** section listing both mechanisms (system-explorer.md §4 "Data source distinction"): **Predefined** concepts from the repo's spec manifest (`GET /repos/{id}/graph/concepts`, projection-backed via `GET /repos/{id}/graph/concept/{name}`) and **Discovered** concepts (cross-cutting name tokens, substring matched). Selecting either fades the canvas to the concept's nodes (concept elements keep full opacity with a highlighted border; everything else fades to background — drawn, never removed), shown as a clearable control-bar chip; right-clicking a multi-selection offers "Create concept", saved as an explorer view. Nodes hidden by the category checkboxes and their edges are skipped by the canvas draw loop.
- **View spec editor** (ui-navigation.md §3): an "Edit view spec" button (code icon) in the control bar opens a collapsible JSON editor for the view spec grammar (data/layout/encoding/highlight). Apply translates the spec via `viewSpecToViewQuery` and re-renders the canvas live; invalid JSON shows an inline error.
- **Generated views** (HSI §3, task-089): the Explorer chat column has two modes — *Explorer* (the repo-scoped WS agent chat) and *Generated Views* (`GeneratedViewChat.svelte`). The Generated Views mode sends a natural-language question to `POST /api/v1/workspaces/{id}/explorer-views/generate`; the LLM (a read-only query translator) answers with a ViewSpec (graph query + layout + explanation). The view is applied to the ExplorerCanvas immediately, with the LLM's explanation rendered as the canvas query annotation (sidebar annotation). Layout follows question intent: data-movement questions get a `"flow"` layout that replays the `trace_source` MR's captured trace as animated particles on the evaluative lens (server-enforced: no trace data → `"graph"` fallback with an explanatory note); structural questions get `"graph"`/`"hierarchical"`. Each generated view has a **Save this view** button (`POST /api/v1/workspaces/{id}/explorer-views`, preserving query + layout + explanation), and follow-up questions refine the current view (the previous view definition is sent as refinement context).

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
| `App.svelte` | Application shell — top bar, two-mode routing, mobile drawer, keyboard shortcuts (no persistent sidebar) |
| `WorkspaceHome.svelte` | Workspace home dashboard — 5 section cards |
| `RepoMode.svelte` | Repo mode shell — horizontal tab routing |
| `ExplorerCanvas.svelte` | Unified architecture canvas — semantic zoom treemap, lens toggle (structural/evaluative/observable), filter presets, view query rendering, minimap (replaces the former Graph/Flow tabs) |
| `explorer/SpecRealizationView.svelte` | Explorer default view — spec ↔ realization two-panel (GovernedBy linkage) |
| `explorer/ChangeView.svelte` | Explorer default view — architectural deltas since last visit |
| `explorer/SystemTraceView.svelte` | Explorer default view — MR SDLC timeline with gate expansion and conversation/diff/attestation sections |
| `GeneratedViewChat.svelte` | Explorer chat column, Generated Views mode — natural-language → LLM-generated ViewSpec via the workspace generate endpoint; apply / save / refine generated views (HSI §3) |
| `ArchPreviewCanvas.svelte` | Architecture mini canvas in spec detail panel |
| `EditorSplit.svelte` | Side-by-side spec editor + architecture canvas |
| `DetailPanel.svelte` | Slide-in right panel for entity drill-in |
| `InlineChat.svelte` | SSE-driven LLM chat widget |
| `PresenceAvatars.svelte` | Live presence avatars in status bar |
| `ScopeBreadcrumb.svelte` | Top bar workspace/repo context breadcrumb |

---

## Design System

Red Hat brand CSS variables (`web/src/lib/design-system.css`) — dark theme with `gray-95` (#151515) background, `red-50` (#ee0000) primary. Component library: `Button`, `Badge`, `Card`, `Table`, `Input`, `Modal`, `Toast`, `Tabs`, `Skeleton`, `EmptyState`, `Breadcrumb`, `SearchBar`, `StatusBadge`, `DiffSuggestion`.

---

## Auth Token UI

Auth status in topbar user avatar dropdown. Shows token kind, agent ID, task ID, scope, and expiry. Personal API tokens managed at `GET/POST /api/v1/users/me/tokens`.
