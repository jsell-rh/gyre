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
commits: ["fc0c24e8b3e55bd043906436edf2e484129f5e30", "40d1c1cde066effba5e42e1ccb6ca4800fe6c59f", "c8f14874cd1c648c4f407c575373262e7dbdf170", "dbe51021272cc9d68fbde82476cdcde45cc4b04e"]
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
filtering, `since:`/`until:` time bounds) ships as real production code
across the domain parser, the port protocol, all three SearchPort
adapters, the REST handler, and the MCP `gyre_search` tool — and this
round rebases that work onto main's task-203 access scoping so both
features hold simultaneously: every query-language query is executed
inside the caller's access scope, never beside it.

### This round — rebase onto task-203 (merge commit `dbe510212`)

Main's task-203 (access scoping) and this branch's task-154 (query
language) both rewrote the same SearchPort surface, so the rebase
conflicts were resolved by combining the semantics, not picking a side:

- `SearchQuery` (crates/gyre-ports/src/search.rs) carries both the
  query-language parse (`parsed`) and the caller's resolved scope
  (`tenant_id`, `accessible_workspace_ids`, `accessible_repo_ids`).
  `SearchQuery::parse` starts fail-closed — no tenant, empty accessible
  sets — and the REST/MCP handlers attach the auth-derived scope via the
  new `with_scope` builder. Scope is never taken from the query string;
  the `entity_type`/`workspace_id` params/facets remain narrowing
  filters that can only intersect the accessible set.
- `facet_counts` keeps task-203's non-`Option` signature: a blank query
  aggregates the caller's whole accessible set (the `/search/facets`
  contract), while any text/facet/shortcut filter restricts the
  aggregation to the same filtered match set `search()` uses (task-154's
  semantics). The `Option<SearchQuery>` whole-index shape from the
  pre-rebase branch was dropped — `None`-means-whole-index would leak
  cross-tenant counts.
- All three adapters compose scope AND query-language filters as WHERE
  clauses of the index query itself, never post-filtered: the mem
  adapter folds both into one `scope_visible` predicate; the SQLite
  adapter pushes a new `build_scope` into the existing `WhereBuilder`
  next to `build_filters`; the PG adapter does the same with
  `PgWhereBuilder` (the boxed-query `scope_filter_sql` machinery was
  removed in favor of the single builder, with `s.*`-aliased SQL kept so
  PG accepts the scope clauses). Entity indexing keeps task-154's
  `updated_at` timestamp inside task-203's tenant-deriving `index_doc`
  wrapper.

### Test evidence (focused probes, logs in /tmp/stage/review-evidence)

- `cargo test -p gyre-adapters --lib search` — 33 passed: both the
  task-154 query-language suite (phrases vs scattered terms, facet
  AND-across/OR-within, `since:`/`until:` relative + absolute,
  facet-only browse, malformed queries, facet_counts honoring query
  filters) and the task-203 access-scoping suite (tenant boundary,
  accessible workspace/repo sets, empty-set fail-closed,
  client-workspace_id-cannot-widen, facet-count and suggest leak
  guards).
- `cargo test -p gyre-adapters --lib postgres::search` — 6 passed: the
  four `s.tsv @@ q` SQL-shape regression tests plus the two adapted
  scope-shape tests (`scope_clauses_are_s_prefixed_index_query_filters`,
  `empty_accessible_set_compiles_to_no_rows`).
- `cargo test -p gyre-adapters --lib` (full suite) — 479 passed, 0
  failed; `cargo test -p gyre-domain --lib` — 499 passed (parser tests
  included).
- `cargo check -p gyre-server --test search_wiring` — compiles clean.

### Verifier must run (blocked here by the 4 GiB server-build guard)

- `cargo test -p gyre-server --test search_wiring` — the HTTP
  integration leg: phrase adjacency (`q=%22count quokkas%22`),
  facet narrowing, facet-vs-param precedence, `since:` bounds, and
  (from task-203) scope-filtered search/suggest/facets over the real
  middleware stack.

### Standing caveats (unchanged by this round)

- The PG runtime leg has no in-repo test harness; SQL-shape tests plus
  remote verification against live PG remain the evidence path.
- `specs/coverage/system/search.md` rows 3–8 and 16 describe tasks
  201/202/203/155 and were not touched by this task's scope.
