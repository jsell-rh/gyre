---
title: "Implement search autocomplete and facets endpoints"
spec_ref: "search.md §API"
depends_on: [task-201]
progress: ready-for-review
coverage_sections:
  - "search.md §API"
  - "search.md §Response Format"
commits: ["6da69d14fd64403a89ece7fcc2b2ce5bbbaaac33", "60493e2757358bd1012d709fbe4ced4cbf6e8dc6", "2750325700f93b1cd1aa5c43890ebb92e84bc469", "774499be5a1b4438739e0733f2e6b333141bd803"]
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

## Shipped

All four §API endpoints and the §Response Format `facet_counts` field are
implemented as real production code across the port, all three adapters, and
the server API layer; the acceptance criteria above are met by that code.

### Endpoints (crates/gyre-server/src/api/search.rs, api/mod.rs:615-617)

- `GET /api/v1/search/suggest?q={prefix}&limit=10` (max 50) — case-insensitive
  prefix match on entity titles; response
  `{"suggestions": [{text, entity_type, entity_id}]}`; empty/whitespace prefix
  returns an empty list, never "every title in the index".
- `GET /api/v1/search/facets` — whole-index aggregation; response
  `{"facets": {"type": {...}, "workspace": {...}, <document facets>}}`.
- `GET /api/v1/search` — response now carries `facet_counts` aggregating the
  query's **filtered match set** (same terms + `entity_type`/`workspace_id`
  filters, result-page `limit` ignored), not the truncated page and not the
  whole index.
- Both new GET routes sit behind the standard auth middleware with ABAC
  resolver entries (abac_middleware.rs:377-379); reindex stays admin-only at
  `/api/v1/admin/search/reindex`.

### Port + adapters (crates/gyre-ports/src/search.rs; gyre-adapters)

- `SearchPort::suggest(prefix, limit) -> Vec<SearchSuggestion>` and
  `SearchPort::facet_counts(Option<SearchQuery>) -> FacetCounts` added to the
  trait (no default impl — every implementor must provide them).
- SQLite (FTS5): `title LIKE 'prefix%' ESCAPE '\'` with user-supplied `%`,
  `_`, `\` escaped so they match literally; facet aggregation reuses the exact
  `MATCH` + filter WHERE-clauses of `search()` minus the LIMIT.
- PostgreSQL (tsvector): same contract via `title ILIKE ... ESCAPE` and
  `websearch_to_tsquery` match-set aggregation.
- Mem adapter: same contract in code (lowercased prefix match, filters
  mirrored from `search()`, limit ignored for aggregation).

### Test evidence

Tests are written at two levels:

- `crates/gyre-server/tests/search_wiring.rs` — endpoint legs through the
  full middleware router (oneshot): suggest prefix match, empty-prefix and
  non-matching-prefix empty lists, 401 without a token on both new endpoints,
  `/search` response `facet_counts` over the filtered match set, `/facets`
  whole-index counts.
- Adapter `#[cfg(test)]` suites: prefix semantics (mid-title terms must NOT
  match), LIKE-wildcard escaping (`100%`/`a_` literal), whole-index vs
  filtered aggregation, limit-ignored aggregation, entity_type/workspace
  filter restriction, non-matching query aggregates nothing.

### Execution status (round 9)

This sandbox cannot reach crates.io (TCP connection refused to
index/static.crates.io; no cached registry, no prebuilt target/), so
`cargo test -p gyre-server --test search_wiring` and the adapter suites
could not be executed here — an infrastructure restriction, not a code
defect. Remote verification must run:

    cargo test -p gyre-server --test search_wiring
    cargo test -p gyre-adapters search

Offline checks that ran this round and pass: `rustfmt --check` on
crates/gyre-ports/src/lib.rs, `python3 scripts/check-rustfmt-diff.py
d81b0fe4ab93abff06089432e4a8fca5be6e5d31` (exit 0, 9 Rust files clean),
and `check-task-commit-attribution.sh` after adding this round's commit
to the frontmatter (raw logs: /tmp/stage/review-evidence/). Earlier
rounds' logs remain in the same directory (`*-round8.log` and older).

### Round 9 review repair

Review finding fbb29ab8: the one-line `pub use search::{...}` re-export
added to `crates/gyre-ports/src/lib.rs` by task commit 774499be was 103
chars, over rustfmt's 100-col max_width, so the CI Format job
(check-rustfmt-diff.py) failed on any PR carrying the diff. Fixed in
commit 6da69d14 by wrapping the use item as rustfmt suggests; the
repo's changed-lines policy now passes against the review base
d81b0fe4. No production behavior changed.
