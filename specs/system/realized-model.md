# Realized System Model

> **Status: Draft.** This spec defines the backend infrastructure that extracts, stores, and serves a live architectural model of the codebase — the foundation for the System Explorer UI and progressive preview.

## Problem

Specs describe intent. Code implements it. But there is no machine-readable representation of **what the system actually is** — its modules, types, relationships, responsibilities, and how they map to specs. Without this, humans lose understanding of systems they didn't build. Code is too low-level (50,000 lines). Specs are too high-level ("implement search"). The gap between them is where understanding lives.

Today, the only way to understand the realized system is to read the code. In a fully autonomous SDLC where agents produce all code, this means the human has no natural entry point for comprehension. They write specs into a void and hope the output is correct.

## Core Insight

Code is structured. It has modules, types, traits, functions, dependencies. These structures have relationships: implements, depends_on, calls, extends. These relationships form a graph. That graph, combined with spec linkage and provenance, IS the system's architecture — not as a diagram someone drew, but as the code declares it.

The forge already knows the code (git), the specs (spec registry), the agents (provenance), and the personas (meta-spec sets). The missing piece is extracting the code's structural graph and linking it to everything else.

## Design

### 1. The Knowledge Graph

The realized model is a knowledge graph stored in the forge database. Nodes are structural elements extracted from code. Edges are relationships between them, and links to specs, agents, and meta-specs.

#### Node Types

Node types are language-agnostic. The extraction pipeline maps language-specific constructs to these universal types.

| Node Type | What It Represents | Rust | Go | Python | TypeScript/JS |
|---|---|---|---|---|---|
| `Package` | Top-level distributable unit | Crate (Cargo.toml) | Module (go.mod) | Package (pyproject.toml) | Package (package.json) |
| `Module` | Namespace / organizational unit | `mod` / file | Package (directory) | Module (file) | Module (file) |
| `Type` | Data structure | `struct`, `enum` | `struct` | `class`, `dataclass` | `interface`, `type`, `class` |
| `Interface` | Behavioral contract | `trait` | `interface` | `Protocol`, `ABC` | `interface` |
| `Function` | Callable unit | `fn`, `impl` method | `func` | `def` | `function`, method |
| `Endpoint` | API route | axum/actix route | net/http handler | FastAPI/Flask route | Express/Hono route |
| `Component` | UI element | — | — | — | `.svelte`, `.tsx`, `.vue` |
| `Table` | Persistence schema | Diesel schema | GORM model | SQLAlchemy model | Prisma/Drizzle schema |
| `Constant` | Named value | `const`, `static` | `const`, `var` | module-level | `const`, `enum` |

The extraction pipeline is pluggable per language. Each language extractor maps its constructs to these universal node types. Adding a new language means writing one extractor — the knowledge graph, API, and explorer UI work unchanged.

#### Edge Types

| Edge Type | Meaning | Example |
|---|---|---|
| `contains` | Parent-child structural relationship | Crate contains Module, Module contains Type |
| `implements` | Trait implementation | `SqliteTaskRepo` implements `TaskPort` |
| `depends_on` | Import / use dependency | `gyre-domain` depends_on `gyre-ports` |
| `calls` | Function invocation (direct) | `spawn_agent` calls `create_worktree` |
| `field_of` | Struct field type reference | `Task.repository_id` references `Repository` |
| `returns` | Function return type | `list_tasks` returns `Vec<Task>` |
| `routes_to` | HTTP endpoint to handler | `POST /agents/spawn` routes_to `handle_spawn` |
| `renders` | UI component data source | `TaskBoard` renders `Task[]` |
| `persists_to` | Type to database table | `Task` persists_to `tasks` |
| `governed_by` | Code element to spec | `SearchService` governed_by `search.md` |
| `produced_by` | Code element to agent provenance | `SearchService` produced_by `worker-7@persona-v3` |

#### Properties on Nodes

Every node carries metadata:

```rust
pub struct GraphNode {
    pub id: Id,
    pub repo_id: Id,
    pub node_type: NodeType,
    pub name: String,
    pub qualified_name: String,     // e.g., gyre_domain::task::Task
    pub file_path: String,          // source file
    pub line_start: u32,
    pub line_end: u32,
    pub visibility: Visibility,     // pub, pub(crate), private
    pub doc_comment: Option<String>,
    pub spec_path: Option<String>,  // governing spec, if linked
    pub spec_confidence: SpecConfidence, // None, Low, Medium, High
    pub last_modified_sha: String,
    pub last_modified_by: Option<Id>,  // agent ID
    pub last_modified_at: u64,
    pub created_sha: String,
    pub created_at: u64,
    pub complexity: Option<u32>,    // cyclomatic complexity
    pub test_coverage: Option<f32>, // 0.0-1.0, if available
    pub churn_count_30d: u32,       // modifications in last 30 days
}
```

### 2. Extraction Pipeline

The knowledge graph is updated on every push, in the post-receive hook — same-tick as commit provenance and spec lifecycle. No external CI, no polling.

#### Architecture

The extraction pipeline is pluggable. A **language extractor** is a function that receives the repository tree and emits universal graph nodes and edges. The pipeline auto-detects languages from manifest files (`Cargo.toml`, `go.mod`, `package.json`, `pyproject.toml`) and runs the appropriate extractors.

```rust
pub trait LanguageExtractor: Send + Sync {
    /// Detect whether this extractor applies to the given repo
    fn detect(&self, repo_root: &Path) -> bool;

    /// Extract nodes and edges from the repository
    fn extract(&self, repo_root: &Path, previous: &Graph) -> ExtractionResult;
}

pub struct ExtractionResult {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub errors: Vec<ExtractionError>,  // non-fatal parse failures
}
```

Multiple extractors can run on the same repo (e.g., a Rust backend + Svelte frontend + SQL schema). Results are merged into one graph.

#### Built-In Extractors

**Rust**: Uses `syn` for fast AST parsing. Extracts packages (Cargo.toml), modules (mod tree), types (struct/enum), interfaces (trait), implementations (impl blocks), functions (pub fn), dependencies (use/extern crate), and route registrations (pattern-match axum/actix router macros).

**TypeScript/JavaScript**: Uses a JS AST parser (tree-sitter or swc). Extracts packages (package.json), modules (files), types (interface/type/class), functions (export), and endpoint registrations (Express/Hono route patterns). Svelte components extracted from `.svelte` files.

**Go**: Uses `go/ast` or tree-sitter. Extracts modules (go.mod), packages (directories), types (struct), interfaces (interface), functions (func), and HTTP handler registrations.

**Python**: Uses tree-sitter or `ast` module output. Extracts packages (pyproject.toml), modules (files), types (class/dataclass), functions (def), and endpoint registrations (FastAPI/Flask decorators).

**SQL/ORM schema**: Parses Diesel schema.rs, Prisma schema, SQLAlchemy models, GORM models, or raw SQL migrations. Extracts tables, columns, foreign keys, indexes.

#### Adding a New Language

Implement the `LanguageExtractor` trait, register it in the pipeline. The knowledge graph, API, and explorer UI work unchanged — they operate on universal node types, not language-specific constructs.

#### Spec Linkage

After structural extraction, the pipeline links code elements to specs:

1. **Explicit links**: code comments like `// spec: search.md` or `#[spec("search.md")]`
2. **Provenance-based**: the commit that introduced a type was created by a task referencing spec X → the type is governed by spec X
3. **Name-based heuristic**: `SearchService` in a commit referencing `search.md` → likely governed by that spec (lower confidence, flagged for human confirmation)

Confidence levels:

| Method | Confidence | Requires Human Confirmation |
|---|---|---|
| Explicit annotation | High | No |
| Provenance chain | Medium | No |
| Name heuristic | Low | Yes — shown as "suggested" link in explorer |

### 3. Architectural Timeline

The knowledge graph is versioned. On each push, the pipeline computes a diff against the previous graph state:

```rust
pub struct ArchitecturalDelta {
    pub repo_id: Id,
    pub commit_sha: String,
    pub timestamp: u64,
    pub spec_ref: Option<String>,
    pub agent_id: Option<Id>,
    pub nodes_added: Vec<GraphNode>,
    pub nodes_removed: Vec<GraphNode>,
    pub nodes_modified: Vec<(GraphNode, Vec<FieldChange>)>,
    pub edges_added: Vec<GraphEdge>,
    pub edges_removed: Vec<GraphEdge>,
}
```

The timeline is queryable: "show me the graph at commit X" or "show me all changes between March 1 and March 15." The System Explorer uses this for the time-scrubber and the delta/briefing views.

### 4. Concept Views

A concept view is a named, saved projection of the knowledge graph that cuts across modules. Concepts are defined in the spec manifest:

```yaml
concepts:
  - name: Authentication
    description: "Token validation, RBAC, ABAC, JWT handling"
    include:
      - types: ["*Auth*", "*Token*", "*Rbac*", "*Abac*", "*Jwt*"]
      - traits: ["*Auth*"]
      - modules: ["*::auth*", "*::identity*"]
      - endpoints: ["/api/v1/auth/*", "/.well-known/*"]
      - specs: ["identity-security.md", "abac-policy-engine.md"]

  - name: Merge Pipeline
    description: "Merge queue, gates, MR lifecycle"
    include:
      - types: ["MergeRequest", "MergeQueueEntry", "Gate*", "QueueProcessor"]
      - modules: ["*::merge*", "*::gates*"]
      - specs: ["source-control.md", "agent-gates.md", "merge-dependencies.md"]
```

Concepts can also be auto-suggested by clustering the knowledge graph (types that frequently co-change or co-depend likely belong to the same concept).

### 5. Risk Metrics

Computed from the knowledge graph and git history, updated on each push:

| Metric | Computation | Meaning |
|---|---|---|
| `churn_rate` | Modifications per 30-day window | High churn = unstable or rapidly evolving |
| `coupling_score` | Co-change frequency between modules | High coupling = change one, must change the other |
| `spec_coverage` | Nodes with `governed_by` edges / total nodes | Low = undocumented code |
| `complexity` | Cyclomatic complexity per function/module | High = hard to understand, error-prone |
| `fan_in` / `fan_out` | Incoming / outgoing dependency count | High fan-out = fragile, many reasons to change |
| `agent_contention` | Concurrent agents modifying same files (from hot-files) | High = conflict risk |
| `staleness` | Days since last modification | Very high = dead code or stable foundation |

These feed the Risk Map view in the System Explorer.

### 6. Narrative Generation

The briefing/delta view requires human-readable summaries, not raw graph diffs. The forge generates narratives from architectural deltas:

**Template-based** (fast, deterministic):
```
"New type `VectorIndex` added to module `gyre_domain::search`.
 Implements trait `FullTextPort`. 3 fields: embedding_model, dimension, index_path.
 Governed by spec: search.md. Produced by agent worker-12 under persona backend-dev v4."
```

**LLM-synthesized** (richer, for the briefing view):
```
"The search subsystem gained vector similarity support. A new VectorIndex type
 was added alongside the existing FtsIndex, both implementing FullTextPort.
 SearchQuery now supports a mode field (FullText | Semantic). This implements
 the semantic search requirement added to search.md on March 15."
```

Both are grounded in the knowledge graph — the LLM is summarizing structured data, not hallucinating.

### 7. API Surface

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/repos/{id}/graph` | GET | Full knowledge graph for a repo (nodes + edges) |
| `GET /api/v1/repos/{id}/graph/types` | GET | All types with relationships |
| `GET /api/v1/repos/{id}/graph/modules` | GET | Module tree with containment |
| `GET /api/v1/repos/{id}/graph/node/{node_id}` | GET | Single node with all edges |
| `GET /api/v1/repos/{id}/graph/spec/{path}` | GET | All nodes governed by a spec |
| `GET /api/v1/repos/{id}/graph/concept/{name}` | GET | Concept view projection |
| `GET /api/v1/repos/{id}/graph/timeline` | GET | Architectural deltas (`?since=&until=`) |
| `GET /api/v1/repos/{id}/graph/risks` | GET | Risk metrics per module |
| `GET /api/v1/repos/{id}/graph/diff` | GET | Graph diff between two commits (`?from=&to=`) |
| `GET /api/v1/workspaces/{id}/graph` | GET | Cross-repo knowledge graph for a workspace |
| `GET /api/v1/workspaces/{id}/graph/concept/{name}` | GET | Workspace-scoped concept search (avoids downloading full workspace graph for concept queries) |
| `GET /api/v1/workspaces/{id}/briefing` | GET | Briefing endpoint (`?since=`). **Response schema owned by `human-system-interface.md` §9** (this spec does not define the response shape). Knowledge graph narratives (§6 above) feed the briefing's `summary` string fields. |
| `POST /api/v1/repos/{id}/graph/link` | POST | Manually link a node to a spec (human confirmation of suggested links) |
| `POST /api/v1/repos/{id}/graph/predict` | POST | Structural prediction for a spec diff (request body: `{spec_path, draft_content}`) |

### 8. Storage

The knowledge graph is stored in the forge database alongside other domain entities:

```sql
CREATE TABLE graph_nodes (
    id          TEXT PRIMARY KEY,
    repo_id     TEXT NOT NULL,
    node_type   TEXT NOT NULL,
    name        TEXT NOT NULL,
    qualified_name TEXT NOT NULL,
    file_path   TEXT NOT NULL,
    line_start  INTEGER NOT NULL,
    line_end    INTEGER NOT NULL,
    visibility  TEXT NOT NULL DEFAULT 'public',
    doc_comment TEXT,
    spec_path   TEXT,
    spec_confidence TEXT DEFAULT 'none',
    last_modified_sha TEXT NOT NULL,
    last_modified_by TEXT,
    last_modified_at INTEGER NOT NULL,
    created_sha TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    complexity  INTEGER,
    churn_count_30d INTEGER DEFAULT 0,
    test_coverage REAL              -- 0.0-1.0, NULL if unavailable
);

CREATE TABLE graph_edges (
    id          TEXT PRIMARY KEY,
    repo_id     TEXT NOT NULL,
    source_id   TEXT NOT NULL REFERENCES graph_nodes(id),
    target_id   TEXT NOT NULL REFERENCES graph_nodes(id),
    edge_type   TEXT NOT NULL,
    metadata    TEXT  -- JSON, edge-type-specific data
);

CREATE TABLE graph_deltas (
    id          TEXT PRIMARY KEY,
    repo_id     TEXT NOT NULL,
    commit_sha  TEXT NOT NULL,
    timestamp   INTEGER NOT NULL,
    agent_id    TEXT,
    spec_ref    TEXT,
    delta_json  TEXT NOT NULL  -- serialized ArchitecturalDelta
);
```

## Relationship to Existing Specs

**Extends:**
- **forge-advantages.md** — Capability 5 (Rich Commit Provenance) and Capability 8 (Cross-Agent Code Awareness) are data sources for the knowledge graph
- **spec-registry.md** — spec linkage (`governed_by` edges) connects the knowledge graph to the spec ledger
- **spec-lifecycle.md** — architectural deltas feed the spec lifecycle (structural changes can trigger spec drift detection)
- **meta-spec-reconciliation.md** — provenance in the knowledge graph tracks meta-spec versions, enabling blast radius computation

**Depends on:**
- **source-control.md** — git push hooks trigger extraction
- **spec-registry.md** — spec paths and SHAs for linkage
- **platform-model.md** — workspace/repo scoping for cross-repo graphs

**Enables:**
- **system-explorer.md** — the knowledge graph is the data layer for the explorer UI
- **human-system-interface.md** — the briefing narrative is generated from architectural deltas (supersedes `ui-journeys.md`)
