---
title: "Implement gyre search CLI command"
spec_ref: "search.md §CLI"
depends_on: []
progress: not-started
coverage_sections:
  - "search.md §CLI"
commits: []
---

## Spec Excerpt

From `search.md` §CLI:

> ```bash
> gyre search "identity security"                          # Simple search
> gyre search "merge queue" --type spec                    # Search specs only
> gyre search "ABAC" --type spec --status approved         # Faceted search
> gyre search "auth.rs" --type commit --since 7d           # Recent commits touching auth
> gyre search "budget" --workspace platform-team           # Workspace-scoped
> gyre search --suggest "iden"                              # Autocomplete
> ```

## Implementation Plan

1. **Add `Search` variant to CLI Commands enum** (`crates/gyre-cli/src/main.rs`):
   ```rust
   /// Full-text search across all entities
   Search {
       /// Search query (supports quoted phrases and faceted syntax)
       query: Option<String>,
       /// Filter by entity type (spec, task, mr, commit, agent)
       #[arg(long, short = 't')]
       r#type: Option<String>,
       /// Filter by status
       #[arg(long)]
       status: Option<String>,
       /// Filter by workspace slug
       #[arg(long, short = 'w')]
       workspace: Option<String>,
       /// Show results since (e.g., 7d, 2026-03-01)
       #[arg(long)]
       since: Option<String>,
       /// Autocomplete mode — return suggestions for the given prefix
       #[arg(long)]
       suggest: Option<String>,
       /// Maximum results to return
       #[arg(long, default_value = "20")]
       limit: usize,
   },
   ```

2. **Implement search handler** in the CLI match block:
   - If `--suggest` is provided, call `GET /api/v1/search/suggest?q={prefix}` (once task-153 is implemented; for now, fall back to regular search with prefix)
   - Otherwise, call `GET /api/v1/search?q={query}&entity_type={type}&workspace_id={workspace}&limit={limit}`
   - Append `--since`, `--status` as facets in the query string if the query language parser (task-154) is available
   - Format results in a readable table: `[type] title (id) — snippet`

3. **Add API client method** to the CLI's API client struct:
   - `async fn search(&self, query: &str, entity_type: Option<&str>, workspace: Option<&str>, limit: usize) -> Vec<SearchResult>`

4. **Tests**:
   - CLI parse test: `gyre search "identity"` parses correctly
   - CLI parse test: `gyre search --type spec --workspace platform-team "ABAC"`
   - CLI parse test: `gyre search --suggest "iden"`

## Acceptance Criteria

- [ ] `gyre search "query"` calls the search API and displays results
- [ ] `--type`, `--workspace`, `--status`, `--since` flags filter results
- [ ] `--suggest` flag returns autocomplete suggestions
- [ ] Results formatted as readable table with entity type, title, and snippet
- [ ] `--limit` controls result count (default 20)
- [ ] CLI parse tests pass

## Agent Instructions

- Read `crates/gyre-cli/src/main.rs` for the existing CLI command structure — follow the pattern of other commands like `Explore`, `Tasks`, `Status`
- The API client is defined in `crates/gyre-cli/src/client.rs` — add a search method there
- The search endpoint is `GET /api/v1/search` with query params `q`, `entity_type`, `workspace_id`, `limit`
- For the initial implementation, `--suggest` can fall back to regular search until task-153 adds the `/search/suggest` endpoint
- For `--since` and `--status`, include them as query params or as part of the query string facet syntax
