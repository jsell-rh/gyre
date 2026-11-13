# Review — task-154 (Search §Query Language: quoted phrases, faceted filtering)

Spec: `specs/system/search.md` §Query Language (Simple Search, Quoted Phrases,
Faceted Filtering) via task `specs/tasks/task-154.md`.
Candidate: `f44fcef38e2db82006aa24f4c5dfd268bf2fdfff` (base `a218b956`).
Branch history includes prior review/repair rounds (PG predicate restore
`40d1c1cde`, rustfmt `fc0c24e8`/`2c3eff661`, test initializers `5ace1b3ac`,
MCP tenant seed `1bb3c01bc`, search_wiring tenant seed `2acdab52`,
rebase onto task-203 access scoping `dbe510212`); all seven task-labeled
product commits are recorded in the task's `commits:` frontmatter.
Verdict: **approved**.

## Probes run (evidence in /tmp/stage/review-evidence)

All via the review-stage build wrapper (`/tmp/stage/bin/cargo`, serialized,
`-j1`); the review sandbox cannot build `gyre-server` test binaries (wrapper
rejects gyre-server codegen — 4 GiB budget), so server tests are deferred to
the verifier (list at the bottom).

- `cargo test -p gyre-domain search_query` → **25 passed** (parser: terms,
  phrases incl. unterminated/empty spans, facet extraction, OR-within-key,
  malformed-facet forgiveness, unknown-key fallthrough to terms,
  non-alphanumeric term splitting, case-insensitive facet keys, facet-filter
  key mapping, relative/absolute/invalid/leap-day time bounds, first-wins).
- `cargo test -p gyre-adapters --lib -- sqlite::search:: mem_search::` →
  **29 passed** (all search-module tests, incl. the four new SQLite
  query-language tests and three mem query-language tests; no regression in
  the pre-existing scoping/suggest/facet-counts suites).
- `cargo test -p gyre-adapters --lib -- postgres::search` → **6 passed**
  (SQL-shape regression tests guarding the `s.tsv @@ q` full-text predicate,
  facet-only shape, `$1` FROM-vs-WHERE bind ordering).
- `cargo check -p gyre-server` → clean (handler, MCP, indexing call sites
  type-check; every `SearchQuery`/`SearchDocument` construction site updated).
- `cargo fmt -p gyre-domain -p gyre-ports -p gyre-adapters -p gyre-server
  -- --check` → clean.
- `scripts/check-arch.sh`, `scripts/check-mem-port-contracts.sh` → pass
  (parser lives in gyre-domain with no infra deps; port contract parity).

## Mutation verification (each mutated file restored byte-identical after;
`git status` clean, only the harness's task-154.md reset differs)

1. **Phrase adjacency broken** (SQLite `build_match_expr` flattened phrase
   words into independent quoted terms):
   `quoted_phrases_match_exactly_not_individual_terms` **FAILED**
   (`["adjacent","scattered"]` vs `["adjacent"]`) — the test kills the exact
   class of bug that would make "quoted phrases" mean plain AND-of-terms.
2. **Facet filters disabled** (`for filter in Vec::<FacetFilter>::new()` in
   `build_filters`): `facet_filters_and_or_within_compose` and
   `facet_only_query_browses_without_text` **both FAILED**.
3. **`since:` inverted** (`{TS_EXPR} >= {p}` → `<=`):
   `time_facets_since_and_until_relative_and_absolute` **FAILED**.

## Findings

None blocking. The implementation is real across the full stack:

- **Parser** (`gyre-domain/src/search_query.rs`): forgiving by contract —
  malformed facets (`type:`) dropped, unknown `key:` tokens fall through to
  plain terms, facet keys whitelisted to the spec's 12 (type, workspace,
  repo, status, priority, assigned, author, label, approval, scope, since,
  until), so no unvetted key reaches storage. Time facets resolve relative
  (`7d`/`24h`/`2w` via checked arithmetic) and absolute `YYYY-MM-DD`
  (validated calendar incl. leap years; `until:` = end-of-day 23:59:59,
  `since:` = start-of-day), unparseable values ignored.
- **SQLite adapter**: FTS5 `MATCH` with each term and each phrase as a quoted
  FTS5 phrase (adjacency enforced by FTS5; quoting keeps user text inert as
  syntax — the parser guarantees terms are single alphanumeric tokens and
  phrases contain no `"`). Facet filters are real WHERE clauses (IN-lists on
  entity_type/workspace_id/repo_id columns, `json_extract` on metadata with a
  bound path parameter), AND-composed, OR-within-key via IN. `since:`/`until:`
  filter on a timestamp persisted under the reserved `updated_at` metadata
  key (FTS5 virtual tables cannot be ALTERed — reasonable storage choice;
  the key is stripped from result facets and facet-count aggregations, proven
  by `facet_counts_honor_query_language_filters`). Facet-only queries browse
  without a MATCH clause; empty queries return nothing, never everything.
- **PostgreSQL adapter**: `websearch_to_tsquery` with terms + quoted phrases,
  and — the prior round's drop — the `s.tsv @@ q` row filter restored in the
  shared `build_filters` used by both `search()` and `facet_counts()`, guarded
  by SQL-shape regression tests (no PG harness in-repo; runtime verification
  against live PG is a deferred verifier item, as the coverage note records).
- **mem adapter**: same contract via `scope_visible` + `score_doc`
  (exact case-insensitive substring for phrases), parity with the SQL
  adapters for facet-only and malformed queries.
- **REST handler** (`api/search.rs`): `q` parsed first; query-string
  `type:`/`workspace:` facets take precedence over the legacy
  `entity_type`/`workspace_id` params (which remain as narrowing shortcuts);
  scope still derived from the authenticated caller (task-203), never from
  client params. **MCP `gyre_search`**: same grammar, same precedence, tool
  description documents the query language; read-only, so no `needs_write`
  gate change.
- **Indexing call sites** (tasks/agents/MRs) stamp `updated_at` from the
  entity's own update time; adapters default to index time otherwise.

The durable finding (verification-round failure `search_wiring.rs:443`:
`count quokkas` returned 1 of 2) was repaired by `2acdab522` seeding the
caller's tenant on the directly-indexed docs — a legitimate test-seed fix,
not assertion weakening: the unquoted query still expects both results, and
the invisibility of tenant-unresolvable docs is correct fail-closed
§Access Scoping behavior from task-203 (the same class of fix as the MCP
round `1bb3c01bc`).

Minor, non-blocking observations:

- `crates/gyre-ports/src/search.rs` `facet_counts` trait doc has the
  "Aggregate facet counts over the index (spec §API `GET /search/facets`)"
  line duplicated (cosmetic doc duplication only).
- PG runtime behavior is guarded by SQL-shape tests only (no PG server
  harness in-repo); the coverage row records this honestly.

## Deferred verifier checks (server builds blocked in the review sandbox)

- `cargo test -p gyre-server --test search_wiring` — in particular
  `sqlite_url_wires_fts5_search_port`, which now carries the full HTTP query
  -language leg (phrase adjacency via `q=%22count%20quokkas%22`, unquoted
  both-match, `status:` narrowing + facet_counts, non-matching facet → 0,
  facet-only browse, `type:` facet overriding the `entity_type` param,
  `since:7d` exclusion, absolute since/until) and the durability assertions.
- `cargo test -p gyre-server --lib mcp::tests::mcp_search_query_language`
  (MCP phrase + facet + facet-over-arg precedence) and the surrounding
  `mcp::*search*` tests.
- Full workspace suite, clippy, and the pre-commit check scripts (CI parity).
- Optional: live-PostgreSQL run of the query-language paths (the in-repo PG
  coverage is SQL-shape only).
