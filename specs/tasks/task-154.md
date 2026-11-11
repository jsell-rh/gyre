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
commits: ["001df9412a9d85f2e51e7d6cdfd8f0da1d919916", "fc0c24e8b3e55bd043906436edf2e484129f5e30", "40d1c1cde066effba5e42e1ccb6ca4800fe6c59f", "c8f14874cd1c648c4f407c575373262e7dbdf170"]
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
filtering, `since:`/`until:` time bounds) is implemented as real production
code across the domain parser, the port protocol, all three SearchPort
adapters, the REST handler, and the MCP `gyre_search` tool.

### Parser — `crates/gyre-domain/src/search_query.rs` (new)

- `parse_query(&str) -> ParsedQuery` extracts free-text terms (split on
  non-alphanumeric boundaries, mirroring the FTS `unicode61` tokenizer),
  `"quoted phrases"`, and `key:value` facets. Facet keys are whitelisted to
  the spec's table (type/workspace/repo/status/priority/assigned/author/
  label/approval/scope/since/until); malformed facets (`type:` with no value)
  are dropped, unknown `key:value` tokens fall back to plain terms — the
  parser is forgiving and never errors, and no unvetted key reaches a
  storage backend's facet-filter path.
- `ParsedQuery::facet_filters()` maps facets to typed filters (entity type /
  workspace / repo columns vs. metadata keys); `ParsedQuery::time_bounds(now)`
  resolves `since:`/`until:` — relative (`7d`/`24h`/`2w`) and absolute
  (`YYYY-MM-DD`, leap-year-aware, since = start-of-day, until = end-of-day) —
  to epoch-second bounds via a dependency-free `days_from_civil`.
- 25 unit tests cover every pattern in the plan plus malformed input,
  unterminated quotes, first-value-wins time facets, and leap days.

### Port — `crates/gyre-ports/src/search.rs`

- `SearchQuery` carries `parsed: ParsedQuery` alongside the raw `query`
  text (built via `SearchQuery::parse`, so the two stay consistent);
  `entity_type`/`workspace_id` remain as documented fallback shortcuts.
- `SearchDocument` gained `updated_at: Option<i64>` — the document timestamp
  the time facets filter on; `None` stamps index time in the adapter.

### Adapters — all three, filters as real index-query WHERE clauses

- **SQLite FTS5**: MATCH expression built from quoted terms + phrases;
  facets filter `entity_type`/`workspace_id`/`repo_id` columns and metadata
  JSON keys via bound-parameter `json_extract` paths; `since:`/`until:`
  filter on the timestamp persisted under the reserved metadata key
  (`gyre_ports::search::UPDATED_AT_KEY`) because the specced FTS5 schema has
  no timestamp column and SQLite forbids ALTER on virtual tables (verified:
  `virtual tables may not be altered`). Facet-only queries browse without a
  MATCH clause (constant rank, body-prefix snippet). Filters apply BEFORE
  the result limit — not post-filtered.
- **PostgreSQL**: terms + quoted phrases flow through
  `websearch_to_tsquery` (implicit AND, real phrase semantics) and are
  applied as the `s.tsv @@ q` row filter — restored by the review repair
  (round 2): a `build_filters()` rewrite had reserved the tsquery for
  ranking but dropped the match predicate, so every text query returned all
  facet-passing rows. Four SQL-shape regression tests in
  `postgres/search.rs` pin the predicate (and the facet-only / bind-order
  shapes); a mutation check confirmed they fail when the predicate is
  removed. Facet filters use `IN` lists on columns / `metadata->>key`;
  `updated_at` is a real BIGINT column (`ADD COLUMN IF NOT EXISTS` covers
  pre-existing deployments).
- **Mem**: same contract in code — phrase = exact case-insensitive
  substring, facet AND/OR composition, time bounds on `updated_at`.
- `facet_counts(Some(q))` in all three adapters aggregates the same
  text + facet filtered match set (limit ignored), and the reserved
  timestamp key never leaks into result facets or facet-count aggregations.

### Server — REST + MCP

- `GET /api/v1/search?q=` (api/search.rs) parses `q` first; a `type:` /
  `workspace:` facet in the query string takes precedence over the
  `entity_type` / `workspace_id` params (they remain as fallback shortcuts).
- MCP `gyre_search` (mcp.rs) parses `q` with the same grammar and the same
  precedence; the tool description and input schema document the language.
- The three index-on-create sites (tasks/agents/MRs) pass the entity's
  real update time as `updated_at`.

### CI repair (this round)

Durable finding `a9d572fb2408486490b71d2ac8bf0bcd`: the e2e job at PR head
f061d0e02 (run 43036531271) failed at "Install mold linker" — apt could
not resolve `azure.archive.ubuntu.com`, a transient GitHub-runner DNS
failure, not a code defect. Because `.cargo/config.toml` hard-wires
`-fuse-ld=mold`, the failed install bricked every later build step.

Fix: the four inline `sudo apt-get install -y mold` steps (e2e plus
clippy/test/build-release in ci.yml) now use a composite action,
`.github/actions/setup-linker`, which retries the install with growing
backoff (20s/40s/60s) and, if apt remains unreachable, exports
`RUSTFLAGS=-C link-arg=-fuse-ld=lld` so the job still builds with the
runner image's preinstalled clang+lld (also a parallel low-memory
linker, preserving the OOM rationale in `.cargo/config.toml`). The
jobs' checks are unchanged — only the link step degrades, with a
`::warning::`. Verified: the `RUSTFLAGS` env var replaces the config's
target rustflags (only the env flags reach rustc — checked with
`cargo build -v` on a scratch crate), clang+lld links working binaries,
and stubbed-apt runs of the action cover persistent failure (4 attempts,
fallback env written, exit 0), recovery on the 3rd attempt (mold used,
no fallback), and immediate success. Evidence:
`/tmp/stage/review-evidence/ci-mold-fallback/`.

An earlier verification round's rustfmt repair (whitespace-only format
of this task's changed lines) is preserved in commit fc0c24e8b.

### Test evidence

- Re-run at this round's head: `cargo test -p gyre-domain search_query`
  25 passed; `cargo test -p gyre-adapters search` 30 passed
  (sqlite + mem + postgres SQL-shape). Server-side suites remain
  deferred to the independent verifier per the 4 GiB budget.

- `gyre-domain` `search_query` — 25 parser/time-bound tests.
- `gyre-adapters` SQLite — `quoted_phrases_match_exactly_not_individual_terms`,
  `facet_filters_and_or_within_compose`,
  `time_facets_since_and_until_relative_and_absolute`,
  `facet_only_query_browses_without_text`,
  `facet_counts_honor_query_language_filters`, plus all pre-existing search
  tests unchanged in intent (constructions migrated to `SearchQuery::parse`).
- `gyre-adapters` mem — `query_language_phrases_and_facets`,
  `query_language_time_bounds`, `query_language_facet_counts_and_malformed_queries`.
- `gyre-server` — `tests/search_wiring.rs` drives phrases, facet narrowing,
  facet-only browse, facet-over-param precedence, and since/until bounds
  through the real middleware router against the SQLite FTS5 backend;
  `mcp.rs` `mcp_search_query_language` exercises the same grammar over MCP
  (mem-backed state). Server test compilation is deferred to the
  independent verifier per the 4 GiB budget; `cargo check -p gyre-server`
  is clean.
- Known limit: the PostgreSQL leg compiles and mirrors the SQLite contract
  but has no runtime test — the repo has no PG test harness. The restored
  `s.tsv @@ q` predicate is pinned by SQL-shape regression tests
  (mutation-verified); execution against a live PG instance is recorded
  for remote verification.
