# Coverage: Search

**Spec:** [`system/search.md`](../../system/search.md)
**Last audited:** 2026-09-29 (full audit — verified against mem_search.rs, ports/search.rs, api/search.rs, mcp.rs handle_search, index-on-write callsites). MAJOR reclassification: search is backed ONLY by MemSearchAdapter (in-memory Vec substring matcher, mem_search.rs) wired in production (lib.rs:956) — no SQLite FTS5 / Postgres tsvector adapter exists anywhere. reindex_all is a no-op (counts in-memory docs, mem_search.rs:132; does NOT rebuild from domain entities). Only 3 of 10 spec entity types indexed (task/mr/agent). No access scoping: both REST search_handler (search.rs:46) and MCP handle_search (mcp.rs:1467) take workspace_id from client-supplied params, no auth/membership derivation — spec §Access Scoping unmet + leak risk.
**Coverage:** 4/14 (4 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Problem | 2 | n/a | - | Problem statement — no implementable requirement. |
| 2 | Design Principles | 2 | n/a | - | Design constraints — no implementable requirement. |
| 3 | Searchable Entities | 2 | task-assigned | task-202 | Only 3 of 10 entity types indexed (task/mr/agent); reindex_all is a no-op. task-202 indexes all 10 types + real reindex coordinator. |
| 4 | Search Index | 2 | task-assigned | task-201 | Index is an ephemeral in-memory Vec (MemSearchAdapter, lib.rs:956); no persistent FTS backend. task-201 implements SQLite FTS5 + Postgres tsvector adapters wired via store!. |
| 5 | Technology | 3 | task-assigned | task-201 | No SQLite FTS5 / Postgres tsvector backend exists. task-201 implements both, zero external deps. |
| 6 | Index Updates | 3 | implemented | - | Partial (code-verified 2026-09-29) — sync SearchPort::index() on write is genuine for task (tasks.rs:207-215), mr (merge_requests.rs:377-385), agent (agents.rs:124-132). Missing: spec-push indexing (content from git blob), commit-message indexing, async code-search background job. reindex_all "full rebuild" is a no-op (mem_search.rs:132). |
| 7 | Index Schema | 3 | task-assigned | task-201 | No FTS5 virtual table. task-201 creates search_index (fts5, tokenize='porter unicode61', UNINDEXED tenant/workspace/repo, metadata JSON) + PG tsvector table. |
| 8 | Access Scoping | 2 | task-assigned | task-203 | No access scoping — workspace_id taken from client params in REST + MCP. task-203 derives scope from auth context and filters at index-query level. |
| 9 | Query Language | 2 | n/a | - | Section heading only — no implementable requirement. Subsections below cover specifics. |
| 10 | Simple Search | 3 | verified | - | Code-verified 2026-09-29 — AND-of-terms semantics genuine: mem_search.rs score_doc (:28-47) returns None unless every whitespace-split term matches title/body (search.rs handler + mem_search.rs:76-124); multi-term queries require all terms, results sorted by descending score, snippet extracted around match. Section requirement (default AND over terms) met. NB: backed by in-memory substring matcher (see §Technology row 5), not a persistent FTS index. |
| 11 | Quoted Phrases | 3 | task-assigned | task-154 | No query language parser. Quoted phrases passed as raw text to FTS — may work with FTS5 MATCH syntax but no explicit phrase extraction in the search handler. |
| 12 | Faceted Filtering | 3 | task-assigned | task-154 | entity_type query param exists as basic filter, but the spec's query syntax (type:spec status:approved workspace:platform-team) is not parsed. No query language parser to extract facets from query string. |
| 13 | API | 2 | task-assigned | task-153 | Partial — 2 of 4 endpoints exist: GET /api/v1/search (search handler), POST /api/v1/admin/search/reindex. Missing: GET /search/suggest (autocomplete), GET /search/facets (facet metadata). Response missing facet_counts field. |
| 14 | Response Format | 3 | task-assigned | task-153 | Partial — SearchResponse has query, total, results with entity_type/id/title/snippet/score/facets. Missing: facet_counts aggregation, url field on results. |
| 15 | MCP Integration | 2 | implemented | - | Partial (code-verified 2026-09-29) — `search` MCP tool registered (mcp.rs:329) and functional: handle_search (mcp.rs:1467) calls SearchPort::search, returns formatted results. But prior claim "scoped by agent's authenticated context" is FALSE — workspace_id comes from client args (mcp.rs:1473), not the agent's token scope; no access enforcement. Missing spec `search.suggest` tool entirely. |
| 16 | CLI | 2 | task-assigned | task-155 | No `gyre search` CLI command. Only `gyre explore` exists (graph concept search, different purpose). Spec requires full-text search CLI with --type, --status, --workspace, --since, --suggest flags. |
| 17 | UI | 2 | implemented | - | Partial — SearchBar.svelte with Cmd+K shortcut. Search modal with navigation items. Missing: dedicated search results page with faceted sidebar, result grouping by entity type, highlighted snippets, recent searches. |
| 18 | Relationship to Existing Specs | 2 | n/a | - | Cross-reference section — no implementable requirement. |
