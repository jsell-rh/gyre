---
title: "Implement search autocomplete and facets endpoints"
spec_ref: "search.md §API"
depends_on: [task-201]
progress: ready-for-review
coverage_sections:
  - "search.md §API"
  - "search.md §Response Format"
commits: ["60493e2757358bd1012d709fbe4ced4cbf6e8dc6", "2750325700f93b1cd1aa5c43890ebb92e84bc469", "774499be5a1b4438739e0733f2e6b333141bd803"]
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

## Acceptance Criteria

- [x] `GET /api/v1/search/suggest?q={prefix}` returns autocomplete suggestions
- [x] `GET /api/v1/search/facets` returns available facets with value counts
- [x] `GET /api/v1/search` response includes `facet_counts` alongside results
- [x] SearchPort trait has `suggest` method with adapter implementation
- [x] Routes registered in mod.rs
- [x] Tests cover all three endpoints

## Shipped

All four §API endpoints and the §Response Format `facet_counts` field are
implemented as real production code across the port, all three adapters, and
the server API layer.

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
  resolver entries (abac_middleware.rs:378-379); reindex stays admin-only at
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

Execution caveat (recorded in /tmp/stage/review-evidence/): this sandbox
cannot reach crates.io (TCP connection refused to index/static.crates.io;
no cached registry), so `cargo test -p gyre-server --test search_wiring` and
the adapter suites could not be executed here. The prior checkpoint iteration
had crates.io access, compiled the workspace for ~4h, and was killed by its
budget while the test binary was still linking — it never produced a run
either. Remote verification must run:

    cargo test -p gyre-server --test search_wiring
    cargo test -p gyre-adapters search

Offline checks that did run here and pass: `rustfmt --check` on all six
task-touched files (formatting fixed this session), `check-abac-route-registry.sh`,
`check-arch.sh`, `check-mem-port-contracts.sh`, `check-byte-slice-truncation.sh`,
`check-scope-literal-defaults.sh` (raw log: review-evidence/offline-checks-raw.log).
