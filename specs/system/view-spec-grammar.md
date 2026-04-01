# View Spec Grammar

> **Status: Draft.** Extends `ui-layout.md` §4. Implements `system-explorer.md` §4 (Concept Views), §8 (Conversational Exploration). Depends on: `explorer-canvas.md` (renderer), `lsp-call-graph.md` (complete graph data).

## Problem

The explorer canvas can render graphs, overlays, particles, diffs, and semantic zoom. But each visualization mode is hardcoded — switching between "blast radius" and "trace flow" and "diff" requires different JavaScript code paths.

Meanwhile, the LLM can answer architectural questions by querying the knowledge graph — but it can only express answers as text. There's no way for the LLM to say "show the user THIS subgraph, colored THIS way, with THESE annotations" and have the explorer render it.

The gap: a **declarative grammar** that both the LLM and the explorer understand. The LLM generates view specs; the explorer renders them. No custom code per question.

## Design

### View Spec Structure

A view spec is a JSON document with three sections: **base** (what to show), **overlays** (how to color/annotate it), and **presentation** (layout and interaction hints).

```json
{
  "version": 1,
  "title": "Blast radius: TaskPort::find_by_id",
  "description": "Changing the return type impacts 14 functions across 4 crates",

  "base": {
    "repo_id": "uuid",
    "lens": "structural",
    "scope": {
      "type": "traversal",
      "root": "gyre-ports::TaskPort::find_by_id",
      "edges": ["calls", "implements"],
      "direction": "both",
      "depth": 3
    }
  },

  "overlays": [
    {
      "type": "highlight",
      "nodes": {
        "gyre-ports::TaskPort::find_by_id": { "color": "#ef4444", "label": "Changed" },
        "gyre-adapters::SqliteTaskRepo::find_by_id": { "color": "#f97316", "label": "Implementor" },
        "gyre-server::api::tasks::get_task": { "color": "#eab308", "label": "Caller" }
      },
      "dim_unmatched": 0.15
    }
  ],

  "presentation": {
    "zoom": "fit-focus",
    "zoom_padding": 1.3,
    "annotation_position": "top-left"
  }
}
```

### Base: What to Show

The `base` section defines the subgraph to visualize.

#### Scope Types

**`all`** — Show the entire repo graph (default at workspace level).
```json
{ "type": "all" }
```

**`traversal`** — Start at a node, follow edges to build a subgraph.
```json
{
  "type": "traversal",
  "root": "qualified_name or node_id",
  "edges": ["calls", "implements", "contains"],
  "direction": "outgoing" | "incoming" | "both",
  "depth": 3,
  "stop_at_types": ["package"]
}
```
This is the primitive for blast radius, dependency analysis, and trace-from-here.

**`filter`** — Show nodes matching criteria.
```json
{
  "type": "filter",
  "node_types": ["endpoint", "function"],
  "spec_path": "specs/system/auth.md",
  "has_edge_type": "calls",
  "min_churn": 5
}
```

**`diff`** — Show changes between two commits.
```json
{
  "type": "diff",
  "from_sha": "abc1234",
  "to_sha": "HEAD"
}
```

**`concept`** — Show a cross-cutting concept by pulling related elements.
```json
{
  "type": "concept",
  "name": "Authentication",
  "seed_nodes": ["AuthService", "TokenStore", "JwtValidator"],
  "expand_edges": ["calls", "implements"],
  "expand_depth": 1
}
```

#### Lens

The `lens` field selects the data layer:
- `"structural"` — static topology, spec linkage, complexity indicators
- `"evaluative"` — OTLP trace data overlay (particles, timing heat)
- `"observable"` — production telemetry overlay (traffic, error rates)

The lens can be combined with any scope type.

### Overlays: How to Color It

Overlays add visual information on top of the base graph. Multiple overlays compose (applied in order).

#### `highlight` — Color specific nodes

```json
{
  "type": "highlight",
  "nodes": {
    "qualified_name": { "color": "#ef4444", "label": "Changed", "badge": "!" }
  },
  "dim_unmatched": 0.15
}
```

#### `heat` — Color all nodes by a metric

```json
{
  "type": "heat",
  "metric": "call_frequency" | "complexity" | "churn_count_30d" | "test_coverage",
  "palette": "blue-red" | "green-red" | "cool-warm",
  "scale": "linear" | "log"
}
```

#### `diff` — Green/red for added/removed

```json
{
  "type": "diff",
  "added_color": "#22c55e",
  "removed_color": "#ef4444",
  "dim_unchanged": 0.3
}
```

#### `badges` — Attach text/numbers to nodes

```json
{
  "type": "badges",
  "data": {
    "qualified_name": { "text": "12 callers", "color": "#ef4444" }
  }
}
```

#### `flow` — Animate particles along paths

```json
{
  "type": "flow",
  "paths": [
    { "from": "POST /agents/spawn", "to": "AgentPort::create", "color": "#60a5fa" },
    { "from": "AgentPort::create", "to": "SqliteAgentRepo::create", "color": "#60a5fa" }
  ],
  "speed": 1.0,
  "continuous": true
}
```

#### `group` — Draw boundaries around node sets

```json
{
  "type": "group",
  "groups": [
    { "name": "Domain Layer", "nodes": ["Task", "Agent", "Repository"], "color": "#22c55e33" },
    { "name": "Adapter Layer", "nodes": ["SqliteTaskRepo", "SqliteAgentRepo"], "color": "#60a5fa33" }
  ]
}
```

### Presentation: Layout Hints

```json
{
  "zoom": "fit-focus" | "fit-all" | "current" | { "level": 2.5 },
  "zoom_padding": 1.3,
  "layout": "column" | "force" | "hierarchical" | "layered",
  "annotation_position": "top-left" | "bottom" | "none",
  "show_legend": true,
  "interactive": true
}
```

### Composition Example: Complex Query

**User asks:** "Show me the auth flow and highlight which parts have low test coverage"

**LLM generates:**
```json
{
  "version": 1,
  "title": "Auth flow with test coverage gaps",
  "description": "The auth flow has 3 components below 50% test coverage",

  "base": {
    "repo_id": "...",
    "lens": "structural",
    "scope": {
      "type": "traversal",
      "root": "POST /auth/login",
      "edges": ["calls", "routes_to"],
      "direction": "outgoing",
      "depth": 5
    }
  },

  "overlays": [
    {
      "type": "heat",
      "metric": "test_coverage",
      "palette": "green-red",
      "scale": "linear"
    },
    {
      "type": "badges",
      "data": {
        "TokenStore::validate": { "text": "23% coverage", "color": "#ef4444" },
        "AuthService::refresh": { "text": "0% coverage", "color": "#ef4444" }
      }
    },
    {
      "type": "flow",
      "paths": [
        { "from": "POST /auth/login", "to": "AuthService::authenticate" },
        { "from": "AuthService::authenticate", "to": "TokenStore::validate" },
        { "from": "TokenStore::validate", "to": "db.query" }
      ],
      "speed": 0.5
    }
  ],

  "presentation": {
    "zoom": "fit-focus",
    "zoom_padding": 1.5,
    "layout": "layered",
    "show_legend": true
  }
}
```

The explorer renders this without any custom code — it's all declarative.

## LLM Integration

### How the LLM Generates View Specs

The LLM receives:
1. The user's question
2. The knowledge graph (or a relevant subgraph — nodes, edges, metadata)
3. The view spec grammar (this document)
4. Available metrics (test_coverage, churn, complexity, etc.)

It responds with a view spec JSON block that the explorer parses and renders.

### MCP Tool

```json
{
  "name": "explorer_view",
  "description": "Generate a visual exploration view for the user",
  "input_schema": {
    "type": "object",
    "properties": {
      "view_spec": { "$ref": "#/definitions/ViewSpec" },
      "explanation": { "type": "string" }
    }
  }
}
```

The briefing assistant, interrogation agents, and conversational explorer all use this tool to show visual answers.

### Saved Views

View specs can be saved and named:
- **System views:** predefined views like "Architecture Overview", "Endpoint Map", "Dependency Graph"
- **User views:** saved from conversational exploration ("my auth flow view")
- **Generated views:** LLM-generated for specific questions, ephemeral unless saved

Saved views are stored as JSON in the spec manifest or a dedicated views registry.

## Relationship to Other Specs

**Extends:** `ui-layout.md` §4 (View Spec Grammar — this supersedes the original grammar with a richer model)

**Implements:** `system-explorer.md` §4 (Concept Views), §8 (Conversational Exploration)

**Consumed by:** `explorer-canvas.md` (the renderer that interprets view specs)

**Produced by:** LLM briefing/conversational endpoints, saved view registry, spec manifests
