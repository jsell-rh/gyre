# Review — task-154 (Implement search query language: quoted phrases, faceted filtering)

Spec: `specs/system/search.md` §Query Language (Simple Search, Quoted
Phrases, Faceted Filtering).
Candidate under review: `1dec619f` (base `e981bf29`, branch
`pipeline/task-154/2f408735e45944edabda780ccf692eb9-1`).
Verdict: **needs-revision** — one blocking finding (F1: the gyre-server
test suite does not compile at the candidate); the underlying production
implementation itself is genuine and well tested everywhere it builds.

## Probes run (exact candidate checkout)

- `cargo check -p gyre-server --tests` → **fails, exit 101, 3 × E0063**
  (`/tmp/stage/review-evidence/cargo-check-gyre-server-tests.txt`).
- `cargo test -p gyre-domain search_query` → 25/25 pass (parser: terms,
  phrases, facets, OR-within-facet, malformed facets, relative/absolute
  time bounds, invalid values, first-value-wins).
- `cargo test -p gyre-adapters search` → 35/35 pass (SQLite FTS5 phrase
  adjacency vs scattered terms, facet AND-across/OR-within,
  `since:`/`until:` relative+absolute, facet-only browse, facet_counts
  honoring query-language filters; mem mirror; PG SQL-shape regressions
  incl. the restored `s.tsv @@ q` predicate).
- `cargo check -p gyre-server --test search_wiring` → compiles clean.
- Read-verification of the full diff base..candidate (parser, ports,
  three adapters, REST handler, MCP handler, wiring tests, docs,
  coverage rows).

## Verified working (no findings)

- **Parser** (`gyre-domain/src/search_query.rs`): faithful to the spec
  grammar — quoted spans become phrases (unterminated quote runs to end,
  empty spans dropped), whitelisted `key:value` facets (all 12 spec keys
  including `assigned:`/`author:`/`approval:`/`scope:`), OR within a key,
  malformed facets (`type:` with no value) dropped, unknown keys fall
  back to terms, terms split on non-alphanumeric boundaries mirroring the
  FTS `unicode61` tokenizer. Time facets resolve relative (`7d`/`24h`/`2w`)
  and absolute (`YYYY-MM-DD`, `since`=start-of-day, `until`=end-of-day,
  leap-year correct via `days_from_civil`) to epoch bounds. Pure domain
  logic, no infrastructure deps — hexagonal boundary respected.
- **Adapters**: all three execute the parse as real index-query filters.
  SQLite FTS5 quotes every term/phrase into the MATCH expression (safe
  against FTS5 syntax injection — hostile-query test proves it), facets
  become `IN (…)` WHERE clauses on index columns or `json_extract` on the
  metadata JSON (key whitelisted, values bound), time bounds filter the
  persisted timestamp (reserved `updated_at` metadata key — the FTS5
  schema cannot be ALTERed; PG gets a real column + `ALTER … ADD COLUMN
  IF NOT EXISTS`). Facet-only queries browse without a MATCH clause.
  AND-across / OR-within composition is proven by
  `facet_filters_and_or_within_compose` against real FTS5 storage.
- **Integration**: the REST handler parses `q` first with query-string
  facets taking precedence over the `entity_type`/`workspace_id` params
  (both directions tested in the wiring suite); the MCP `gyre_search`
  handler applies the same parse and precedence (handler code read-
  verified); scope stays auth-derived in both paths (task-203 composition
  preserved — `SearchQuery::parse` starts fail-closed, `with_scope`
  attaches the caller's resolved access).
- **Rebase resolution** (merge `dbe510212` onto task-203): the conflict
  resolution combines both features' semantics rather than dropping
  either — verified by the merged adapter suites (scope + query-language
  filters in the same WHERE builder, never post-filtered).

## Findings

- **F1 (blocking, code):** `cargo check -p gyre-server --tests` fails
  with 3 × E0063 in `crates/gyre-server/src/mcp.rs`:
  - :3997/:4013 — the new `mcp_search_query_language` test's two
    `SearchDocument` literals omit the required `tenant_id` field;
  - :3842 — the pre-existing helper for
    `mcp_search_is_access_scoped_to_the_agent_token` (compiling at base)
    was not updated for the `updated_at` field this diff adds to
    `SearchDocument`.
  The entire `cargo test -p gyre-server` build fails, so the MCP
  acceptance criterion's dedicated test has never compiled or run, and
  every other server test is blocked behind it.
- **F2 (same test, follows from F1):** even after a naive compile fix,
  the new MCP test would fail at runtime: its documents are indexed with
  no tenant, and the mem adapter's `scope_visible` requires
  `doc.tenant_id == Some(query.tenant_id)` while the test's global dev
  token authenticates as Admin in tenant `"default"`. The docs need
  `tenant_id: Some("default")` (the pattern the sibling scoped test at
  :3849 already uses). The test was authored, never executed.

## Verifier must run (deferred from this sandbox)

- `cargo test -p gyre-server` — after F1/F2 are fixed; this is the suite
  the candidate broke.
- `cargo test -p gyre-server --test search_wiring` — the HTTP
  integration leg (compiles clean here; binary link exceeds the sandbox's
  4 GiB build guard).
- Full `cargo test -p gyre-adapters`, `cargo test -p gyre-domain`
  (search-filtered subsets ran green here: 35/35, 25/25).
