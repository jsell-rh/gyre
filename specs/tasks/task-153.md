---
title: "Implement search autocomplete and facets endpoints"
spec_ref: "search.md §API"
depends_on: []
progress: not-started
coverage_sections:
  - "search.md §API"
  - "search.md §Response Format"
commits: []
---

## Spec Excerpt

From `search.md` §API:

> | Endpoint | Method | Purpose |
> |---|---|---|
> | `GET /api/v1/search?q={query}` | GET | Full-text search with facets |
> | `GET /api/v1/search/suggest?q={prefix}` | GET | Autocomplete suggestions |
> | `GET /api/v1/search/facets` | GET | Available facets and their values |
> | `POST /api/v1/search/reindex` | POST | Force full reindex (admin only) |
>
> Response format includes `facet_counts`:
> ```json
> {
>   "query": "identity security",
>   "total": 14,
>   "results": [...],
>   "facet_counts": {
>     "type": { "spec": 5, "task": 7, "mr": 2 },
>     "workspace": { "platform-team": 10, "app-team": 4 },
>     "status": { "approved": 3, "pending": 2 }
>   }
> }
> ```

## Implementation Plan

1. **Add `GET /api/v1/search/suggest` endpoint** (`crates/gyre-server/src/api/search.rs`):
   - Accept `q` (prefix) query parameter
   - Return top-N (default 10) entity title/name matches by prefix
   - Query the SearchPort with a prefix search (add `suggest` method to SearchPort trait if needed)
   - Response: `{"suggestions": [{"text": "identity-security.md", "entity_type": "spec", "entity_id": "..."}]}`

2. **Add `suggest` to SearchPort** (`crates/gyre-ports/src/search.rs`):
   - Add `async fn suggest(&self, prefix: &str, limit: usize) -> Result<Vec<SearchSuggestion>>`
   - `SearchSuggestion { text: String, entity_type: String, entity_id: String }`
   - Implement in SQLite adapter — use prefix matching on the title field of the search index

3. **Add `GET /api/v1/search/facets` endpoint** (`crates/gyre-server/src/api/search.rs`):
   - No query parameter required — returns all available facets and their current value counts
   - Response: `{"facets": {"type": {"spec": N, "task": N, ...}, "workspace": {...}, "status": {...}}}`
   - Query the search index to aggregate facet counts

4. **Enhance search response with `facet_counts`**:
   - Modify `SearchResponse` in `search.rs` to include `facet_counts: HashMap<String, HashMap<String, usize>>`
   - After executing the search query, compute facet counts from the result set (or from a parallel aggregation query)
   - Counts should reflect the filtered result set, not the entire index

5. **Register new routes** in `crates/gyre-server/src/api/mod.rs`:
   - `.route("/api/v1/search/suggest", get(search::suggest_handler))`
   - `.route("/api/v1/search/facets", get(search::facets_handler))`

6. **Tests**:
   - Test suggest endpoint returns prefix-matched results
   - Test facets endpoint returns facet counts
   - Test search response includes facet_counts
   - Test empty suggest query returns empty list

## Acceptance Criteria

- [ ] `GET /api/v1/search/suggest?q={prefix}` returns autocomplete suggestions
- [ ] `GET /api/v1/search/facets` returns available facets with value counts
- [ ] `GET /api/v1/search` response includes `facet_counts` alongside results
- [ ] SearchPort trait has `suggest` method with adapter implementation
- [ ] Routes registered in mod.rs
- [ ] Tests cover all three endpoints

## Agent Instructions

- Read `crates/gyre-server/src/api/search.rs` for the existing search handler and response types
- Read `crates/gyre-ports/src/search.rs` for the SearchPort trait (index, search, delete, reindex_all)
- Read `crates/gyre-server/src/api/mod.rs` line ~596 for current search route registration and line ~478-481 for admin search/reindex
- The SearchPort adapter is in the sqlite or mem adapters — find it via the `SearchPort` implementation
- Follow the existing response patterns in search.rs for consistency
- Both new endpoints should be authenticated (within the standard auth middleware layer, same as the existing search endpoint)
