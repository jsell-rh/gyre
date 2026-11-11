---
title: "Implement search query language (quoted phrases, faceted filtering)"
spec_ref: "search.md §Query Language"
depends_on: [task-153]
progress: not-started
coverage_sections:
  - "search.md §Query Language"
  - "search.md §Simple Search"
  - "search.md §Quoted Phrases"
  - "search.md §Faceted Filtering"
commits: []
---

## Spec Excerpt

From `search.md` §Query Language:

> ### Simple Search
> `identity security JWT` — matches any entity containing all three terms.
>
> ### Quoted Phrases
> `"merge queue" ordering` — matches entities containing the exact phrase "merge queue" and the term "ordering".
>
> ### Faceted Filtering
> `identity type:spec status:approved workspace:platform-team` — filters by entity type and attributes.
>
> | Facet | Applies To | Example |
> |---|---|---|
> | `type:` | All | `type:spec`, `type:task` |
> | `workspace:` | All | `workspace:platform-team` |
> | `repo:` | All | `repo:gyre-server` |
> | `status:` | Task, MR, Agent | `status:in_progress` |
> | `priority:` | Task | `priority:high` |
> | `label:` | Task | `label:spec-drift-review` |
> | `since:` | All | `since:7d`, `since:2026-03-01` |
> | `until:` | All | `until:2026-03-20` |
>
> Facets compose with AND. Multiple values for the same facet compose with OR.

## Implementation Plan

1. **Create query parser** (`crates/gyre-domain/src/search_query.rs` — new file):
   - Parse a search query string into a structured `ParsedQuery`:
     ```rust
     pub struct ParsedQuery {
         pub terms: Vec<String>,          // individual words
         pub phrases: Vec<String>,        // "quoted phrases"
         pub facets: HashMap<String, Vec<String>>,  // type: -> [spec, task], status: -> [approved]
     }
     ```
   - Handle quoted phrase extraction: `"merge queue"` → phrase "merge queue"
   - Handle facet extraction: `type:spec` → facet("type", "spec")
   - Handle relative time parsing for `since:` and `until:` facets: `7d` → 7 days ago, `2026-03-01` → absolute date
   - Remaining unquoted tokens become search terms

2. **Integrate parser into SearchPort**:
   - Modify `SearchQuery` in `crates/gyre-ports/src/search.rs` to accept either a raw query string OR a `ParsedQuery`
   - The search handler in `crates/gyre-server/src/api/search.rs` parses the query string first, then passes the structured query to the port
   - The `entity_type` and `workspace_id` query parameters become fallbacks — facets in the query string take precedence

3. **Update SearchPort adapter** to handle structured queries:
   - For SQLite FTS5: use `MATCH` with quoted phrases and regular terms
   - Apply facet filters as SQL WHERE clauses on the search index metadata
   - For `since:`/`until:`: filter on the indexed document's timestamp

4. **Tests**:
   - Parser: simple terms `identity security` → terms: ["identity", "security"]
   - Parser: quoted phrase `"merge queue" ordering` → phrases: ["merge queue"], terms: ["ordering"]
   - Parser: facets `type:spec status:approved` → facets: {type: [spec], status: [approved]}
   - Parser: mixed `identity "merge queue" type:spec since:7d`
   - Parser: multiple values `type:spec type:task` → facets: {type: [spec, task]}
   - Integration: search with quoted phrase returns only exact matches
   - Integration: search with facet filter narrows results

## Acceptance Criteria

- [ ] Query parser correctly extracts terms, quoted phrases, and facets from query string
- [ ] Quoted phrases are searched as exact phrases (not individual terms)
- [ ] Facet filters narrow search results (AND across facets, OR within same facet)
- [ ] `since:` and `until:` support relative (`7d`) and absolute (`2026-03-01`) formats
- [ ] Search handler integrates parser — users can use the query language in `GET /api/v1/search?q=`
- [ ] MCP `gyre_search` tool also supports the query language
- [ ] Tests cover all parser patterns and integration

## Agent Instructions

- Read `crates/gyre-server/src/api/search.rs` for the current search handler
- Read `crates/gyre-ports/src/search.rs` for SearchQuery and SearchPort trait
- The query parser goes in `gyre-domain` (pure logic, no infrastructure deps) per hexagonal architecture
- The parser should be forgiving: malformed facets (e.g., `type:` with no value) should be ignored, not cause errors
- Look at how `view_query.rs` in `gyre-domain` parses its query grammar for a reference pattern
- The existing `entity_type` and `workspace_id` query params on the search endpoint should remain as backward-compatible shortcuts — they're equivalent to `type:{value}` and `workspace:{value}` in the query string
