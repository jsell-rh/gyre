# UI Layout & Interaction Patterns

> Companion to `human-system-interface.md`. This spec defines the spatial structure, interaction patterns, and view specification grammar for Gyre's UI. It assumes the interaction model, trust gradient, and data flows defined in that spec.

## 1. Application Shell

### Fixed Structure

The application shell has three permanent zones that never change:

```
┌─────────────────────────────────────────────────────────────┐
│ Topbar: [Breadcrumb: Tenant > Workspace > Repo]  [Cmd+K] [User] │
├────────┬────────────────────────────────────────────────────┤
│        │                                                    │
│ Side-  │              Content Area                          │
│ bar    │                                                    │
│        │                                                    │
│ Inbox  │   (adapts to nav selection + scope)                │
│ Brief  │                                                    │
│ Explor │                                                    │
│ Specs  │                                                    │
│ Meta   │                                                    │
│ Admin  │                                                    │
│        │                                                    │
├────────┴────────────────────────────────────────────────────┤
│ Status bar: [WebSocket ●] [Trust: Guided] [Budget: 67%]     │
└─────────────────────────────────────────────────────────────┘
```

**Topbar** (48px height):
- Left: scope breadcrumb (clickable segments). Clicking a segment zooms out to that scope.
- Center: `Cmd+K` search trigger (global search overlay).
- Right: user avatar dropdown (profile, settings, sign out), notification badge count.

**Sidebar** (240px width, collapsible to 48px icon-only via toggle):
- Six nav items, always in this order, always present. Active item highlighted.
- At the bottom: server status indicator (WebSocket connection, version).

Workspace switching is done exclusively via the breadcrumb (click workspace segment → dropdown of member workspaces). No duplicate switcher in the sidebar — one control, one location.

**Content Area** (remaining space):
- Adapts to the selected nav item and current scope.
- Can be split into main + detail panel (see §3).

**Status Bar** (24px height):
- Trust level indicator for the current workspace.
- Budget usage percentage.
- WebSocket connection status.
- Presence: avatars of other active users in this workspace (active = heartbeat within last 60 seconds; clients send `UserPresence` every 30 seconds; the server evicts entries older than 60 seconds from the presence map).

### Entrypoint Flow

**First visit (no stored state):**
1. User authenticates (Keycloak OIDC or dev token).
2. Landing page: Explorer at tenant scope — workspace cards. User picks a workspace.
3. After selection: redirect to Inbox at workspace scope. Breadcrumb updates. Workspace ID stored in `localStorage`.

**Subsequent visits:**
1. Restore last-used workspace from `localStorage`.
2. Land on **Inbox** at workspace scope. Inbox is the default active sidebar item.
3. If the stored workspace no longer exists or the user lost membership, fall back to workspace cards (Explorer at tenant scope).

**Switching workspace:**
- Click workspace name in the breadcrumb → dropdown of member workspaces.
- Or click tenant name in breadcrumb → Explorer at tenant scope (workspace cards).
- Workspace selection updates `localStorage` and re-renders all content.

**Multi-tenant (future):**
- Tenant selector appears as the first breadcrumb segment.
- Single-tenant deployments (the default) omit it.

---

## 2. Content Area Layouts

The content area uses one of four layout patterns depending on the view:

### Full-Width

Used by: Inbox, Briefing, Specs (list), Meta-specs (catalog), Admin tabs. Note: full-width views can transition to Split layout when the user clicks an entity reference — the detail panel slides in over the full-width content. The view starts full-width but supports on-demand split.

```
┌──────────────────────────────────────────────────────┐
│                                                      │
│                   Main Content                       │
│                   (scrollable)                       │
│                                                      │
└──────────────────────────────────────────────────────┘
```

No panels, no splits. Content fills the area. Items expand inline (accordion).

### Split (Main + Detail Panel)

Used by: Explorer (when a node is selected), Specs (when a spec is open), any view with a drill-down active.

```
┌────────────────────────────────┬─────────────────────┐
│                                │                     │
│        Main Content            │   Detail Panel      │
│        (60%)                   │   (40%)             │
│                                │   ┌─ Tabs ────────┐ │
│                                │   │ Info │ Spec │  │ │
│                                │   │ Chat │ Hist │  │ │
│                                │   └───────────────┘ │
│                                │                     │
│                                │   [Pop Out] [✕]     │
└────────────────────────────────┴─────────────────────┘
```

The detail panel slides in from the right when the user clicks an entity (node, agent, MR, task). The main content compresses to 60%. Closing the panel (Esc or ✕) returns to full-width.

**Detail panel tabs (contextual — not all tabs appear for all entities):**

| Tab | When | Content |
|---|---|---|
| **Info** | Always | Entity metadata, properties, status |
| **Spec** | When entity has `spec_path` | Spec content viewer + editor + LLM chat |
| **Chat** | When entity is an agent or has an author agent | Scoped inline chat showing recipient |
| **History** | When entity has modification history | Timeline of changes, conversation turn links |
| **Diff** | When viewing an MR or preview | Side-by-side diff |

**Pop Out:** Opens the entity in a full-width view (replaces content area entirely). Used for complex content that needs more space (large diffs, long specs). Back button returns to the previous layout.

### Canvas + Controls

Used by: Explorer at workspace/repo scope.

```
┌──────────────────────────────────────────────────────┐
│ ┌──────────────────────────────────────────────────┐ │
│ │                                                  │ │
│ │              Graph/View Canvas                   │ │
│ │              (pan, zoom, click)                  │ │
│ │                                                  │ │
│ │                                                  │ │
│ └──────────────────────────────────────────────────┘ │
│ ┌──────────────────────────────────────────────────┐ │
│ │ [Lens: Structural ▾] [View: Boundary ▾]         │ │
│ │ [Search /________] [Ask: ____________________]  │ │
│ └──────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────┘
```

The canvas fills most of the content area. An optional **in-view filter panel** (left side, 200px, collapsible) provides category filters from `system-explorer.md` (Boundaries, Interfaces, Data, Specs). This is inside the Explorer content area, NOT part of the app sidebar. It toggles via a filter icon in the control bar.

Below the canvas, a control bar with:
- Lens selector (Structural / Evaluative / Observable)
- View selector (Boundary / Spec Realization / Change / saved views / LLM-generated)
- Search input (`/` to focus — canvas-local search, highlights matching nodes)
- Ask input (natural language → LLM generates a view)

When a node is clicked, the Split layout activates (canvas compresses to 60%, detail panel at 40%).

### Editor Split

Used by: Meta-specs preview loop, Spec editing with preview.

```
┌────────────────────────────────┬─────────────────────┐
│                                │                     │
│   Editor (markdown)            │   Preview           │
│                                │                     │
│   [Manual edit]                │   Architectural     │
│   [Chat: "Add error           │   Impact Diff       │
│    handling for retry"]        │                     │
│                                │   + RetryPolicy     │
│   ┌─ LLM Draft ─────────┐     │   ~ PaymentPort     │
│   │ Suggested changes:   │     │     +retry_payment  │
│   │ + "## Error Handling │     │   = 47 unchanged    │
│   │   On the 4th retry.. │     │                     │
│   │ [Accept] [Edit] [✕]  │     │   [Architecture]    │
│   └──────────────────────┘     │   [Code Diff]       │
│                                │                     │
│   [Preview] [Publish]          │                     │
└────────────────────────────────┴─────────────────────┘
```

Left panel: spec or meta-spec editor with two modes:
- **Manual edit:** standard markdown editor with toolbar.
- **LLM chat:** inline input below the editor. "Add error handling requirements for the retry case" → LLM produces a draft revision shown as an inline diff block with Accept/Edit/Dismiss. Accepting applies the changes to the editor.

Right panel: preview of architectural impact:
- **Architecture tab (default):** structural diff from the knowledge graph — what types/interfaces/modules would change. Generated by extracting the graph from the preview branch and computing a delta against the current graph.
- **Code Diff tab:** traditional line-level diff of the agent's implementation on the throwaway branch.

The architecture view is the primary output. The code diff is secondary.

---

## 3. Interaction Patterns

### Scope Transitions

When the user changes scope (clicks a breadcrumb segment, selects a workspace, clicks a repo):

1. Breadcrumb updates immediately.
2. Content area cross-fades (150ms opacity transition) to the new scope's content.
3. Sidebar active item doesn't change (you stay in Explorer/Inbox/etc.).
4. URL updates via `history.pushState`.

No full-page reload. No sidebar change. The mental model: you're zooming in/out on the same system.

### Drill-Down (Entity Detail)

Click any entity reference (agent name, MR title, task ID, spec path, graph node):

1. Detail panel slides in from the right (200ms ease-out).
2. Main content compresses to 60%.
3. Panel shows contextual tabs based on entity type.
4. Clicking another entity replaces the panel content (no stacking).
5. Esc or ✕ closes the panel, main content returns to full-width.

**Double-click** on a graph node in the Explorer: drill down to the next C4 level (workspace → repo → crate → module). This changes the canvas content, not the panel.

### Inline Expansion (Inbox/Briefing)

Inbox items and Briefing sections expand inline:

1. Click an Inbox item → it expands below the header row, showing the action context.
2. The expanded content includes the relevant data (spec content, diff, gate output) and action buttons.
3. Click again or click another item → current item collapses, new one expands.
4. Only one item expanded at a time (accordion).

No modals. No navigation away. The Inbox is designed to be cleared without leaving the view.

### Contextual Chat

Chat input appears at the bottom of the detail panel (or below the Briefing) with an explicit recipient indicator:

```
┌─────────────────────────────────────┐
│ Message to worker-12 ▸             │
│ [____________________________________] [Send]
└─────────────────────────────────────┘
```

The recipient line shows exactly who/what is receiving the message:
- `Message to worker-12 ▸` — Directed message to a specific agent
- `Message to workspace orchestrator ▸` — Directed message to the orchestrator
- `Ask about this briefing ▸` — LLM Q&A grounded in briefing data
- `Ask about RetryPolicy ▸` — LLM Q&A grounded in the selected node's context
- `Edit spec: "Add error handling..." ▸` — LLM spec editing assistance

Different recipients have different capabilities:
- Agent messages: signed, persisted, delivered via message bus
- LLM Q&A: read-only, cannot trigger actions, grounded in specific data
- Spec editing: produces draft revisions, human must Accept to apply

### LLM-Assisted Spec Editing

In the Spec tab of any detail panel, or in the Editor Split layout:

1. Human types a change request in the chat input: "Add a section on error handling for the timeout case"
2. LLM reads the current spec + knowledge graph context (what code exists for this spec)
3. LLM produces a draft revision shown as an inline diff block in the editor:
   ```
   ┌─ Suggested Change ──────────────────────────┐
   │  ## Error Handling                           │
   │                                              │
   │  When the retry count exceeds `max_attempts`:│
   │  - The system MUST return a structured error │
   │    with the original payment ID and attempt  │
   │    count.                                    │
   │  - Timeout for the final attempt: 30 seconds │
   │    (configurable via `PAYMENT_RETRY_TIMEOUT`)│
   │                                              │
   │  [Accept] [Edit] [Dismiss]                   │
   └──────────────────────────────────────────────┘
   ```
4. **Accept:** applies the change to the spec editor. If preview is active, triggers agent re-implementation on throwaway branch.
5. **Edit:** copies the suggested text into the editor for manual refinement.
6. **Dismiss:** removes the suggestion.

Multiple suggestions can be requested. Each appears as a separate inline diff block. The human curates the spec — the LLM assists but never writes directly.

---

## 4. View Specification Grammar

The Explorer renders views from a declarative JSON specification. This grammar is the interface through which LLMs create visualizations and humans save/share views.

### Structure

```json
{
  "name": "How authentication works",
  "description": "Authentication flow from request to identity resolution",

  "data": {
    "concept": "auth",
    "node_types": ["Module", "Function", "Type", "Endpoint"],
    "edge_types": ["Contains", "Implements", "RoutesTo"],
    "depth": 2,
    "filter": {
      "min_churn": 0,
      "spec_path": null,
      "visibility": null
    },
    "repo_id": null
  },

  "layout": "hierarchical",

  "encoding": {
    "color": {"field": "node_type", "scale": "categorical"},
    "size": {"field": "churn_count_30d", "scale": "linear", "range": [24, 64]},
    "border": {
      "field": "spec_confidence",
      "scale": {"high": "#22c55e", "medium": "#eab308", "low": "#f97316", "none": "#ef4444"}
    },
    "opacity": {"field": "visibility", "scale": {"public": 1.0, "private": 0.4}},
    "label": "qualified_name",
    "group_by": "file_path"
  },

  "annotations": [
    {"node_name": "require_auth_middleware", "text": "Entry point — validates all tokens"},
    {"node_name": "AuthenticatedAgent", "text": "Resolves caller identity from JWT"}
  ],

  "explanation": "Authentication flows through require_auth_middleware which validates..."
}
```

### Data Layer

Specifies what to pull from the knowledge graph.

| Field | Type | Description |
|---|---|---|
| `concept` | `Option<String>` | Concept search — matches node `name` or `qualified_name` (case-insensitive substring) |
| `node_types` | `Vec<String>` | Filter to these node types. Empty = all types. |
| `edge_types` | `Vec<String>` | Include only these edge types. Empty = all edges. |
| `depth` | `u32` | Traversal depth from matching nodes (0 = matched nodes only, 1 = +neighbors, etc.) |
| `filter.min_churn` | `Option<u32>` | Only nodes with `churn_count_30d >= min_churn` |
| `filter.spec_path` | `Option<String>` | Only nodes linked to this spec |
| `filter.visibility` | `Option<String>` | Only nodes with this visibility (`public`, `private`) |
| `repo_id` | `Option<String>` | Scope to a single repo. Null = all repos in workspace. |

The data layer maps to knowledge graph API endpoints:
- `repo_id` set + `concept` → `GET /repos/:id/graph/concept/:name` (single repo)
- `repo_id` null + `concept` → `GET /workspaces/:id/graph` (workspace-aggregated graph, client-side concept filtering)
- `spec_path` → `GET /repos/:id/graph/spec/:path`
- No concept/spec_path → `GET /repos/:id/graph` or `GET /workspaces/:id/graph`
- `node_types`/`edge_types` → client-side filtering on the response

### Layout Layer

Specifies spatial arrangement.

| Layout | Description | Best for |
|---|---|---|
| `"graph"` | Force-directed graph (d3-force). Nodes attract/repel based on edges. | Relationships, dependencies |
| `"hierarchical"` | Top-down tree layout (dagre/ELK). Parent-child containment. | Module structure, C4 drill-down |
| `"layered"` | Horizontal layers (ELK layered). Nodes in columns by depth. | Data flow, request pipelines |
| `"list"` | Sortable, filterable table. One row per node. Columns from encoding. | Flat collections, search results |
| `"timeline"` | Nodes on a horizontal time axis by `last_modified` or delta timestamp. | Change history, evolution |
| `"split"` | Two sub-views side by side. Used for spec realization and diffs. | Comparison, before/after |
| `"diff"` | Structural diff — added/modified/removed nodes between two graph snapshots. | Preview impact, change review |

**Composability:** A `"split"` layout contains two sub-view specs:

```json
{
  "layout": "split",
  "left": {
    "data": {"spec_path": "system/payment-retry.md"},
    "layout": "list",
    "encoding": {"label": "name", "color": {"field": "node_type"}}
  },
  "right": {
    "data": {"repo_id": "...", "node_types": ["Type", "Function"]},
    "layout": "hierarchical"
  }
}
```

This enables the LLM to compose visualizations from primitives — a graph next to a table, a timeline with embedded lists, a spec alongside its realization graph.

### Encoding Layer

Maps data attributes to visual properties.

| Property | Accepted fields | Description |
|---|---|---|
| `color` | `node_type`, `spec_confidence`, `visibility`, any custom field | Node fill color. Categorical or ordinal scale. |
| `size` | `churn_count_30d`, `complexity` | Node size. Linear scale with min/max range. `fan_out` and `fan_in` are computed by the risks endpoint (`GET /repos/:id/graph/risks`) and available when the Evaluative lens is active. |
| `border` | `spec_confidence`, any field | Node border color/style. |
| `opacity` | `visibility`, `spec_confidence` | Node transparency. |
| `label` | `name`, `qualified_name`, `file_path` | Text displayed on/below the node. |
| `group_by` | `file_path`, `node_type`, `spec_path` | Visual grouping (background rectangle around group). |
| `edge_color` | `edge_type` | Edge stroke color by type. |
| `edge_style` | `edge_type` | Edge stroke style (solid, dashed, dotted). |

All encoding fields reference `GraphNode` or `GraphEdge` attributes from `realized-model.md`. The Explorer renderer reads the encoding spec and maps graph data to visual properties. No arbitrary code execution.

### Extensibility

New layout types can be added without modifying the Explorer core:

1. Define a new layout name (e.g., `"sankey"`, `"matrix"`, `"chord"`).
2. Implement a renderer component that accepts the data + encoding spec.
3. Register it in the layout registry.

The view spec grammar is open for new layouts but closed for modification — existing layouts never change semantics. The LLM's training data for the grammar can be updated when new layouts are added.

### LLM Constraints

When the LLM generates a view spec:
- It has read-only access to the knowledge graph API.
- It can only produce view specs within this grammar.
- It cannot execute arbitrary code, create tasks, or trigger agent actions.
- If a question cannot be answered within the grammar, the LLM says so and suggests the closest possible view.
- The generated spec is shown to the human before rendering (a "preview" step) so the human can modify it.

---

## 5. Explorer at Each Scope

### Tenant Scope — Workspace Cards

Not a canvas. A grid of cards, one per workspace:

```
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│ Payments         │  │ Platform Core   │  │ Mobile App      │
│                  │  │                 │  │                 │
│ 3 repos          │  │ 6 repos         │  │ 2 repos         │
│ 5 active agents  │  │ 12 active agents│  │ 0 active agents │
│ Budget: 67%      │  │ Budget: 89%     │  │ Budget: 12%     │
│ Trust: Guided    │  │ Trust: Autonomous│ │ Trust: Supervised│
│                  │  │                 │  │                 │
│ [Enter Workspace]│  │ [Enter Workspace]│ │ [Enter Workspace]│
└─────────────────┘  └─────────────────┘  └─────────────────┘
```

Data from existing endpoints: `GET /api/v1/workspaces` + `GET /api/v1/workspaces/:id/budget`.

### Workspace Scope — Realized Architecture

Default view: Boundary View (C4 Level 1). Graph canvas showing repos as nodes, cross-repo dependencies as edges. Click a repo node → drill down to Level 2 (crates within that repo). The detail panel shows repo metadata, recent activity, and spec summary.

The lens selector, view selector, search, and ask input are in the control bar below the canvas.

### Repo Scope — Architecture Detail

Default view: Boundary View (C4 Level 2 — crates/packages). Further drill-down to modules (Level 3) and types (Level 4). The detail panel shows type/function detail with spec linkage, churn metrics, and conversation turn provenance.

The Code sub-view (branches, commits, MRs, merge queue) is accessed via a tab in the control bar alongside the lens/view selectors:

```
[Architecture] [Code] | [Lens: Structural ▾] [View: Boundary ▾]
```

---

## 6. Inbox Layout

### Item Structure

Each Inbox item is a card with consistent structure:

```
┌─ Priority Badge ─────────────────────────────────────────┐
│ [!] Agent needs clarification                      2m ago │
│     worker-8 on auth-refactor (spec: identity-security)   │
│                                                           │
│  ▼ Expand for details                                     │
└───────────────────────────────────────────────────────────┘
```

Expanded (accordion):

```
┌─ Priority Badge ─────────────────────────────────────────┐
│ [!] Agent needs clarification                      2m ago │
│     worker-8 on auth-refactor (spec: identity-security)   │
│                                                           │
│  Uncertainty:                                             │
│  "Token refresh for offline clients not covered by spec.  │
│   Used 30s timeout as default."                           │
│                                                           │
│  Related spec: specs/system/identity-security.md          │
│  Agent: worker-8 | Persona: backend-dev v4                │
│                                                           │
│  [Respond to Agent] [View Spec] [Open in Explorer]        │
│  [Dismiss — accept agent's decision]                      │
└───────────────────────────────────────────────────────────┘
```

### Action Buttons per Item Type

| Priority | Item Type | Primary Actions |
|---|---|---|
| 1 | Agent clarification | Respond to Agent, View Spec, Dismiss (accept decision) |
| 2 | Spec approval | Approve, Reject, Open Spec |
| 3 | Gate failure | View Diff, View Output, Retry, Override, Close MR |
| 4 | Cross-workspace change | Review Changes, Dismiss |
| 5 | Conflicting interpretations | View Both, Pick A / Pick B, Reconcile |
| 6 | Meta-spec drift | View Results, Adjust Meta-spec |
| 7 | Budget warning | Increase Limit, Pause Work |
| 8 | Trust suggestion | Increase Trust, Dismiss |
| 9 | Spec assertion failure | View Code, Update Spec |
| 10 | Suggested link | Confirm, Dismiss |

---

## 7. Briefing Layout

Full-width. No split panel. Narrative flows top-to-bottom with section headers:

```
┌──────────────────────────────────────────────────────────┐
│ Briefing                    Since: 14 hours ago  [▾ 24h] │
│ Workspace: Payments         Trust: Guided                 │
├──────────────────────────────────────────────────────────┤
│                                                          │
│ COMPLETED                                                │
│ ✓ Payment retry logic (spec: payment-retry.md)           │
│   3 MRs merged. All gates passed.                        │
│   Decision: exponential backoff (confidence: high)       │
│                                                          │
│ IN PROGRESS                                              │
│ ◐ Auth refactor (spec: identity-security.md)             │
│   3/5 sub-specs complete. 2 agents active.               │
│   ⚠ worker-8 uncertain: "token refresh for offline..."   │
│   [Respond to worker-8] [View spec]                      │
│                                                          │
│ CROSS-WORKSPACE                                          │
│ ↔ platform-core updated idempotent-api.md                │
│   Your payment-retry.md depends on it.                   │
│   [Review changes] [Dismiss]                             │
│                                                          │
│ EXCEPTIONS                                               │
│ ✗ Gate failure: billing-service MR #47                   │
│   cargo test failed (3 tests).                           │
│   [View Diff] [View Output] [Override] [Close MR]        │
│                                                          │
│ METRICS                                                  │
│ 12 MRs │ 47 runs │ $23.40 │ Budget: 67%                 │
│                                                          │
├──────────────────────────────────────────────────────────┤
│ Ask about this briefing ▸ [_________________________]     │
└──────────────────────────────────────────────────────────┘
```

Action buttons are inline within the narrative. Clicking an entity name (spec, agent, MR) opens the detail panel (Split layout) over the Briefing.

---

## 8. Meta-specs Preview Loop Layout

Uses the Editor Split layout (§2):

**State 1: Editing**

```
┌────────────────────────────┬─────────────────────────────┐
│ Persona: backend-dev v4    │ Target Specs:               │
│                            │                             │
│ ## System Prompt           │ ☑ payment-retry.md          │
│ You are a backend...       │ ☑ charge-processing.md      │
│                            │ ☐ identity-security.md      │
│ [Edit manually]            │ ☐ search.md                 │
│                            │                             │
│ Chat: "Make error handling │                             │
│  more defensive" ▸        │                             │
│ [_______________________] │                             │
│                            │                             │
│ [Preview] [Publish]        │                             │
└────────────────────────────┴─────────────────────────────┘
```

**State 2: Preview Running**

```
┌────────────────────────────┬─────────────────────────────┐
│ Persona: backend-dev v4    │ Preview: Running             │
│ (locked during preview)    │                             │
│                            │ ◐ payment-retry.md          │
│ ## System Prompt           │   Agent implementing...      │
│ You are a backend...       │ ✓ charge-processing.md      │
│ + "Handle all errors       │   Complete                   │
│    with Result<T, E>..."   │                             │
│                            │ Progress: 1/2 specs          │
│                            │                             │
│                            │ [Cancel Preview]             │
└────────────────────────────┴─────────────────────────────┘
```

**State 3: Preview Complete**

```
┌────────────────────────────┬─────────────────────────────┐
│ Persona: backend-dev v4    │ [Architecture] [Code Diff]   │
│                            │                             │
│ ## System Prompt           │ Architectural Impact:        │
│ You are a backend...       │                             │
│ + "Handle all errors       │ + ErrorHandler module        │
│    with Result<T, E>..."   │   (payment-domain)           │
│                            │                             │
│                            │ ~ ChargeService              │
│ LLM suggestion:            │   +3 error result returns    │
│ ┌─ Draft ───────────┐      │   -2 panic! calls removed    │
│ │ "Consider adding   │      │                             │
│ │  a section on..."  │      │ ~ RetryPolicy                │
│ │ [Accept] [Dismiss] │      │   +1 new error variant       │
│ └────────────────────┘      │                             │
│                            │ = 45 types unchanged         │
│ [Iterate] [Publish]        │                             │
└────────────────────────────┴─────────────────────────────┘
```

The Architecture tab is the default — it shows structural impact derived from the knowledge graph delta between the current code and the preview branch. The Code Diff tab shows traditional line-level changes.

**Iterate:** returns to State 1 with the preview results still visible. The human can adjust the persona and preview again. Each iteration is cheap (agents re-implement on throwaway branches).

**Publish:** enters the spec approval flow. The persona change becomes the published version. Reconciliation begins across the workspace.

---

## 9. Rendering Technology

### Canvas Rendering

The Explorer canvas uses **SVG** for graph/hierarchical/layered layouts (rendered by Svelte components, positioned by ELK or dagre layout algorithms). SVG is chosen over Canvas/WebGL because:
- DOM elements are individually addressable (click events, tooltips, accessibility)
- Svelte's reactive model works naturally with SVG elements
- The node count per view is typically <500 (after C4 drill-down filtering)
- CSS transitions work natively for visual state changes

For views exceeding ~1000 nodes, the Explorer shows a warning and suggests applying filters or drilling down. Virtual scrolling is used for list layouts.

### Layout Engines

| Layout | Engine | Notes |
|---|---|---|
| `graph` | d3-force (Svelte wrapper) | Force simulation with configurable forces |
| `hierarchical` | ELK (elkjs, WASM) | Layered hierarchical layout |
| `layered` | ELK (elkjs, WASM) | Left-to-right layered layout |
| `list` | Native Svelte `{#each}` | Sortable table with virtual scroll |
| `timeline` | d3-scale (time axis) + Svelte | Nodes positioned on time axis |
| `split` | CSS Grid (50/50 or 60/40) | Two sub-views rendered independently |
| `diff` | Custom Svelte component | Added/modified/removed node cards |

### Interaction Events

All interactive elements (nodes, edges, rows, cards) emit standardized events:

```typescript
interface ViewEvent {
  type: 'click' | 'dblclick' | 'hover' | 'context-menu';
  entity_type: string;  // 'node', 'edge', 'spec', 'agent', 'mr', 'task'
  entity_id: string;
  position: { x: number, y: number };  // for context menus
}
```

The Explorer shell handles all events uniformly — `click` opens the detail panel, `dblclick` drills down, `hover` shows a tooltip, `context-menu` opens a context menu with actions.

---

## Relationship to Existing Specs

**Companion to:** `human-system-interface.md` — this spec defines the spatial structure for the interaction model defined there.

**Extends:** `system-explorer.md` — adds the view specification grammar, layout engine choices, and canvas rendering approach.

**Uses:**
- Design system from `web/src/lib/design-system.css` (Red Hat brand, M8.1) — colors, typography, component library
- Knowledge graph API from `realized-model.md` — data source for all Explorer views
- Message bus from `message-bus.md` — real-time updates via WebSocket subscription
- ABAC from `abac-policy-engine.md` — all new endpoints require `RouteResourceMapping`

**New endpoints introduced:**
- Explorer view CRUD (defined in `human-system-interface.md` §3)
- Presence query (defined in `human-system-interface.md` §7)
- Conversation retrieval (defined in `human-system-interface.md` §5)

No new API endpoints are introduced by this spec — all data flows use existing or `human-system-interface.md`-defined endpoints.
