# Review — task-154 (Search query language: quoted phrases, faceted filtering)

Spec: `specs/system/search.md` §Query Language (§Simple Search, §Quoted Phrases, §Faceted Filtering).
Base: `cba416b4` — Candidate: `236f8d71` (product commits `c8f14874` feat, `40d1c1cd` PG predicate repair, `fc0c24e8` rustfmt repair; plus docs/process commits and CI linker repair `001df941`).
Verdict: **approved** — all seven acceptance criteria met with real production code and discriminating tests; no findings.

## Round 1

Probes run (checkout at `236f8d71`, `/tmp/stage/bin/cargo`):

- `cargo test -p gyre-domain search_query` → **25 passed, 0 failed** (evidence: `review-evidence/domain-search-query-test.log`).
- `cargo test -p gyre-adapters -- sqlite::search mem_search postgres::search` → **30 passed, 0 failed** (evidence: `review-evidence/adapters-search-tests.log`) — real SQLite FTS5 runtime tests, mem contract tests, PG SQL-shape tests.
- `cargo check -p gyre-server`, `cargo check -p gyre-cli`, `cargo check -p gyre-server --test search_wiring` → all clean (evidence: `review-evidence/server-cargo-check.log`, `server-search-wiring-check.log`).
- `bash scripts/check-arch.sh`, `check-mem-port-contracts.sh`, `check-byte-slice-truncation.sh`, `check-inert-enforcement.sh` → all OK.
- Independent FTS5 probes via sqlite3 CLI (evidence: `review-evidence/fts5-phrase-probe.sh/.log`): the adapter's MATCH construction for `"merge queue" ordering` (`"merge queue" "ordering"`) returns 1 against adjacent+scattered docs, while degraded term matching returns 2 — the phrase tests are genuinely discriminating. Pure-punctuation quoted phrases (`"( )"`, `"!!!"`, `""`) do not raise FTS5 syntax errors — the parser's forgiving contract holds end-to-end.
- `check-task-commit-attribution.sh` FAILs against the **working tree** only because the review stage reset `specs/tasks/task-154.md` frontmatter (`commits: []`); the candidate commit itself records all three product-surface SHAs. Not a candidate defect.

## Verified working (no findings)

- **Parser** (`gyre-domain/src/search_query.rs`, new): extracts terms (split on non-alphanumeric boundaries, mirroring the unicode61 tokenizer), quoted phrases (unterminated quote runs to end; empty spans dropped), and facets. Facet keys whitelisted to the canonical spec's full 12-key table (type/workspace/repo/status/priority/assigned/author/label/approval/scope/since/until — a superset of the task excerpt's 8); malformed facets (`type:` no value) dropped, unknown `key:value` falls back to terms; keys case-insensitive; `assigned:agent:worker-42` splits at the first colon only. Time facets: relative (`7d`/`24h`/`2w`) and absolute (`YYYY-MM-DD`, leap-year-validated, since=start-of-day, until=end-of-day) resolved via a dependency-free `days_from_civil`; invalid values ignored (no bound). Forgiving contract holds — no input errors.
- **Port** (`gyre-ports/src/search.rs`): `SearchQuery` carries `parsed` alongside raw `query` (built via `SearchQuery::parse` so they stay consistent); `SearchDocument.updated_at` added for time facets. All struct-literal construction sites (REST handler, MCP) updated; `gyre-cli` unaffected.
- **SQLite adapter**: MATCH expression quotes every term and phrase — injection-safe by construction (terms are alphanumeric-only, phrases cannot contain `"`), and quoting prevents user input from reaching FTS5 as boolean/query syntax. Facet filters are real bound-parameter WHERE clauses (IN lists = OR-within-facet, AND across), applied before LIMIT; `since:`/`until:` filter on the timestamp persisted under the reserved `updated_at` metadata key (FTS5 tables cannot be ALTERed — sound workaround; the key is stripped from result facets and facet-count aggregations, with a test proving no leak). Discriminating tests: the scattered-words doc must NOT match a phrase query; facet id-set assertions are exact; a 40-day-old doc is excluded by `since:7d`.
- **PG adapter**: `websearch_to_tsquery` (implicit AND, quoted phrases, never raises syntax errors) with the **`s.tsv @@ q` row filter present in `build_filters`** — the predicate a prior review round found dropped was restored by `40d1c1cd` and is guarded by four SQL-shape regression tests (`text_query_emits_tsv_match_predicate` etc.) that assert the predicate lands in the WHERE clause and that the tsquery parameter is bound first (FROM references `$1`). `updated_at` column with `ADD COLUMN IF NOT EXISTS` for pre-existing tables; metadata facets via `metadata->>$n::text` with bound key from the parser whitelist.
- **Mem adapter**: same query-language contract in code (exact case-insensitive substring phrases, AND/OR facet composition, time bounds on stamped index time); contract tests mirror the SQL adapters.
- **REST handler** (`api/search.rs`) and **MCP `gyre_search`** (`mcp.rs`): both parse `q` first and pass the structured query to the port; `entity_type`/`workspace_id` params/args remain as fallback shortcuts with query-string facets taking precedence (implemented, tested at both surfaces — the wiring test asserts `type:task` overrides `entity_type=mr`). MCP tool description documents the language. Create-path indexers (tasks/agents/MRs) now stamp real entity update times and index real status/priority facet values, so the facets filter real data.
- **No invariant violations**: hexagonal boundary clean (parser is pure domain logic); no new routes (ABAC registry untouched); no new migrations (dialect-specific DDL deliberately kept on backend-specific init paths, respecting `check-migration-sql-portability`); all facet values are bound SQL parameters, JSON paths built only from whitelisted keys.
- **CI repair** (`001df941` + `setup-linker` action): targeted response to the recorded e2e infrastructure failure (run 43036531271, mold apt fetch DNS failure). Retry with backoff, then `RUSTFLAGS=-C link-arg=-fuse-ld=lld` fallback to the runner's preinstalled lld — the `linker = "clang"` table entry is not part of rustflags so it survives; checks still run, only the link step differs. Conservative and sound.
- **Coverage rows 11/12** updated to `implemented` with accurate, non-overstated notes (including the honest "PG leg has no runtime test in-repo" caveat); API reference updated for both REST and MCP surfaces.

Test-inflation check: none found. The phrase tests have a both-words-non-adjacent doc that must not match; facet tests assert exact id sets that break under wrong AND/OR composition; time tests exclude an old doc; the PG shape tests fail if the `@@` predicate is dropped from the generated statement.

## Verifier must run (deferred from review per build-budget policy)

- `cargo test -p gyre-server --test search_wiring` — HTTP leg over real SQLite FTS5 (phrases, facets, precedence, time bounds, facet_counts).
- `cargo test -p gyre-server --lib mcp_search` — MCP query-language test (mem-backed state).
- Full `cargo test --all`, `cargo clippy --all-targets`, `cargo fmt --check`.
- Remote verification: PG leg (`postgres/search.rs`) against a live PostgreSQL — no in-repo PG harness; runtime behavior of `websearch_to_tsquery` + `s.tsv @@ q` + JSONB facets is guarded only by SQL-shape tests (flagged in coverage row 11).

Minor (non-blocking, recorded for completeness):

- Relative `until:7d` means "until 7 days ago" (`now - offset`) — a defensible reading of relative time bounds for `until`, documented in `time_bounds` docs.
- A negative relative value (`since:-7d`) yields a future bound and thus empty results — garbage-in, empty-out, no error; harmless.
- Response `total` is the truncated page length, not the full match count — pre-existing Response Format partial already tracked in coverage row 14, out of task-154 scope.
