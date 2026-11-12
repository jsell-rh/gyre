# Review — task-154 (Implement search query language: quoted phrases, faceted filtering)

Spec: `specs/system/search.md` §Query Language (Simple Search, Quoted Phrases, Faceted Filtering).
Comparison base: `e981bf29cbd9e03ba44c6c6708dc11843d9776ec` → candidate `1dec619fa0272ff954f33b472e982a8f7159238f`.
Diff: 16 files — domain parser (`gyre-domain/src/search_query.rs`, new, 551 lines), `SearchQuery`/`SearchDocument` protocol changes (gyre-ports), all three SearchPort adapters, REST search handler, MCP `gyre_search`, indexing call sites (tasks/agents/MRs), `tests/search_wiring.rs`, docs, coverage matrix, task file.

Verdict: **needs-revision** (not approved). Evidence under `/tmp/stage/review-evidence/` (probe-log.txt, server-lib-tests-compile-failure.txt, sqlite-search.rs.orig probe backup).

## What is real and good

The implementation itself is a genuine, production-quality query language, not fakery:

- **Parser** (`gyre-domain/src/search_query.rs`): pure logic, no infra deps (hexagonal boundary holds — `check-arch.sh` layering respected by construction). Facet keys are a fixed whitelist (`FACET_KEYS`) so no unvetted key reaches a storage backend; malformed facets (`type:` with no value) are dropped per the task's forgiving-contract instruction; unknown `key:value` falls back to plain terms; terms are split on non-alphanumeric boundaries mirroring the FTS `unicode61` tokenizer. `since:`/`until:` resolve relative (`7d`/`24h`/`2w`) and absolute (`YYYY-MM-DD`, validated incl. leap years, Hinnant `days_from_civil`) to epoch bounds.
- **SQLite adapter**: FTS5 `MATCH` with every term and phrase individually double-quoted — user input can never reach the FTS5 parser as syntax (the hostile-query test proves it). Facets compile to bound-parameter WHERE clauses (`IN` lists = OR-within-facet, AND across facets) evaluated inside the index query; `since:`/`until:` filter via `json_extract` on a reserved metadata key (FTS5 virtual tables cannot be ALTERed — sound workaround, and the key is stripped from result facets and aggregations). Facet-only queries browse without a MATCH clause.
- **PG adapter**: `websearch_to_tsquery` with reconstructed quoted phrases; the `s.tsv @@ q` row filter IS present in `build_filters` (shared by `search()` and `facet_counts()`), with SQL-shape regression tests pinning it — the exact defect an earlier review round found is now guarded. Placeholder ordering ($1 = tsquery in FROM) is tested.
- **Mem adapter**: parity contract (phrases as exact substrings, facet AND/OR, time bounds) — `check-mem-port-contracts` concern addressed.
- **REST + MCP handlers**: both parse `q` first; `entity_type`/`workspace_id` params/args are fallback shortcuts that query-string facets override (tested both at HTTP and MCP level); access scope still derives from auth (task-203 semantics preserved through the rebase).
- **Tests are hard, mutation-verified this round**: I ran three independent mutation probes (phrases degraded to individual AND terms; facet-filter loop emptied; time bounds defaulted) — each was killed by its respective adapter test. Parser: 25 passed. Adapters (`--lib search`): 35 passed across sqlite/mem/pg suites.

## Blocking finding (why this is not approved)

**`cargo test -p gyre-server --lib` does not compile at the candidate** — 3× E0063 in `crates/gyre-server/src/mcp.rs` (log: `review-evidence/server-lib-tests-compile-failure.txt`):

1. `mcp.rs:3842` — the pre-existing task-203 test `mcp_search_is_access_scoped_to_the_agent_token` builds `SearchDocument` without `updated_at`. The candidate added that field to `SearchDocument` (gyre-ports/src/search.rs:40) and updated the other three production initializers (tasks/agents/MRs) plus `search_wiring.rs`, but missed this test initializer.
2. `mcp.rs:3997`, `mcp.rs:4013` — the candidate's own NEW test `mcp_search_query_language` omits the base's `tenant_id` field in both `SearchDocument` literals. The MCP acceptance-criterion test has therefore never compiled, never run.

Consequences: CI's `cargo test --all` fails at compile; the entire gyre-server unit-test target (not just search tests) is blocked; the "MCP `gyre_search` also supports the query language" acceptance criterion currently rests on a test that cannot execute. The shipped notes' claim of test evidence covered `gyre-adapters`/`gyre-domain`/`cargo check --test search_wiring` — all of which do pass — but the lib-test compile was evidently never run.

Fix is mechanical (add `updated_at: None` at :3842; `tenant_id: None` at :3997/:4013) but must be followed by the verifier running `cargo test -p gyre-server --lib` and `cargo test -p gyre-server --test search_wiring`.

## Non-blocking observations (recorded, not grounds for rejection)

- `gyre-ports/src/search.rs` trait docs: the `facet_counts` doc comment line "Aggregate facet counts over the index (spec §API `GET /search/facets`" is duplicated (rebase artifact, cosmetic).
- PG runtime leg still has no in-repo harness; SQL-shape tests + remote verification remain the evidence path (standing caveat from prior rounds, unchanged).
- Coverage matrix rows 11/12 (Quoted Phrases, Faceted Filtering → `implemented`) accurately describe the shipped source; the MCP row 15 claim is contingent on the compile fix above.

## Verifier must run

- `cargo test -p gyre-server --lib` — currently fails to compile (the blocking finding).
- `cargo test -p gyre-server --test search_wiring` — HTTP leg (phrase adjacency, facet narrowing, facet-vs-param precedence, `since:` bounds, access scoping).
- `cargo test -p gyre-adapters --lib` and `cargo test -p gyre-domain --lib` — pass at candidate (35 + 499 incl. parser 25).
