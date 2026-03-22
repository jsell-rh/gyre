# Search

## Problem

At enterprise scale (hundreds of repos, thousands of specs, tens of thousands of tasks/MRs), finding things is the difference between a usable platform and one that's technically correct but practically unusable. Today Gyre has no search. Users and agents must know exact IDs or browse through lists.

Without search:
- "Which spec covers JWT authentication?" requires manually reading specs/index.md
- "Find all tasks related to the ABAC migration" requires scrolling through task lists
- "What commits touched auth.rs last week?" requires git log on the right repo
- Agents waste MCP tool calls browsing when they could search directly
- Cross-workspace discovery is impossible without knowing where to look

## Design Principles

1. **Search is access-scoped.** Results never include entities the user/agent can't access. A search for "identity" by a user without access to workspace X won't return specs from workspace X.
2. **Search is a first-class MCP resource.** Agents search via MCP, not by paginating through list endpoints.
3. **Search covers everything.** Not just code - specs, tasks, MRs, commits, agents, personas, activity events, audit events.
4. **Search is fast.** Full-text search must return results in <200ms for interactive use. This means a dedicated search index, not SQL LIKE queries.

## Searchable Entities

| Entity | Indexed Fields | Facets |
|---|---|---|
| Spec | title, path, full content, owner, links | workspace, repo, approval_status, scope |
| Task | title, description, labels | workspace, repo, status, priority, assigned_to |
| Merge Request | title, source_branch, target_branch, author | workspace, repo, status, has_conflicts |
| Commit | message, SHA, author, branch | workspace, repo, agent_id, date_range |
| Agent | name, persona, status | workspace, repo, status |
| Persona | name, prompt content | scope (global/workspace/repo) |
| Activity Event | description, event_type, agent_id | workspace, repo, event_type, date_range |
| Audit Event | event_type, details, path | workspace, repo, agent_id, event_type, date_range |
| Review | body, decision, reviewer | workspace, repo, MR |
| File | path, content (code search) | workspace, repo, language |

## Search Index

### Technology

SQLite FTS5 for single-node deployments. For Postgres deployments, use Postgres full-text search (`tsvector`/`tsquery`). Both are built-in - no external search infrastructure required (no Elasticsearch dependency).

For scale beyond what built-in FTS handles (>1M documents), the architecture should support plugging in an external engine (Meilisearch, Typesense) via the hexagonal port pattern. But the default must work with zero external dependencies.

### Index Updates

The search index is updated synchronously on every write:
- Task created/updated -> index updated
- Spec pushed -> index updated (content extracted from git blob)
- MR created/reviewed -> index updated
- Commit pushed -> commit messages indexed
- Agent registered/status changed -> index updated

For code search (file content), indexing happens asynchronously on push via a background job. Code search is eventually consistent (seconds, not minutes).

### Index Schema

```sql
-- SQLite FTS5 example
CREATE VIRTUAL TABLE search_index USING fts5(
    entity_type,       -- 'spec', 'task', 'mr', 'commit', 'agent', etc.
    entity_id,
    tenant_id UNINDEXED,
    workspace_id UNINDEXED,
    repo_id UNINDEXED,
    title,
    body,              -- Full content (spec text, task description, commit message)
    metadata,          -- JSON: author, status, labels, etc. for faceted filtering
    tokenize='porter unicode61'
);
```

## Access Scoping

Every search query is filtered by the caller's access:

1. Extract `tenant_id` from auth context
2. Determine which `workspace_ids` the user/agent has membership in
3. Determine which `repo_ids` within those workspaces the user/agent can access
4. Filter search results to only include entities within accessible scopes

This filtering happens at the index query level (not post-filter) to avoid leaking result counts.

For agents, scope is determined by their OIDC token claims:
- Repo-scoped agent: search results limited to its repo
- Workspace Orchestrator: search results include all repos in its workspace
- No agent can search outside its workspace

## Query Language

### Simple Search

```
identity security JWT
```

Matches any entity containing all three terms. Default behavior.

### Quoted Phrases

```
"merge queue" ordering
```

Matches entities containing the exact phrase "merge queue" and the term "ordering".

### Faceted Filtering

```
identity type:spec status:approved workspace:platform-team
```

Filters by entity type and attributes. Available facets:

| Facet | Applies To | Example |
|---|---|---|
| `type:` | All | `type:spec`, `type:task`, `type:mr`, `type:commit` |
| `workspace:` | All | `workspace:platform-team` |
| `repo:` | All | `repo:gyre-server` |
| `status:` | Task, MR, Agent | `status:in_progress`, `status:open` |
| `priority:` | Task | `priority:high` |
| `assigned:` | Task | `assigned:@jsell`, `assigned:agent:worker-42` |
| `author:` | MR, Commit, Review | `author:@jsell` |
| `label:` | Task | `label:spec-drift-review` |
| `approval:` | Spec | `approval:pending`, `approval:approved` |
| `scope:` | Persona | `scope:global`, `scope:workspace` |
| `since:` | All | `since:7d`, `since:2026-03-01` |
| `until:` | All | `until:2026-03-20` |

Facets compose with AND. Multiple values for the same facet compose with OR:
```
type:spec type:task status:pending workspace:platform-team
```
Finds specs OR tasks that are pending in the platform-team workspace.

## API

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/search?q={query}` | GET | Full-text search with facets |
| `GET /api/v1/search/suggest?q={prefix}` | GET | Autocomplete suggestions |
| `GET /api/v1/search/facets` | GET | Available facets and their values |
| `POST /api/v1/search/reindex` | POST | Force full reindex (admin only) |

### Response Format

```json
{
  "query": "identity security",
  "total": 14,
  "results": [
    {
      "entity_type": "spec",
      "entity_id": "system/identity-security.md",
      "workspace": "platform-team",
      "repo": "gyre",
      "title": "Identity & Security",
      "snippet": "...SPIFFE for cryptographic agent **identity**. **Security** by default...",
      "score": 0.95,
      "url": "/workspaces/platform-team/repos/gyre/specs/system/identity-security.md"
    }
  ],
  "facet_counts": {
    "type": { "spec": 5, "task": 7, "mr": 2 },
    "workspace": { "platform-team": 10, "app-team": 4 },
    "status": { "approved": 3, "pending": 2 }
  }
}
```

## MCP Integration

Agents search via MCP:

| Tool | Purpose |
|---|---|
| `search.query` | Full-text search (returns results scoped to agent's access) |
| `search.suggest` | Autocomplete for building queries |

```json
// Agent MCP tool call
{
  "tool": "search.query",
  "params": {
    "q": "ABAC policy type:spec approval:approved",
    "limit": 5
  }
}
```

This is critical for cache-hit optimization. Instead of an agent guessing file paths, it searches. A search hit is faster and more reliable than iterating through `spec.list` results.

## CLI

```bash
gyre search "identity security"                          # Simple search
gyre search "merge queue" --type spec                    # Search specs only
gyre search "ABAC" --type spec --status approved         # Faceted search
gyre search "auth.rs" --type commit --since 7d           # Recent commits touching auth
gyre search "budget" --workspace platform-team           # Workspace-scoped
gyre search --suggest "iden"                              # Autocomplete
```

## UI

- **Global search bar** (Cmd+K / Ctrl+K) in top nav, always accessible
- **Search results page** with faceted sidebar (filter by type, workspace, repo, status)
- **Highlighted snippets** showing query terms in context
- **Result grouping** by entity type with counts
- **Recent searches** for quick re-access
- **Search within views** - task board, MR list, spec registry all have local search that uses the same engine

## Relationship to Existing Specs

- **Spec Registry** (`spec-registry.md`): specs are searchable by content, title, approval status, links
- **Agent Experience** (`development/agent-experience.md`): search is a cache-hit optimization mechanism. Agents find specs by searching instead of guessing paths.
- **Platform Model** (`platform-model.md`): search respects tenant/workspace/repo scoping. MCP `search.query` tool is scoped by agent OIDC token.
- **User Management** (`user-management.md`): "My Stuff" views can use search to filter personal entities
