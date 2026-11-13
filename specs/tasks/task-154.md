---
title: "Implement search query language (quoted phrases, faceted filtering)"
spec_ref: "search.md §Query Language"
depends_on: [task-153]
progress: complete
coverage_sections:
  - "search.md §Query Language"
  - "search.md §Simple Search"
  - "search.md §Quoted Phrases"
  - "search.md §Faceted Filtering"
commits: ["2acdab522376a73621f2672d397ac87fab323357", "1bb3c01bce54dda1ac5146c59aab3ce9c2a9c7e2", "2c3eff66174a6d34c4bc6a8d72c11a69ca224ecb", "5ace1b3aca5c5fd2e2b7864c186e7f0e1aa17fbd", "fc0c24e8b3e55bd043906436edf2e484129f5e30", "40d1c1cde066effba5e42e1ccb6ca4800fe6c59f", "c8f14874cd1c648c4f407c575373262e7dbdf170"]
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

The §Query Language grammar ships as production code across the stack:
the pure parser (`crates/gyre-domain/src/search_query.rs`: terms,
quoted phrases, facets with AND-across/OR-within composition, and
`since:`/`until:` relative `7d` and absolute `2026-03-01` time bounds),
the port protocol (`crates/gyre-ports/src/search.rs` carries the parse
plus the caller's resolved access scope), all three SearchPort adapters
(SQLite FTS5, PostgreSQL, in-memory), the REST handler
(`GET /api/v1/search?q=`), and the MCP `gyre_search` tool. Every
query-language query executes inside the caller's task-203 access
scope; the `entity_type`/`workspace_id` params remain narrowing
shortcuts that query-string facets override. Parser and integration
tests live in `gyre-domain` (`search_query`), `gyre-server`
(`mcp_search_query_language`, REST search tests), and
`tests/search_wiring.rs`.

### Repair round (2acdab52)

The full-suite verification round (merged with main at `44a01390`)
failed `tests/search_wiring.rs:sqlite_url_wires_fts5_search_port` at
line 443: the unquoted `count quokkas` query returned 1 of the
expected 2 results. Test-seeding bug, same class as the MCP round
(`1bb3c01b`): the directly-indexed `scattered` and `ancient` docs
carried no tenant and no workspace, so under the task-203 access
scope's fail-closed tenant boundary they were stored under the empty
tenant and invisible to the admin HTTP caller (tenant `"default"`).
The implementation was correct; the seeding contradicted the
access-scoping contract. Commit `2acdab52` seeds both docs with the
caller's tenant, mirroring `mcp_search_query_language`. Side effect:
the `since:7d` assertion now genuinely exercises the time bound — the
40-day-old `ancient` doc is visible again and must be excluded by the
bound itself, not by the tenant filter.

### Test evidence (this sandbox)

- `cargo check -p gyre-server --test search_wiring` — exit 0
- `rustfmt --check` on the changed file — exit 0
- `cargo test -p gyre-adapters --lib sqlite::search` — 18/18 passed
  (phrases, facet AND/OR, time bounds, access scoping)
- `cargo test -p gyre-adapters --lib mem_search` — 11/11 passed
- `cargo test -p gyre-domain --lib` — 499/499 passed (parser suite)

Evidence saved under `/tmp/stage/review-evidence/`
(`task-154-search-wiring-tenant-seed-fix.txt`).

### Verifier must run

- `cargo test -p gyre-server --test search_wiring`
- `cargo test --all --quiet`

(gyre-server test builds exceed this sandbox's 4 GiB budget and run in
the independent verifier.)
