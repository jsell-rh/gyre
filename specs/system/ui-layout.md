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
- Right: user avatar dropdown (profile, settings, sign out), Inbox badge count (shows the number of unresolved Inbox items — this is a shortcut to the Inbox nav item, not a separate notification system).

**Sidebar** (240px width, collapsible to 48px icon-only via toggle):
- Six nav items, always in this order, always present. Active item highlighted.
- At the bottom: server version indicator.

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

Used by: Inbox, Briefing, Specs (list), Meta-specs (catalog), Admin tabs, Explorer at tenant scope (workspace cards). Note: full-width views can transition to Split layout when the user clicks an entity reference — the detail panel slides in over the full-width content. The view starts full-width but supports on-demand split.

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
| **Spec** | When entity has `spec_path` (graph nodes, agents) | Spec content viewer + editor + LLM chat. Note: when opening a spec entity directly from the Specs view (§6), the detail panel uses the richer tab set: Content, Edit, Progress, Links, History. The "Spec" tab here is the summary version for non-spec entities. |
| **Chat** | When entity is an agent or has an author agent | Scoped inline chat showing recipient |
| **History** | When entity has modification history | Timeline of changes, conversation turn links |
| **Diff** | When viewing an MR | Side-by-side code diff |
| **Gates** | When viewing an MR | Gate execution results |
| **Attestation** | When viewing a merged MR | Merge attestation bundle + conversation provenance |
| **Ask Why** | When entity has an author agent with `conversation_sha` | Tab content: initially shows a "Start interrogation" button. Clicking it spawns the interrogation agent (per HSI §4) and the tab becomes an inline chat with the spawned agent. Disabled with tooltip "Conversation unavailable" when `conversation_sha` is null. |

Note: The tab set is contextual per entity type. An MR shows Info/Diff/Gates/Attestation/Ask Why. A graph node shows Info/Spec/Chat/History. An agent shows Info/Chat/History. A spec entity (opened from the Specs view §6) replaces the generic Info/Spec tabs with the richer set: Content/Edit/Progress/Links/History — the "Info" tab is subsumed by "Content" which includes metadata. The tabs listed above are the superset — only applicable tabs appear.

**Pop Out:** Expands the detail panel to full-width (replaces the main content temporarily). The sidebar and breadcrumb remain — the user is still in the same nav context. The URL updates to include the entity ID as a query param (e.g., `/workspaces/:id/explorer?detail=agent:uuid&expanded=true`) so the state is deep-linkable and shareable. Back button or Esc returns to the split layout. Entity views from `platform-model.md` §9 (Task Board, Agent List, etc.) are accessed exclusively through this drill-down pattern, never as primary navigation items.

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
- Ask input (natural language → `POST /api/v1/workspaces/:workspace_id/explorer-views/generate`). Request: `{question: "How does auth work?", repo_id?: "<uuid>"}`. Response (200): `{view_spec: {...}, explanation: "..."}`. On error or unanswerable question: the `event: complete` SSE event carries `{view_spec: null, explanation: "I cannot visualize that — here's why...", fallback: {layout: "list", ...}}` — same streaming mechanism, the null `view_spec` signals the client to render the fallback. If the LLM connection fails entirely, `event: error` fires instead (no fallback available). The server sends the LLM: the question, the list of available node types and counts in the workspace, and the view spec grammar schema. The LLM produces a view spec or explains why it can't. The generated view is **ephemeral** (not auto-saved) — the user can save it explicitly via the saved views CRUD. Requires workspace membership. ABAC resource type: `explorer_view`, action: `generate` (explicit action override in `RouteResourceMapping` — overrides the default POST→write mapping, distinct from `write` used by saved view CRUD). **Budget:** LLM calls from `generate`, `briefing/ask`, and `specs/assist` endpoints are charged to the workspace budget as `llm_query` cost entries (using the existing cost tracking from `analytics.md`). Rate limited to 10 requests/minute per user per workspace, enforced by a per-handler rate limiter that runs after auth (unlike the global rate limiter which runs before auth). The handler extracts `(user_id, workspace_id)` from the auth context and uses an in-memory sliding window counter (evicted after 60s of inactivity). Exceeding the limit returns 429 with `Retry-After` header.

### LLM Endpoint Contract

All three LLM endpoints (`explorer-views/generate`, `briefing/ask`, `specs/assist`) share these behaviors:

**Streaming:** All responses stream via Server-Sent Events (SSE). The client sends a POST, the server responds with `Content-Type: text/event-stream`. Events:
- `event: partial` — incremental text chunks (for `explanation` and `answer` fields only — structured data like `view_spec` and `diff` arrays are not streamed incrementally)
- `event: complete` — final complete JSON response (view_spec, diff, or answer as a single unit)
- `event: error` — error message if the LLM fails

The client renders incrementally as `partial` events arrive (explanation text appears progressively). The `complete` event carries the full response for client-side caching. If the connection drops before `complete`, the client retries with the same request (idempotent — same question produces same view).

**Prompt storage and versioning:** Each endpoint uses a **system prompt template** stored in the repo as a versioned spec file:

```
specs/prompts/
  explorer-generate.md    — system prompt for view generation
  briefing-ask.md         — system prompt for briefing Q&A
  specs-assist.md         — system prompt for spec editing
```

These prompt templates are:
- **Versioned in git** alongside other specs — changes are tracked and diffable. Prompt templates are in `specs/prompts/` which should be added to `spec-lifecycle.md`'s `ignored_paths` — prompt iteration should be fast and not require formal spec approval. The git SHA is recorded for audit, not for approval gating.
- **Parameterized** with runtime context (workspace graph summary, current spec content, etc.) injected at call time
- **Bound to the workspace's meta-spec-set** — if the workspace has custom principles or standards, those are injected into the prompt context
- **Auditable** — the prompt template version (git SHA) is recorded in cost entries for reproducibility

The prompt template format:

```markdown
# Explorer View Generation Prompt

## Role
You are an architecture visualization assistant for the Gyre platform.

## Available Data
You have access to the knowledge graph for workspace "{{workspace_name}}".
Node types available: {{node_type_summary}}
Total nodes: {{node_count}}

## Output Format
Produce a JSON view specification matching this grammar:
{{view_spec_grammar}}

## Constraints
- Only reference node types that exist in the graph
- Keep depth <= 3 to avoid overwhelming the canvas
- Prefer hierarchical layout for containment questions, graph for relationships
```

Variables enclosed in `{{...}}` are substituted at runtime by the server. The template itself is static text committed to git.

**Model selection:** The server selects the LLM model based on workspace configuration (stored on the Workspace entity as `llm_model: Option<String>`, defaulting to the server's `GYRE_LLM_MODEL` env var). This allows different workspaces to use different models (e.g., a cost-sensitive workspace uses a smaller model).

**Token limits:** Each endpoint has a configurable max output token limit:
- `explorer-views/generate`: 2,000 tokens (view specs are compact JSON)
- `briefing/ask`: 4,000 tokens (narrative answers can be longer)
- `specs/assist`: 4,000 tokens (spec diffs can be substantial)

These are configurable via `GYRE_LLM_MAX_TOKENS_GENERATE`, `GYRE_LLM_MAX_TOKENS_ASK`, `GYRE_LLM_MAX_TOKENS_ASSIST`.

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
- **Architecture tab (default):** structural diff from the knowledge graph — what types/interfaces/modules would change. Generated by extracting the graph from the preview branch and computing a delta against the current graph (uses the existing `ArchitecturalDelta` mechanism, NOT the `graph/predict` endpoint which is for the deferred ghost overlay feature in `system-explorer.md` §3). The diff shows concrete added/modified/removed nodes, not probabilistic predictions.
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

**Double-click** on a graph node in the Explorer: drill down to the next C4 level. This **changes scope** — double-clicking a repo node at workspace scope transitions to repo scope. The breadcrumb updates, the URL changes via `history.pushState` (e.g., `/workspaces/:ws_id/explorer` → `/repos/:repo_id/explorer`), and the canvas re-renders for the new scope level. Single-click opens the detail panel without changing scope.

### Inline Expansion (Inbox/Briefing)

Inbox items and Briefing sections expand inline:

1. Click an Inbox item → it expands below the header row, showing the action context.
2. The expanded content includes the relevant data (spec content, diff, gate output) and action buttons.
3. Click again or click another item → current item collapses, new one expands.
4. Only one item expanded at a time (accordion).

No modals. The Inbox is designed to be cleared without leaving the view. Exception: clicking an entity reference within an expanded item (e.g., "View Spec", agent name, MR link) opens the detail panel in Split layout — the Inbox content compresses to 60% and the entity detail appears at 40%. This is the standard drill-down pattern from §2, not "navigating away" — closing the panel returns to full-width Inbox.

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
2. Frontend calls `POST /api/v1/repos/:repo_id/specs/assist` — request: `{spec_path, instruction, draft_content?: string}`, response: `{diff: [{op: "add"|"remove"|"replace", path: string, content: string}], explanation}`. The `op` values: `add` (insert new content at `path` position), `remove` (delete content at `path`), `replace` (substitute content at `path` with `content`). `path` is a markdown section header string (e.g., `"## Error Handling"`) identifying the section to modify. For additions, `path` is the header after which to insert. For removes/replaces, `path` identifies the section whose content is targeted. If no matching header exists, `path` falls back to a line range `"L15-L22"`. If `draft_content` is provided, the LLM uses it instead of the committed spec (allows editing assistance on unsaved changes). ABAC: `RouteResourceMapping` with `resource_type: "spec"`, `repo_param: "repo_id"`, `action_override: "generate"` (the endpoint only produces suggestions, not writes — same rationale as `explorer-views/generate`). The LLM reads the current spec + knowledge graph context
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
4. **Accept:** applies the change to the spec editor content (in-memory). The edit is NOT committed to git yet — the human must explicitly save. **Save workflow:** clicking Save commits the spec change to a feature branch named `spec-edit/<spec_path_slug>-<short_uuid>` (e.g., `spec-edit/system-payment-retry-a1b2`). The slug is derived from the spec path by replacing `/` with `-`, stripping the `.md` extension, and lowercasing (e.g., `system/payment-retry.md` → `system-payment-retry`). The server creates the branch, commits the change, and auto-creates an MR targeting the default branch. The MR itself creates a priority-2 "Spec pending approval" notification (directly, not via spec lifecycle — spec lifecycle triggers on default-branch pushes, but the human needs to see the pending approval before merging). After approval (via Inbox "Approve" action or spec approval endpoint), the MR is automatically enqueued into the merge queue. The linkage: the `specs/save` handler stores the `mr_id` on the notification entity's `entity_ref` field; the spec approval handler reads `entity_ref` from the notification to find the associated MR and enqueues it. Merging the MR triggers spec lifecycle automation (approval invalidation, task creation). This ensures spec edits always require approval before taking effect. The save endpoint: `POST /api/v1/repos/:repo_id/specs/save` — request: `{spec_path, content, message}`, response: `{branch, mr_id}`. If the user has an existing open `spec-edit/*` MR for the same `spec_path` (matched by querying open MRs where the branch matches `spec-edit/<spec_path_slug>-*` and the MR author is the current user), the save appends a commit to the existing branch rather than creating a new one. **Branch cleanup:** `spec-edit/*` branches are deleted when their MR is merged or closed (standard MR branch cleanup). Abandoned branches (no commits for 7 days, MR still open) are flagged in the Inbox as priority-10 suggested cleanup items. ABAC: `RouteResourceMapping` with `resource_type: "spec"`, `repo_param: "repo_id"`, `action: "write"`.
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
| `concept` | `Option<String>` | Concept search — matches node `name` or `qualified_name` (case-insensitive substring). This is a text search, not a manifest-declared concept view from `realized-model.md` §4 — the two are different mechanisms. The `GET /repos/:id/graph/concept/:name` endpoint uses the manifest-based concept projections; the view spec `concept` field does substring matching on the graph query response. |
| `node_types` | `Vec<String>` | Filter to these node types. Empty = all types. |
| `edge_types` | `Vec<String>` | Include only these edge types. Empty = all edges. |
| `depth` | `u32` | Traversal depth from matching nodes (0 = matched nodes only, 1 = +neighbors, etc.) |
| `filter.min_churn` | `Option<u32>` | Only nodes with `churn_count_30d >= min_churn` |
| `filter.spec_path` | `Option<String>` | Only nodes linked to this spec |
| `filter.visibility` | `Option<String>` | Only nodes with this visibility (`public`, `private`) |
| `repo_id` | `Option<String>` | Scope to a single repo. Null = all repos in workspace. **Validated:** the server rejects saved views where `repo_id` does not belong to the workspace in the URL (prevents cross-workspace data leakage). |

The data layer maps to knowledge graph API endpoints:
- `repo_id` set + `concept` → `GET /repos/:id/graph?concept=:name` (single repo, substring search via query param — distinct from `GET /repos/:id/graph/concept/:name` which serves manifest-based concept projections)
- `repo_id` null + `concept` → `GET /workspaces/:id/graph` (workspace-aggregated graph, client-side concept filtering). This is the specified behavior; the workspace-scoped concept endpoint in the HSI upstream amendments table is an optimization, not a prerequisite.
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
| `"side-by-side"` | Two sub-views side by side. Used for spec realization and diffs. Not to be confused with the Split content area layout (§2) which is the main+detail panel pattern. | Comparison, before/after |
| `"diff"` | Structural diff — added/modified/removed nodes between two graph snapshots. | Preview impact, change review |

**Composability:** A `"side-by-side"` layout contains two sub-view specs:

```json
{
  "layout": "side-by-side",
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

**Nesting depth limit:** `side-by-side` sub-views cannot themselves contain `side-by-side` layouts. Maximum composition depth is 1. Validated **both** server-side (the `/explorer-views` CRUD and `/generate` endpoints reject invalid specs with 400) and client-side (the Svelte renderer checks before rendering and shows an error message instead of crashing).

### Encoding Layer

Maps data attributes to visual properties.

| Property | Accepted fields | Description |
|---|---|---|
| `color` | `node_type`, `spec_confidence`, `visibility`, any custom field | Node fill color. Categorical or ordinal scale. |
| `size` | `churn_count_30d`, `complexity`, `fan_out`, `fan_in` | Node size. Linear scale with min/max range. |
| `border` | `spec_confidence`, any field | Node border color/style. |
| `opacity` | `visibility`, `spec_confidence` | Node transparency. |
| `label` | `name`, `qualified_name`, `file_path` | Text displayed on/below the node. |
| `group_by` | `file_path`, `node_type`, `spec_path` | Visual grouping (background rectangle around group). |
| `edge_color` | `edge_type` | Edge stroke color by type. |
| `edge_style` | `edge_type` | Edge stroke style (solid, dashed, dotted). |

All encoding fields reference `GraphNode` or `GraphEdge` attributes from `realized-model.md`.

**Lens-driven data enrichment:** When the Evaluative lens is active, the Explorer automatically fetches supplementary data from two sources:
1. `GET /repos/:id/graph/risks` — provides `fan_out`, `fan_in`, `churn_rate`, `spec_covered` per node
2. `GET /repos/:id/gates` + `GET /api/v1/specs?kind=&workspace_id=` — provides gate pass/fail status and spec assertion state, mapped to nodes via `spec_path`

These are merged into node objects before applying the encoding. The fetch is triggered by lens selection, not by the view spec. If a view spec references Evaluative fields without the lens active, the field is `null` and the encoding falls back to a default value. The Explorer renderer reads the encoding spec and maps graph data to visual properties. No arbitrary code execution.

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
- The generated view is rendered immediately. The view spec JSON is visible and editable in a collapsible panel alongside the view — the human can refine the query/layout/encoding and re-render.

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

Default view: Boundary View. Graph canvas showing repos as nodes, cross-repo dependencies as edges. (This maps to C4 "Container" level — repos are containers within the workspace system. We skip C4 "Context" level because the workspace boundary IS the context.) Click a repo node → detail panel shows repo metadata. Double-click → drill down to crate level (scope changes to repo).

The lens selector, view selector, search, and ask input are in the control bar below the canvas.

### Repo Scope — Architecture Detail

Default view: Boundary View (C4 Level 2 — crates/packages). Further drill-down to modules (Level 3) and types (Level 4). The detail panel shows type/function detail with spec linkage, churn metrics, and conversation turn provenance.

The Code sub-view is accessed via a tab in the control bar. When Architecture is active, the control bar is below the canvas (consistent with §2). When Code is active, the control bar moves to the **top** of the content area (above the table), since there is no canvas:

```
[Architecture] [Code] | [Lens: Structural ▾] [View: Boundary ▾] | [/ Search] [Ask...]
```

**Code tab layout** (Full-Width, replaces canvas — when Code is active, canvas-specific controls (Lens, View, Search, Ask) are hidden; the control bar shows only `[Architecture] [Code]`):

```
┌──────────────────────────────────────────────────────────┐
│ Branches  │  Merge Requests  │  Merge Queue              │
├──────────────────────────────────────────────────────────┤
│                                                          │
│  [Branch list / MR list / Queue entries]                 │
│  (sortable table, one row per entity)                    │
│  Click row → detail panel opens (Split layout)           │
│                                                          │
└──────────────────────────────────────────────────────────┘
```

Three sub-tabs within Code: **Branches** (from `GET /repos/:id/branches` + `GET /repos/:id/commits`), **Merge Requests** (from `GET /merge-requests?repository_id=:id`), **Merge Queue** (from `GET /merge-queue?repository_id=:id`). Each sub-tab renders a sortable table with a filter input at the top (text search on branch name, MR title, or queue entry — replaces the hidden canvas Search control). Clicking a row opens the detail panel with entity-specific tabs (MR detail: Diff/Gates/Attestation/Ask Why).

---

## 6. Specs View Layout

Full-Width layout. Content adapts to scope:

**Tenant/Workspace scope:** Sortable table of specs with columns: Path, Status (Approved/Pending/Deprecated), Kind (if meta-spec), Owner, Last Updated. Filter pills for status and kind. Data from `GET /api/v1/specs?workspace_id=` (workspace) or `GET /api/v1/specs` (tenant).

**Repo scope:** Spec list with implementation progress per spec:

```
┌──────────────────────────────────────────────────────────┐
│ Specs: payment-api                    [+ New Spec]       │
├──────────────────────────────────────────────────────────┤
│ payment-retry.md        Approved ✓   4/5 tasks  ████░   │
│ charge-processing.md    Approved ✓   5/5 tasks  █████   │
│ identity-security.md    Pending ◐    0/3 tasks  ░░░░░   │
│ search.md               Approved ✓   2/4 tasks  ██░░░   │
└──────────────────────────────────────────────────────────┘
```

Click a spec → detail panel opens with tabs: **Content** (markdown viewer), **Edit** (markdown editor + LLM chat via `POST /repos/:id/specs/assist`), **Progress** (task list with status), **Links** (spec link graph for this spec), **History** (approval event timeline).

Data from: `GET /api/v1/specs?workspace_id=` (list), `GET /api/v1/specs/:path?repo_id=` (detail — `repo_id` required to disambiguate spec paths across repos), `GET /api/v1/specs/:path/links?repo_id=` (links — defined in `spec-links.md`), `GET /api/v1/specs/:path/history?repo_id=` (approval event timeline — response: `[{event: "approved"|"invalidated"|"created", user_id, timestamp, sha}]`), `GET /api/v1/specs/:path/progress?repo_id=` (task rollup — response: `{total_tasks, completed_tasks, tasks: [{id, title, status, agent_id}]}`). The `repo_id` query parameter is required on all spec-by-path endpoints because spec paths (e.g., `system/vision.md`) are not globally unique — the same path can exist in different repos. Per-handler auth resolves workspace from the `repo_id` parameter.

---

## 7. Inbox Layout

### Item Structure

Each Inbox item is a card with consistent structure. At tenant scope, each card shows the workspace name in brackets (e.g., `[Payments]`) for context:

```
┌─ Priority Badge ─────────────────────────────────────────┐
│ [!] Agent needs clarification          [Payments]  2m ago │
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
| 1 | Agent clarification | Respond to Agent (opens Split layout — agent detail panel with Chat tab focused), View Spec, Dismiss (accept decision) |
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

## 8. Briefing Layout

Full-width by default. Clicking entity references within the Briefing opens the detail panel (Split layout) per the standard drill-down pattern in §2. Narrative flows top-to-bottom with section headers:

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

## 9. Meta-specs Preview Loop Layout

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

## 10. Rendering Technology

### Canvas Rendering

The Explorer canvas uses **SVG** for graph/hierarchical/layered layouts (rendered by Svelte components, positioned by ELK or dagre layout algorithms). SVG is chosen over Canvas/WebGL because:
- DOM elements are individually addressable (click events, tooltips, accessibility)
- Svelte's reactive model works naturally with SVG elements
- The node count per view is typically <500 (after C4 drill-down filtering)
- CSS transitions work natively for visual state changes

For views exceeding ~500 nodes, the Explorer automatically applies a filter: only public nodes are shown, with a banner "Showing public API only — X private nodes hidden. [Show All]". For views exceeding ~1000 nodes even after filtering, the Explorer switches to list layout with virtual scrolling and a warning. This prevents Level 4 drill-down on large modules from overwhelming the SVG canvas.

### Layout Engines

| Layout | Engine | Notes |
|---|---|---|
| `graph` | d3-force (Svelte wrapper) | Force simulation with configurable forces |
| `hierarchical` | ELK (elkjs, WASM) | Layered hierarchical layout |
| `layered` | ELK (elkjs, WASM) | Left-to-right layered layout |
| `list` | Native Svelte `{#each}` | Sortable table with virtual scroll |
| `timeline` | d3-scale (time axis) + Svelte | Nodes positioned on time axis |
| `side-by-side` | CSS Grid (50/50 or 60/40) | Two sub-views rendered independently |
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

**New API endpoints introduced by this spec:**
- `POST /api/v1/workspaces/:workspace_id/explorer-views/generate` — LLM view generation (§2)
- `POST /api/v1/repos/:repo_id/specs/assist` — LLM-assisted spec editing (§3)
- `POST /api/v1/workspaces/:workspace_id/briefing/ask` — Briefing Q&A (referenced from `human-system-interface.md` §9, layout in §8)

All other data flows use existing endpoints.
