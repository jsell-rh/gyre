# Review — task-154 (search.md §Query Language — quoted phrases, faceted filtering)

Spec: `specs/system/search.md` §Query Language (§Simple Search, §Quoted
Phrases, §Faceted Filtering). Task: implement the user-facing query language
— space-separated AND terms, `"quoted phrases"` matched as exact phrases,
`key:value` facet filters (AND across keys, OR within a key), `since:`/
`until:` time bounds (relative `7d` and absolute `2026-03-01`) — integrated
into the REST `GET /api/v1/search?q=` handler and the MCP `gyre_search` tool,
with `entity_type`/`workspace_id` params retained as fallback shortcuts.

Candidate `d8929d58` on `pipeline/task-154/2a526f38519d48988dbbce10ac19f250-1`,
diffed from base `18adceb68`. This is review round 3: round 2 found the
PostgreSQL `build_filters()` rewrite had dropped the `s.tsv @@ q` full-text
predicate (ranking-only cross join — every text query returned all
facet-passing rows); the repair commit `40d1c1c` restored it and added
SQL-shape regression tests. Round 3 verifies the repair and the full
candidate. Verdict: **complete** — approved.

## Probes (evidence: /tmp/stage/review-evidence/probe-log.md)

All via `/tmp/stage/bin/cargo`; working tree verified clean and
byte-identical to the candidate SHA before and after mutation probes.

- `cargo test -p gyre-domain --lib search_query` → 25 passed, 0 failed.
- `cargo test -p gyre-adapters --lib sqlite::search` → 16 passed, 0 failed.
- `cargo test -p gyre-adapters --lib mem_search` → 10 passed, 0 failed.
- `cargo test -p gyre-adapters --lib postgres::search` → 4 passed, 0 failed.
- `cargo test -p gyre-server --test search_wiring` → 1 passed (drives the
  full middleware router against the real SQLite FTS5 backend: phrase
  adjacency over HTTP, facet narrowing, facet-only browse, facet-over-param
  precedence, since/until bounds, filtered-set facet_counts, durability).
- `cargo test -p gyre-server --lib mcp_search` → 1 passed (`mcp_search_query_language`
  over MCP tools/call with mem-backed state: phrase match, facet narrowing,
  `type:` facet overriding the `entity_type` arg).
- `scripts/check-arch.sh`, `check-mem-port-contracts.sh`,
  `check-byte-slice-truncation.sh` → all OK. No new routes, migrations, or
  MCP write tools in the diff, so the ABAC-route-registry /
  migration-version / MCP-write-gate checks are unaffected by construction.

Mutation kills (each applied, test run, reverted, restore diff-verified):

1. **Phrase semantics degraded** (SQLite `build_match_expr` emits phrases
   unquoted → FTS5 implicit AND-of-terms): `quoted_phrases_match_exactly_not_individual_terms`
   FAILS ("phrase must require adjacency: [adjacent, scattered]").
2. **PG full-text predicate removed** (the round-2 defect class, exact line
   `w.push("s.tsv @@ q")` deleted): `text_query_emits_tsv_match_predicate`
   FAILS ("full-text predicate missing from WHERE").
3. **SQLite facet filters dropped** (`for filter in parsed.facet_filters()`
   loop removed): `facet_filters_and_or_within_compose` FAILS.

## Verified

- **Parser** (`gyre-domain/src/search_query.rs`, pure logic, no infra deps):
  `parse_query` extracts terms (split on non-alphanumeric boundaries,
  mirroring the FTS `unicode61` tokenizer), quoted phrases (unterminated
  quote runs to end; empty spans dropped), and whitelisted `key:value`
  facets. Facet keys are whitelisted to the spec table (plus
  assigned/author/approval/scope attribute keys); malformed facets
  (`type:` no value) are dropped, unknown `key:value` tokens fall back to
  plain terms — forgiving, never errors. `time_bounds(now)` resolves
  relative (`7d`/`24h`/`2w`) and absolute (`YYYY-MM-DD`, leap-year-aware,
  since=start-of-day, until=end-of-day) values via a dependency-free
  `days_from_civil`; invalid values resolve to no bound. 25 unit tests cover
  every pattern in the plan plus malformed input and edge cases.
- **Port** (`gyre-ports/src/search.rs`): `SearchQuery` carries `parsed`
  alongside raw `query` (kept consistent via `SearchQuery::parse`);
  `SearchDocument.updated_at` added. `gyre-ports` already depended on
  `gyre-domain` at base (no Cargo.toml change; domain depends only on
  gyre-common — no cycle, arch lint passes).
- **SQLite FTS5 adapter**: MATCH expression quotes every term and phrase —
  the parser guarantees terms are single alphanumeric tokens and phrases
  contain no `"`, so no user input can reach the FTS5 parser as syntax
  (the hostile-query test proves it). Facets are real WHERE clauses
  (`entity_type`/`workspace_id`/`repo_id` IN-lists; metadata via bound
  `json_extract` paths), applied BEFORE the result limit. `since:`/`until:`
  filter on the timestamp persisted under the reserved
  `UPDATED_AT_KEY` metadata key (justified: the specced FTS5 schema has no
  timestamp column and SQLite forbids ALTER on virtual tables); the key is
  stripped from result facets and facet-count aggregations. Facet-only
  queries browse without a MATCH clause (constant rank, body-prefix
  snippet, timestamp-ordered).
- **PostgreSQL adapter**: terms + quoted phrases flow through
  `websearch_to_tsquery` AND are applied as the `s.tsv @@ q` row filter —
  the round-2 defect is genuinely repaired in `build_filters` (shared by
  `search()` and `facet_counts()`), pinned by four SQL-shape regression
  tests (mutation-verified here, probe 2). Facet filters use IN-lists on
  columns / `metadata->>`; `updated_at` is a real BIGINT column with
  `ADD COLUMN IF NOT EXISTS` for pre-existing deployments.
- **Mem adapter**: same contract in code — phrase = exact case-insensitive
  substring, AND across / OR within facets, time bounds on `updated_at`,
  facet-only browse, limit-ignoring facet_counts.
- **REST + MCP**: both parse `q` first; a `type:`/`workspace:` facet in the
  query string takes precedence over the `entity_type`/`workspace_id`
  params/args (tested at both surfaces); the three index-on-create sites
  (tasks/agents/MRs) pass the entity's real update time. Tool description,
  input schema, and api-reference document the language accurately.
- **Coverage rows 11/12** moved to `implemented` with accurate anchors and
  an honest note on the PG leg's lack of a runtime harness.

## Notes (non-blocking)

- The PG leg has no runtime test in-repo (no PG server harness in this
  sandbox). The contract is pinned by mutation-verified SQL-shape tests and
  mirrors the SQLite leg; live-PG execution is a remote-verification item,
  not a code defect.
- `total` in the search response remains `results.len()` (capped by limit) —
  pre-existing §Response Format behavior tracked by row 14, out of scope here.
- Facet whitelist is a superset of the spec table (adds
  assigned/author/approval/scope as attribute keys); the spec's table lists
  examples of "filters by entity type and attributes", so this is an
  extension, not a contradiction.
