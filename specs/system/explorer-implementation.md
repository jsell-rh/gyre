# Explorer Implementation

> **Status: Draft.** Implements: `explorer-canvas.md`, `view-query-grammar.md`. Depends on: `lsp-call-graph.md` (complete graph data), `human-system-interface.md` (chat/conversational UX). Replaces: current Graph + Flow tabs in MoldableView.

## Overview

The explorer is one view + one chat panel. The view is a semantic zoom treemap that always shows the knowledge graph. The chat panel connects to an LLM agent (Claude Agent SDK) that generates view queries to update the treemap in response to user questions. Users can save any view query as a named view.

There are no tabs, no layout switchers, no separate modes. One canvas, one conversation, one understanding.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│ Browser                                                         │
│  ┌─────────────────────────────┐  ┌──────────────────────────┐  │
│  │ ExplorerCanvas (Svelte)     │  │ ExplorerChat (Svelte)    │  │
│  │                             │  │                          │  │
│  │ Semantic zoom treemap       │  │ User types question      │  │
│  │ Renders view queries        │  │ LLM responds with text   │  │
│  │ Pan/zoom/click/drag         │  │ + view query applied     │  │
│  │                             │  │                          │  │
│  │ Filter presets (All,        │  │ Saved views dropdown     │  │
│  │ Endpoints, Types, Calls,    │  │ "Save this view" button  │  │
│  │ Dependencies)               │  │                          │  │
│  └──────────┬──────────────────┘  └──────────┬───────────────┘  │
│             │ WebSocket                       │ WebSocket        │
└─────────────┼─────────────────────────────────┼─────────────────┘
              │                                 │
┌─────────────┼─────────────────────────────────┼─────────────────┐
│ Gyre Server │                                 │                 │
│             ▼                                 ▼                 │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │ Explorer WebSocket Handler                               │   │
│  │                                                          │   │
│  │ Receives: user message + canvas state                    │   │
│  │ Spawns: Claude Agent SDK query()                         │   │
│  │ Agent: generates view query + explanation                │   │
│  │ Self-check: dry-run loop (up to 3 refinements)          │   │
│  │ Sends: final view query + text to frontend               │   │
│  └──────────────────────────────────────────────────────────┘   │
│                         │                                       │
│                         ▼                                       │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │ Knowledge Graph (GraphPort)                              │   │
│  │ Nodes, edges, fields, test_node flags                    │   │
│  │ Used by: dry-run resolver, graph summary for LLM context │   │
│  └──────────────────────────────────────────────────────────┘   │
│                                                                 │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │ Saved Views (DB)                                         │   │
│  │ id, name, repo_id, workspace_id, query_json, created_by, │   │
│  │ created_at, description                                   │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

## WebSocket Protocol

### Endpoint

```
WS /api/v1/repos/:repo_id/explorer
Authorization: Bearer <token>
```

### Messages: Client → Server

**User message:**
```json
{
  "type": "message",
  "text": "What would break if I change Space?",
  "canvas_state": {
    "selected_node": { "id": "uuid", "name": "Space", "node_type": "type", "qualified_name": "domain.Space" },
    "zoom_level": 1.8,
    "visible_tree_groups": ["coordinator", "domain", "db"],
    "active_filter": "all",
    "active_query": null
  }
}
```

**Save view:**
```json
{
  "type": "save_view",
  "name": "Multi-tenancy impact",
  "description": "Shows blast radius for tenant isolation changes",
  "query": { ... }
}
```

**Load view:**
```json
{
  "type": "load_view",
  "view_id": "uuid"
}
```

**List views:**
```json
{
  "type": "list_views"
}
```

### Messages: Server → Client

**LLM text response (streamed):**
```json
{
  "type": "text",
  "content": "Space is the core domain entity. Changing it impacts...",
  "done": false
}
```

**View query (sent when LLM is satisfied with dry-run):**
```json
{
  "type": "view_query",
  "query": {
    "scope": { ... },
    "emphasis": { ... },
    "groups": [ ... ],
    "callouts": [ ... ],
    "narrative": [ ... ],
    "annotation": { ... }
  }
}
```

**View list:**
```json
{
  "type": "views",
  "views": [
    { "id": "uuid", "name": "Multi-tenancy impact", "description": "...", "created_at": 1234567890 }
  ]
}
```

**Status:**
```json
{
  "type": "status",
  "status": "thinking" | "refining" | "ready"
}
```

## LLM Agent (Claude Agent SDK)

### Agent Configuration

The explorer agent runs via `query()` from `@anthropic-ai/claude-agent-sdk`. It is NOT the same agent that writes code — it's a read-only analysis agent with access to the knowledge graph.

```javascript
import { query } from '@anthropic-ai/claude-agent-sdk';

const options = {
  model: process.env.GYRE_LLM_MODEL || 'claude-sonnet-4-6',
  mcpServers: {
    gyre: {
      type: 'http',
      url: `${serverUrl}/mcp`,
      headers: { Authorization: `Bearer ${token}` },
    },
  },
  allowedTools: [
    'mcp__gyre__graph_summary',
    'mcp__gyre__graph_query_dryrun',
    'mcp__gyre__graph_nodes',
    'mcp__gyre__graph_edges',
    'mcp__gyre__search',
  ],
};
```

### MCP Tools Available to the Agent

**`graph_summary`** — returns a condensed overview of the repo's knowledge graph:
```json
{
  "repo_id": "uuid",
  "node_counts": { "type": 55, "function": 265, "endpoint": 73, "module": 61, "interface": 7 },
  "edge_counts": { "calls": 129, "contains": 684, "field_of": 216, "depends_on": 5 },
  "top_types_by_fields": ["Space (8 fields)", "AgentRecord (12 fields)", "Task (7 fields)"],
  "top_functions_by_calls": ["lifecycleErr.Error (28)", "NewKnowledgeSpace (8)", "NewServer (7)"],
  "modules": ["domain", "coordinator", "db.sqlite", "ports"],
  "test_coverage": { "test_functions": 45, "reachable_from_tests": 180, "unreachable": 85 }
}
```

**`graph_query_dryrun`** — takes a view query JSON, resolves it against the graph, returns the preview:
```json
{
  "query": { ... },
  "result": {
    "matched_nodes": 14,
    "matched_node_names": ["KnowledgeSpace", "NewKnowledgeSpace", "Server", "..."],
    "groups_resolved": [{ "name": "Tenant Boundary", "matched": 3, "nodes": ["..."] }],
    "callouts_resolved": 2,
    "callouts_unresolved": [],
    "narrative_resolved": 3,
    "warnings": ["Group 'Persistence' matched 47 nodes - too broad"]
  }
}
```

**`graph_nodes`** — query specific nodes by ID, name, type, or qualified_name pattern.

**`graph_edges`** — query edges by source/target node or edge type.

**`search`** — full-text search across the graph.

### Agent System Prompt

```
You are the Gyre Explorer agent. You help users understand their codebase
by generating view queries that visualize the knowledge graph.

You have access to the knowledge graph via MCP tools. When the user asks
a question:

1. Call graph_summary to understand the codebase structure
2. Reason about which nodes/edges are relevant to the question
3. Generate a view query JSON using the View Query Grammar
4. Call graph_query_dryrun to check the query
5. If there are warnings (too many matches, unresolved nodes, etc.),
   refine the query and dry-run again (max 3 refinements)
6. When satisfied, output the view query in a <view_query> block
7. Also provide a text explanation of what the visualization shows

Output format:
- Text explanation for the user (conversational, concise)
- <view_query>{ ... JSON ... }</view_query> block that the server
  extracts and sends to the frontend

The canvas state is provided with each user message so you know what
they're currently looking at. Use $selected to reference the node
they've clicked. Use $clicked for interactive queries.

IMPORTANT: Be specific with node names in groups/callouts. Use qualified
names (e.g., "coordinator.KnowledgeSpace" not just "Space") to avoid
matching too many nodes. Always dry-run before finalizing.
```

### Self-Check Loop

The server extracts `<view_query>` blocks from the agent's output, runs the dry-run, and feeds the result back as a tool response. The agent can then refine.

```
Agent output: "Let me check this query..."
  → <view_query>{ ... }</view_query>
  → Server extracts, runs dry-run
  → Server injects tool result: { warnings: ["Group matched 47 nodes"] }
  → Agent: "Too broad. Let me use more specific names..."
  → <view_query>{ ... refined ... }</view_query>
  → Server dry-runs again
  → No warnings
  → Agent: "Here's what I found: ..."
  → <view_query>{ ... final ... }</view_query>
  → Server sends to frontend
```

The server caps this at 3 refinement turns. If the agent hasn't resolved warnings by turn 3, send the best version anyway.

## Saved Views

### DB Schema

```sql
CREATE TABLE saved_views (
  id TEXT PRIMARY KEY,
  repo_id TEXT NOT NULL REFERENCES repositories(id),
  workspace_id TEXT NOT NULL,
  tenant_id TEXT NOT NULL,
  name TEXT NOT NULL,
  description TEXT,
  query_json TEXT NOT NULL,  -- the view query JSON
  created_by TEXT NOT NULL,  -- user or agent ID
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  is_system BOOLEAN NOT NULL DEFAULT FALSE  -- system-provided default views
);
```

### System Default Views

Seeded on repo creation or first sync:

```json
[
  {
    "name": "Architecture Overview",
    "description": "Full codebase structure",
    "query": { "scope": { "type": "all" }, "zoom": "fit" }
  },
  {
    "name": "Test Coverage Gaps",
    "description": "Functions not reachable from any test",
    "query": {
      "scope": { "type": "test_gaps" },
      "emphasis": { "highlight": { "matched": { "color": "#ef4444", "label": "Untested" }}, "dim_unmatched": 0.3 },
      "annotation": { "title": "Test coverage gaps" }
    }
  },
  {
    "name": "Hot Paths",
    "description": "Most-called functions",
    "query": {
      "scope": { "type": "all" },
      "emphasis": { "heat": { "metric": "incoming_calls", "palette": "blue-red" }},
      "annotation": { "title": "Hot paths" }
    }
  },
  {
    "name": "Blast Radius (click)",
    "description": "Click any node to see what it impacts",
    "query": {
      "scope": { "type": "focus", "node": "$clicked", "edges": ["calls", "implements", "field_of", "depends_on"], "direction": "incoming", "depth": 10 },
      "emphasis": { "tiered_colors": ["#ef4444", "#f97316", "#eab308", "#94a3b8"], "dim_unmatched": 0.12 },
      "edges": { "filter": ["calls", "implements", "field_of", "depends_on"] },
      "zoom": "fit",
      "annotation": { "title": "Blast radius: $name" }
    }
  }
]
```

### REST API

```
GET    /api/v1/repos/:id/views           — list saved views
POST   /api/v1/repos/:id/views           — create saved view
GET    /api/v1/repos/:id/views/:view_id  — get view query
PUT    /api/v1/repos/:id/views/:view_id  — update view
DELETE /api/v1/repos/:id/views/:view_id  — delete view
```

## Frontend Components

### ExplorerCanvas (Svelte)

The single canvas component. Replaces `MoldableView` + `ExplorerCanvas` + `FlowRenderer` + `FlowCanvas`.

**Props:**
```typescript
{
  repoId: string;
  nodes: GraphNode[];
  edges: GraphEdge[];
  activeQuery: ViewQuery | null;  // applied view query
  filter: 'all' | 'endpoints' | 'types' | 'calls' | 'dependencies';
  lens: 'structural' | 'evaluative' | 'observable';
}
```

**Responsibilities:**
- Semantic zoom treemap rendering (canvas 2D)
- Pan/zoom/click/drag interaction
- View query resolution and rendering (emphasis, groups, callouts, narrative)
- Filter preset application
- Lens overlay (evaluative particles, observable traffic)
- Minimap

### ExplorerChat (Svelte)

The chat panel on the right side of the explorer.

**Props:**
```typescript
{
  repoId: string;
  canvasState: CanvasState;  // current zoom, selected node, etc.
  onViewQuery: (query: ViewQuery) => void;  // applies query to canvas
  savedViews: SavedView[];
}
```

**Responsibilities:**
- WebSocket connection to `/api/v1/repos/:repo_id/explorer`
- Send user messages with canvas state
- Receive and display LLM text responses (streamed)
- Receive and forward view queries to ExplorerCanvas
- Saved views dropdown + "Save this view" button
- Show refinement status ("thinking...", "refining query...", "ready")

### ExplorerView (Svelte)

The container that holds both panels.

```svelte
<div class="explorer-view">
  <ExplorerCanvas {repoId} {nodes} {edges} {activeQuery} {filter} {lens} bind:canvasState />
  <ExplorerChat {repoId} {canvasState} onViewQuery={q => activeQuery = q} {savedViews} />
</div>
```

## Server Implementation

### Explorer WebSocket Handler

```rust
// crates/gyre-server/src/api/explorer_ws.rs

pub async fn explorer_ws(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    auth: AuthenticatedAgent,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_explorer_session(socket, state, repo_id, auth))
}

async fn handle_explorer_session(socket: WebSocket, state: Arc<AppState>, repo_id: String, auth: AuthenticatedAgent) {
    let (mut sender, mut receiver) = socket.split();

    while let Some(msg) = receiver.next().await {
        match parse_message(msg) {
            ExplorerMessage::UserMessage { text, canvas_state } => {
                // Spawn Claude Agent SDK query
                // Stream text responses back
                // Extract and dry-run view queries
                // Send final view query when agent is satisfied
            }
            ExplorerMessage::SaveView { name, description, query } => {
                // Store in saved_views table
            }
            ExplorerMessage::LoadView { view_id } => {
                // Load from saved_views, send query to frontend
            }
            ExplorerMessage::ListViews => {
                // Query saved_views table, send list
            }
        }
    }
}
```

### Graph Summary MCP Tool

```rust
// New MCP tool: graph_summary
pub async fn graph_summary(state: &AppState, repo_id: &str) -> GraphSummary {
    let nodes = state.graph_store.list_nodes(&Id::new(repo_id), None).await?;
    let edges = state.graph_store.list_edges(&Id::new(repo_id), None).await?;

    // Count by type
    let node_counts = count_by_type(&nodes);
    let edge_counts = count_by_edge_type(&edges);

    // Top types by field count
    let field_edges = edges.iter().filter(|e| e.edge_type == EdgeType::FieldOf).collect::<Vec<_>>();
    let top_types = compute_top_types_by_fields(&nodes, &field_edges);

    // Top functions by incoming calls
    let call_edges = edges.iter().filter(|e| e.edge_type == EdgeType::Calls).collect::<Vec<_>>();
    let top_functions = compute_top_by_incoming_calls(&nodes, &call_edges);

    // Test coverage
    let test_nodes = nodes.iter().filter(|n| n.test_node).count();
    let reachable = compute_test_reachable(&nodes, &call_edges);

    GraphSummary { node_counts, edge_counts, top_types, top_functions, test_coverage: ... }
}
```

### Dry-Run MCP Tool

```rust
// New MCP tool: graph_query_dryrun
pub async fn graph_query_dryrun(state: &AppState, repo_id: &str, query: ViewQuery) -> DryRunResult {
    let nodes = state.graph_store.list_nodes(&Id::new(repo_id), None).await?;
    let edges = state.graph_store.list_edges(&Id::new(repo_id), None).await?;

    let mut warnings = Vec::new();

    // Resolve scope
    let result_set = resolve_scope(&query.scope, &nodes, &edges);
    if result_set.is_empty() { warnings.push("Scope matched 0 nodes".into()); }
    if result_set.len() > 200 { warnings.push(format!("Scope matched {} nodes - may be cluttered", result_set.len())); }

    // Resolve groups
    let groups = resolve_groups(&query.groups, &nodes);
    for g in &groups {
        if g.matched > 20 { warnings.push(format!("Group '{}' matched {} nodes - too broad", g.name, g.matched)); }
    }

    // Resolve callouts + narrative
    let callouts = resolve_callouts(&query.callouts, &nodes);
    let narrative = resolve_narrative(&query.narrative, &nodes);

    DryRunResult { matched_nodes: result_set.len(), groups, callouts, narrative, warnings }
}
```

## Migration Plan

This is green field — the current Graph + Flow tabs are completely replaced. No migration needed, just build the new system and swap.

### Phase 1: Canvas + Filters
- Build `ExplorerCanvas.svelte` from the prototype (`explore3.html`)
- Semantic zoom treemap with path tree hierarchy
- Filter presets (All, Endpoints, Types, Calls, Dependencies)
- Three lenses (structural, evaluative, observable)
- View query renderer (scope, emphasis, groups, callouts, narrative)
- Replace `MoldableView`'s Graph + Flow tabs with this single component

### Phase 2: Chat + Agent
- Build `ExplorerChat.svelte` with WebSocket connection
- Implement explorer WebSocket handler in the server
- Implement `graph_summary` and `graph_query_dryrun` MCP tools
- Wire up Claude Agent SDK for the explorer agent
- Self-check loop (extract view queries, dry-run, refine)

### Phase 3: Saved Views
- DB migration for `saved_views` table
- REST API endpoints
- Saved views dropdown in chat panel
- System default views seeded on repo sync
- "Save this view" button

### Phase 4: Polish
- Streaming text responses in chat
- "Thinking..." / "Refining..." status indicators
- Keyboard shortcuts (Escape to clear query, / to focus chat)
- Mobile/responsive layout
- Performance optimization for large graphs (>10k nodes)
- Visual regression tests

## Testing

### Unit Tests
- View query resolver: scope, emphasis, groups, callouts, narrative
- Dry-run warning generation
- Graph summary computation
- Test coverage gap detection (BFS from test nodes)
- Saved views CRUD

### Integration Tests
- WebSocket explorer session lifecycle
- Agent generates valid view query from question
- Self-check loop catches and fixes overly broad groups
- Saved view round-trip (save → load → render)

### Visual Tests
- Semantic zoom at different zoom levels
- View query rendering (groups, callouts, narrative markers)
- Filter presets show correct subsets
- Blast radius interactive mode

## Relationship to Other Specs

**Implements:** `explorer-canvas.md` (unified canvas), `view-query-grammar.md` (query primitives), `system-explorer.md` §8 (conversational exploration)

**Depends on:** `lsp-call-graph.md` (complete graph data), `human-system-interface.md` (chat UX patterns)

**Replaces:** Current `MoldableView` Graph/Flow tabs, `FlowRenderer`, `FlowCanvas`, `ExplorerFilterPanel`

**Uses:** Claude Agent SDK (`@anthropic-ai/claude-agent-sdk`), existing MCP infrastructure, existing WebSocket infrastructure
