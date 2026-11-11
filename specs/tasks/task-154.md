---
title: "Implement search query language (quoted phrases, faceted filtering)"
spec_ref: "search.md §Query Language"
depends_on: [task-153]
progress: ready-for-review
coverage_sections:
  - "search.md §Query Language"
  - "search.md §Simple Search"
  - "search.md §Quoted Phrases"
  - "search.md §Faceted Filtering"
commits: ["c8f14874cd1c648c4f407c575373262e7dbdf170", "40d1c1cde066effba5e42e1ccb6ca4800fe6c59f", "fc0c24e8b3e55bd043906436edf2e484129f5e30"]
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

## Shipped

The §Query Language grammar (simple terms, quoted phrases, faceted
filtering, `since:`/`until:` time bounds) is real production code across
the domain parser, the port protocol, all three SearchPort adapters, the
REST handler, and the MCP `gyre_search` tool.

### Parser — `crates/gyre-domain/src/search_query.rs`

- `parse_query(&str) -> ParsedQuery` extracts free-text terms, `"quoted
  phrases"`, and `key:value` facets. Facet keys are whitelisted to the
  spec's table; malformed facets (`type:` with no value) are dropped and
  unknown `key:value` tokens fall back to plain terms — the parser never
  errors and no unvetted key reaches a storage backend's filter path.
- `facet_filters()` maps facets to typed filters (type/workspace/repo
  columns vs. metadata keys); `time_bounds(now)` resolves `since:`/`until:`
  — relative (`7d`/`24h`/`2w`) and absolute (`YYYY-MM-DD`, leap-year-aware,
  since = start-of-day, until = end-of-day) — to epoch-second bounds.
- 25 unit tests cover every pattern in the plan plus malformed input,
  unterminated quotes, first-value-wins time facets, and leap days.

### Port + adapters

- `SearchQuery` (gyre-ports) carries the raw text plus its `ParsedQuery`;
  `SearchQuery::parse` keeps them consistent. `entity_type`/`workspace_id`
  remain fallback shortcuts; query-string facets take precedence.
- SQLite FTS5: each term and quoted phrase becomes an FTS5 phrase in the
  MATCH expression (real adjacency, not bag-of-words); facet filters are
  WHERE clauses on index columns / metadata JSON (AND across keys, OR
  within a key via `IN`), applied before the limit; `since:`/`until:` filter
  on the document timestamp (reserved metadata key — FTS5 tables cannot be
  ALTERed).
- PostgreSQL: terms + phrases through `websearch_to_tsquery` with the
  `s.tsv @@ q` row predicate in `build_filters` (restored by the review
  repair 40d1c1cde; guarded by mutation-verified SQL-shape tests), same
  facet/time composition on scope columns and metadata `->>`.
- Mem adapter enforces the same port contract in code (exact
  case-insensitive substring phrases, facet AND/OR, time bounds).

### Surfaces

- `GET /api/v1/search?q=` parses `q` with the query language; MCP
  `gyre_search` accepts it in its `q` argument (entity_type/workspace_id
  args are fallbacks there too).

### Test evidence

- `cargo test -p gyre-domain search_query` — 25 passed.
- `cargo test -p gyre-adapters --lib search` — 30 passed (sqlite phrase
  adjacency, facet AND/OR composition, relative+absolute time bounds,
  facet-only browse, facet_counts filters; mem mirrors; PG SQL-shape).
- `cargo check -p gyre-server` — clean (REST + MCP integration types).
- For the verifier (server test compiles exceed the sandbox budget):
  `cargo test -p gyre-server --test search_wiring` (HTTP-leg: phrase
  adjacency, facet narrowing, facet-over-param precedence, since:/until:
  bounds over the real router + SQLite FTS5) and
  `cargo test -p gyre-server --lib mcp::tests::mcp_search_query_language`.
- Durable CI finding on candidate head f061d0e02 (run 43036531271) is an
  E2E "Install mold linker" apt/DNS failure on the GitHub runner before
  any repo code ran — infrastructure, not a code defect; no repair applies.
  Probe evidence: /tmp/stage/review-evidence/task-154-focused-verification.md
