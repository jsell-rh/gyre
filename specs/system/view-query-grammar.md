# View Query Grammar

> **Status: Draft.** Extends `ui-layout.md` §4. Implements `system-explorer.md` §4 (Concept Views), §8 (Conversational Exploration). Depends on: `explorer-canvas.md` (renderer), `lsp-call-graph.md` (complete graph data).
>
> **Naming:** A view query is NOT a spec. Specs define intent — what to build. View queries define what to SHOW — they are ephemeral graph queries that produce a visualization. The LLM generates view queries; the renderer executes them deterministically.

## Problem

The explorer canvas can render graphs, overlays, particles, diffs, and semantic zoom. But each visualization mode is hardcoded. There's no way for the LLM to say "show the user THIS subgraph, colored THIS way" and have the explorer render it.

The gap: a **declarative grammar** that both the LLM and the renderer understand. The LLM generates view queries; the renderer executes them. No custom code per question.

## Primitives

### 1. Computed References

Computed references are **graph traversals resolved by the renderer**, not by the LLM. The LLM references them; the renderer computes them from the knowledge graph. All computations are deterministic — always correct given complete graph data.

```
$clicked                                    — the node the user just clicked (interactive binding)
$selected                                   — the currently selected node
$callers(node, depth?)                      — nodes with incoming Calls edges, transitively
$callees(node, depth?)                      — nodes with outgoing Calls edges, transitively
$implementors(node)                         — types with Implements edges TO this node
$fields(node)                               — nodes with FieldOf edges TO this node
$descendants(node)                          — all children via Contains edges, recursively
$ancestors(node)                            — parent chain via Contains edges to root
$governed_by(spec_path)                     — nodes linked to a spec via GovernedBy
$reachable(node, edge_types[], dir, depth)  — general BFS traversal
$where(property, operator, value)           — metric-based node selection
$test_reachable                             — nodes reachable from any test function via Calls
$test_unreachable                           — complement: structural test coverage gaps
$test_fragility(node)                       — count of distinct test paths reaching this node
$intersect(set_a, set_b)                    — set intersection
$union(set_a, set_b)                        — set union
$diff(set_a, set_b)                         — set difference (in A but not B)
```

### 2. Scope — What Subgraph to Show

| Type | Description |
|---|---|
| `all` | Show everything |
| `focus` | BFS from a node along specified edges. Supports `$clicked` for interactive mode |
| `filter` | Show nodes matching `node_types` or computed set |
| `test_gaps` | Nodes NOT reachable from any test function |
| `diff` | Changes between two commits |
| `concept` | Cross-cutting concept from seed nodes expanded along edges |

### 3. Emphasis — How to Color It

| Primitive | Description |
|---|---|
| `highlight.matched` | Color + label for nodes in the result set |
| `dim_unmatched` | Opacity for non-matched nodes (0.0–1.0) |
| `tiered_colors` | Array of colors by BFS depth: depth 0 = [0], depth 1 = [1], etc. |
| `heat` | Color all nodes by metric (incoming_calls, complexity, test_fragility, etc.) |
| `badges` | Attach text labels. `{{count}}` filled by renderer |

### 4. Edges — What Relationships to Show

Edge filter by type. When a result set is active, edges are further restricted to connections BETWEEN result nodes only — no spaghetti from unrelated calls.

### 5. Zoom

`"fit"` (zoom to highlighted), `"current"` (don't change), or `{"level": N}`.

### 6. Annotation

Title + description with template variables: `$name` (focused node name), `{{count}}` (result set size), `{{group_count}}` (distinct parent modules).

### 7. Interactive Bindings

`"node": "$clicked"` makes the query a **mode** — each user click re-runs the traversal from the clicked node. The annotation updates with the new node's name. Essential for exploratory analysis: "click any node to see its blast radius."

## Interaction Context

When the LLM is invoked, it receives the current canvas state:

```json
{
  "canvas_state": {
    "selected_node": { "id": "uuid", "name": "TaskPort", "node_type": "interface" },
    "zoom_level": 2.3,
    "visible_tree_groups": ["gyre-server", "gyre-domain"],
    "active_filter": "calls",
    "active_lens": "structural",
    "active_query": { "type": "blast_radius", "node": "TaskPort" }
  },
  "recent_interactions": [
    { "action": "click", "node": "TaskPort::find_by_id" },
    { "action": "zoom_in", "to_level": 2.3 },
    { "action": "query", "preset": "blast_radius" }
  ]
}
```

This lets the LLM understand conversational context: "what about this one?" = `$selected`. "Show me test coverage for what I'm looking at" = scope to visible groups.

## Composition Examples

### Blast radius (interactive)
```json
{
  "scope": { "type": "focus", "node": "$clicked", "edges": ["calls", "implements"], "direction": "incoming", "depth": 10 },
  "emphasis": { "tiered_colors": ["#ef4444", "#f97316", "#eab308", "#94a3b8"], "dim_unmatched": 0.12 },
  "edges": { "filter": ["calls", "implements"] },
  "zoom": "fit",
  "annotation": { "title": "Blast radius: $name", "description": "{{count}} transitive callers/implementors" }
}
```

### Test coverage gaps
```json
{
  "scope": { "type": "test_gaps" },
  "emphasis": { "highlight": { "matched": { "color": "#ef4444", "label": "Untested" }}, "dim_unmatched": 0.3 },
  "annotation": { "title": "Test coverage gaps", "description": "{{count}} functions not reachable from any test" }
}
```

### High-risk untested code
```json
{
  "scope": { "type": "filter", "computed": "$intersect($where(complexity, '>', 20), $test_unreachable)" },
  "emphasis": { "heat": { "metric": "complexity", "palette": "blue-red" }, "dim_unmatched": 0.15 },
  "annotation": { "title": "High-risk untested code", "description": "{{count}} complex functions with no test coverage" }
}
```

### Authentication concept
```json
{
  "scope": { "type": "concept", "seed_nodes": ["AuthService", "TokenStore"], "expand_edges": ["calls"], "expand_depth": 2 },
  "emphasis": { "highlight": { "matched": { "color": "#60a5fa" }}, "dim_unmatched": 0.15 },
  "zoom": "fit",
  "annotation": { "title": "Authentication system", "description": "{{count}} nodes" }
}
```

## Relationship to Other Specs

**Extends:** `ui-layout.md` §4 (supersedes the original view spec grammar)
**Implements:** `system-explorer.md` §4 (Concept Views), §8 (Conversational Exploration)
**Consumed by:** `explorer-canvas.md` (renderer)
**Produced by:** LLM endpoints, preset buttons, user-composed JSON
