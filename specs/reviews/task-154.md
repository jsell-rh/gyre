# Review — task-154 (Implement search query language: quoted phrases, faceted filtering)

Spec: `specs/system/search.md` §Query Language (§Simple Search, §Quoted Phrases, §Faceted Filtering).
Candidate under review: `887f5cdc8f8422ab7a19bfd466b95a1ff046093e` (base `cfeefbda975211a6ac24a17e5920be6f8b23008b`).
Product commits: `c8f14874` (implementation), `40d1c1cd` (PG predicate restore), `fc0c24e8` (rustfmt repair); `d8929d58`/`8d1380f9c`/`bbe577e36`/`4f36182b4`/`887f5cdc8` (merge/docs/process).
Verdict: **approved** — no findings. Every acceptance criterion holds under independent probe, with mutation evidence that the new tests kill disabled behavior.

## Round 1 (2026-11-09) — independent review of `887f5cdc`

### Probes run (all at candidate HEAD; evidence in `/tmp/stage/review-evidence/`)

- `cargo test -p gyre-domain search_query` → **25/25 passed** (parser: terms, phrases, facets, malformed-input tolerance, relative/absolute time bounds, leap days, first-value-wins).
- `cargo test -p gyre-adapters --lib search` → **30/30 passed** (SQLite FTS5 + mem + PG SQL-shape suites).
- `cargo test -p gyre-adapters --lib mem_search` → **10/10 passed** (`query_language_phrases_and_facets`, `query_language_time_bounds`, `query_language_facet_counts_and_malformed_queries` all present and green).
- `cargo test -p gyre-adapters --lib postgres::search` → **4/4 passed** (`text_query_emits_tsv_match_predicate`, `simple_terms_query_emits_tsv_match_predicate`, `facet_only_query_has_no_tsv_predicate_but_real_filters`, `tsquery_parameter_is_bound_first`).
- `cargo check -p gyre-server --tests` → **clean** (5m53s; type-checks the lib, the MCP test module, and `tests/search_wiring.rs`).
- `bash scripts/check-arch.sh` → **passed** (parser lives in `gyre-domain`, no infra deps; the `gyre-ports` → `gyre-domain` re-export direction is pre-existing and unviolated).
- `python3 scripts/check-rustfmt-diff.py cfeefbda` → **changed lines clean (13 files)**.
- `python3 scripts/check-clippy-diff.py cfeefbda` → blocked by the sandbox build wrapper (workspace-wide clippy deferred to the verifier — infrastructure restriction, not a code defect; the prior verification round at `d8929d58` had clippy changed-lines clean, and the only commits since are whitespace-rustfmt, docs, and process records).
- `bash scripts/check-task-commit-attribution.sh` against the **committed** candidate frontmatter → **exit 0** (all three product SHAs recorded). Note: the script fails against the review checkout only because the harness resets `specs/tasks/task-154.md` to its pre-assignment state in the working tree; verified by re-running with the committed file checked out, then restoring the harness state byte-identically.
- **Mutation probes** (temporary edits to `sqlite/search.rs`, each restored and re-verified green afterwards; source saved/restored from `/tmp/stage/review-evidence/sqlite-search.rs.orig`, `git diff crates/` empty after each):
  1. Phrases matched as individual terms instead of an FTS5 phrase → `quoted_phrases_match_exactly_not_individual_terms` **FAILED** (kills the bug).
  2. Facet filters skipped (`std::iter::empty`) → `facet_filters_and_or_within_compose`, `facet_only_query_browses_without_text`, `facet_counts_honor_query_language_filters` **FAILED** (3 failures).
  3. Time bounds zeroed → `time_facets_since_and_until_relative_and_absolute` **FAILED**.
  Post-restore full `--lib search` suite → 30/30 passed again.

### What was verified (by reading the diff base..candidate plus the probes above)

- **Parser** (`gyre-domain/src/search_query.rs`, new): `parse_query` extracts quoted spans as phrases (unterminated quote runs to end; empty spans dropped), whitelisted `key:value` facets (all 12 spec keys; keys case-insensitive; first-colon split keeps `assigned:agent:worker-42` intact), and free-text terms split on non-alphanumeric boundaries mirroring the FTS `unicode61` tokenizer. Malformed facets (`type:` no value) are dropped; unknown `key:value` falls back to terms — forgiving, never errors. Facet keys are whitelisted before any storage path sees them. `time_bounds(now)` resolves relative (`7d`/`24h`/`2w`) and absolute (`YYYY-MM-DD`, leap-year-validated via dependency-free `days_from_civil`; since = start-of-day, until = end-of-day) values; unparseable values resolve to no bound.
- **Port** (`gyre-ports/src/search.rs`): `SearchQuery` carries `parsed: ParsedQuery` (built via `SearchQuery::parse`); `SearchDocument` gained `updated_at: Option<i64>`. Both `SearchQuery {` construction sites outside the port (REST handler, MCP handler) updated; all `SearchDocument` sites updated (tasks/agents/MRs index-on-create pass the entity's real update time).
- **SQLite FTS5 adapter**: MATCH expression is quoted terms + quoted phrases (safe by construction — parser guarantees terms are alphanumeric and phrases contain no `"`). Facet filters are real WHERE clauses: type/workspace/repo on index columns via `IN` lists, metadata facets via bound-parameter `json_extract` paths (key whitelisted, path bound — no interpolation of user text). `since:`/`until:` filter on the timestamp persisted under the reserved `UPDATED_AT_KEY` metadata key (sound workaround: FTS5 virtual tables cannot be ALTERed; the key is stripped from result facets and facet-count aggregations). Facet-only queries browse without a MATCH clause (constant rank, body-prefix snippet, ordered by timestamp). Filters apply before LIMIT. Numbered placeholders (`?N`) are emitted in ascending bind order matching diesel's sequential binding — proven by the passing multi-param facet tests against real SQLite.
- **PostgreSQL adapter**: terms + phrases flow through `websearch_to_tsquery` with the `s.tsv @@ q` row filter present in `build_filters` (the round-2 repair; shared by `search()` and `facet_counts()`, pinned by 4 SQL-shape regression tests). The tsquery parameter is reserved first (FROM clause) so bind order matches `$N` numbering. `updated_at` is a real BIGINT column with `ADD COLUMN IF NOT EXISTS` for pre-existing deployments (adapter init path, not a shared migration — the portability gate scope is the shared `migrations/` dir, correctly untouched). No runtime PG harness exists in-repo; this leg is shape-tested and recorded for remote verification — an honest, documented limit, not fakery.
- **Mem adapter**: same contract in code — exact case-insensitive substring phrases, AND-across/OR-within facet composition, time bounds; facet-only browse; empty query returns empty (parity with SQL adapters).
- **REST handler** (`api/search.rs`): `q` is parsed first; `type:`/`workspace:` facets in the query string take precedence over the `entity_type`/`workspace_id` params (which remain as fallback shortcuts). Empty `q` still short-circuits to an empty response.
- **MCP** (`mcp.rs`): `gyre_search` parses `q` with the same grammar and the same facet-over-arg precedence; tool description and input schema document the language. Handler remains read-only (no `needs_write` gate change needed). `mcp_search_query_language` covers phrase matching, facet narrowing, and precedence over the real router (compiles; runtime deferred to verifier per the 4 GiB budget).
- **`facet_counts(Some(q))`** in all three adapters aggregates the same text + facet + shortcut filtered match set (limit ignored), matching the coverage-row claim.
- **No test inflation found**: every new behavioral test drives real storage (SQLite FTS5 against a temp file, mem state) or the real router; the mutation probes above demonstrate the tests fail when the behavior is disabled. The PG shape tests assert generated SQL (documented as such given no PG harness) and the coverage row honestly flags the PG runtime gap as pending remote verification.
- **Docs**: `docs/api-reference.md` search row and `gyre_search` row updated; coverage rows 11/12 → `implemented` with accurate, non-overstated notes (including the PG-leg limit and the restored predicate).

### Deferred to the independent verifier (server test compilation exceeds the model-sandbox budget)

- `cargo test -p gyre-server --test search_wiring` (HTTP leg: phrases, facet narrowing, facet-only browse, facet-over-param precedence, since/until bounds through the real middleware stack against SQLite FTS5).
- `cargo test -p gyre-server --lib mcp_search` (MCP `gyre_search` query-language test).
- `bash /tmp/stage/checks.sh` full gate run (clippy changed-lines leg was wrapper-blocked in this review sandbox; the rustfmt leg passed here).
- Remote verification: PG search leg against a live PostgreSQL instance (no in-repo PG harness).

### Minor observations (non-blocking, not defects introduced by this candidate)

- `mem_search.rs` `make_snippet` indexes `text` with byte offsets computed against `text_lower` — a pre-existing pattern (identical in the base version) that can mis-slice when lowercasing changes byte length (e.g. `İ`). The rewrite preserved, not introduced, this; the mem adapter is dev/test wiring. Flagged for a future hardening pass, out of this task's scope.
