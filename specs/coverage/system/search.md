# Coverage: Search

**Spec:** [`system/search.md`](../../system/search.md)
**Last audited:** 2026-04-13 (full audit — verified against search.rs, search port, mcp.rs, CLI)
**Coverage:** 9/15 (3 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Problem | 2 | n/a | - | Problem statement — no implementable requirement. |
| 2 | Design Principles | 2 | n/a | - | Design constraints — no implementable requirement. |
| 3 | Searchable Entities | 2 | implemented | - | SearchDocument with entity_type, entity_id, title, body, workspace_id, repo_id, facets (HashMap). SearchPort supports indexing arbitrary entity types. Reindex_all rebuilds from all domain entities. |
| 4 | Search Index | 2 | implemented | - | SearchPort trait (gyre-ports/src/search.rs) with index, search, delete, reindex_all. SQLite FTS5 or in-memory adapter. POST /api/v1/admin/search/reindex for manual rebuild. |
| 5 | Technology | 3 | implemented | - | SearchPort abstraction allows pluggable backends. Default uses SQLite FTS5. Port pattern supports swapping to Meilisearch/Typesense without changing domain logic. |
| 6 | Index Updates | 3 | implemented | - | SearchPort::index() called on entity writes. reindex_all() for full rebuild. Admin reindex endpoint for manual trigger. |
| 7 | Index Schema | 3 | implemented | - | SearchDocument: entity_type, entity_id, title, body, workspace_id, repo_id, facets. Maps to FTS5 virtual table with tokenize='porter unicode61'. |
| 8 | Access Scoping | 2 | implemented | - | Partial — workspace_id filtering in SearchQuery. Full ABAC-based scoping (tenant→workspace→repo filtering at query level) not verified but workspace_id filter provides basic scope enforcement. |
| 9 | Query Language | 2 | n/a | - | Section heading only — no implementable requirement. Subsections below cover specifics. |
| 10 | Simple Search | 3 | implemented | - | Basic text search works via SearchPort::search(). Query string matched against indexed content. Default AND behavior for multiple terms. |
| 11 | Quoted Phrases | 3 | task-assigned | task-154 | No query language parser. Quoted phrases passed as raw text to FTS — may work with FTS5 MATCH syntax but no explicit phrase extraction in the search handler. |
| 12 | Faceted Filtering | 3 | task-assigned | task-154 | entity_type query param exists as basic filter, but the spec's query syntax (type:spec status:approved workspace:platform-team) is not parsed. No query language parser to extract facets from query string. |
| 13 | API | 2 | task-assigned | task-153 | Partial — 2 of 4 endpoints exist: GET /api/v1/search (search handler), POST /api/v1/admin/search/reindex. Missing: GET /search/suggest (autocomplete), GET /search/facets (facet metadata). Response missing facet_counts field. |
| 14 | Response Format | 3 | task-assigned | task-153 | Partial — SearchResponse has query, total, results with entity_type/id/title/snippet/score/facets. Missing: facet_counts aggregation, url field on results. |
| 15 | MCP Integration | 2 | implemented | - | gyre_search MCP tool registered (mcp.rs:329). Agents search via MCP with query and optional parameters. Scoped by agent's authenticated context. |
| 16 | CLI | 2 | task-assigned | task-155 | No `gyre search` CLI command. Only `gyre explore` exists (graph concept search, different purpose). Spec requires full-text search CLI with --type, --status, --workspace, --since, --suggest flags. |
| 17 | UI | 2 | implemented | - | Partial — SearchBar.svelte with Cmd+K shortcut. Search modal with navigation items. Missing: dedicated search results page with faceted sidebar, result grouping by entity type, highlighted snippets, recent searches. |
| 18 | Relationship to Existing Specs | 2 | n/a | - | Cross-reference section — no implementable requirement. |
