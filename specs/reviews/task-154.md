# Review — task-154 (Implement search query language: quoted phrases, faceted filtering)

Spec: `specs/system/search.md` §Query Language (§Simple Search, §Quoted Phrases, §Faceted Filtering).
Candidate: `6393fe0b8c4c672f0541ebe14af5c22a3a5d9ae0` (base `3b734b1a`).
Product commits: `c8f1487` (parser + adapters + handler + MCP), `40d1c1c` (PG `s.tsv @@ q` predicate repair), `5ace1b3`/`2c3eff6`/`1bb3c01` (merge/rustfmt/test-seeding repairs); remainder docs/process.
Verdict: **approved** — no blocking findings.

## What shipped

- **Parser** (`crates/gyre-domain/src/search_query.rs`, pure logic, no infra deps — arch lint passes): `parse_query` extracts free-text terms (split on non-alphanumeric boundaries, mirroring FTS `unicode61`), `"quoted phrases"` (unterminated quote runs to end; empty spans dropped), and whitelisted `key:value` facets (the 12 spec keys incl. `assigned:`/`author:`/`approval:`/`scope:`). Malformed facets (`type:` no value) are dropped; unknown `key:value` falls back to terms. `since:`/`until:` resolve relative (`7d`/`24h`/`2w`) and absolute (`YYYY-MM-DD`, since=start-of-day UTC, until=end-of-day, leap-year-validating via `days_from_civil`) to epoch bounds; first value wins (OR of instants is meaningless — documented).
- **Port** (`crates/gyre-ports/src/search.rs`): `SearchQuery` carries `parsed: ParsedQuery` alongside the raw echo string; `SearchQuery::parse` + `with_scope` builders keep the two consistent and start access-scope fail-closed. `gyre-ports → gyre-domain` dependency pre-existed at base (no new hexagonal edge). All values reach SQL as bound parameters; facet keys are whitelist-only, so no user text is interpolated into statement shape.
- **All three adapters** execute the language: SQLite FTS5 (terms+phrases as quoted FTS5 phrases in `MATCH`, facet filters as `WHERE` clauses — `type:`/`workspace:`/`repo:` on index columns, others via bound `json_extract` paths, `since:`/`until:` on a reserved `updated_at` metadata key since FTS5 tables cannot be ALTERed, stripped from results/counts), PostgreSQL (`websearch_to_tsquery` reconstruction of terms+quoted phrases **with the `s.tsv @@ q` row filter restored** — the prior-round defect — plus `updated_at` column with `ADD COLUMN IF NOT EXISTS` migration for pre-existing tables, facets via `metadata->>`), and mem (exact-substring phrases, facet predicates in `scope_visible`, timestamps stamped at index). AND-across-keys / OR-within-key via `IN` lists; facet-only queries browse without a MATCH clause; empty/malformed queries never mean "everything". `entity_type`/`workspace_id` params remain fallback shortcuts with query-string facets taking precedence — enforced in both the REST handler and MCP `handle_search`.
- **REST + MCP integration**: `GET /api/v1/search?q=` parses first; `gyre_search` MCP tool parses `q` with the same precedence rule; docs (`api-reference.md`, tool description) updated. Indexers (tasks/MRs/agents create paths) now stamp real entity `updated_at`.
- **Coverage rows 11/12** flipped to `implemented` with accurate, evidence-citing notes (including the honest "PG leg has no runtime test in-repo" caveat).

## Probes run (evidence in `/tmp/stage/review-evidence/`)

- `cargo test -p gyre-domain search_query` → **25/25 passed** (all parser patterns from the plan: simple terms, phrase extraction, facets, mixed, multi-value OR, malformed, case-insensitive keys, time bounds relative/absolute/leap-day/invalid/first-wins).
- `cargo test -p gyre-adapters --lib mem_search` → **11/11 passed** (incl. `query_language_phrases_and_facets`, `query_language_time_bounds`, `query_language_facet_counts_and_malformed_queries`).
- `cargo test -p gyre-adapters --lib sqlite::search` → **18/18 passed** (incl. `quoted_phrases_match_exactly_not_individual_terms`, `facet_filters_and_or_within_compose`, `time_facets_since_and_until_relative_and_absolute`, `facet_only_query_browses_without_text`, `facet_counts_honor_query_language_filters`).
- `cargo test -p gyre-adapters --lib postgres::search` → **6/6 passed** (SQL-shape guards: `text_query_emits_tsv_match_predicate`, `simple_terms_query_emits_tsv_match_predicate`, `facet_only_query_has_no_tsv_predicate_but_real_filters`, `tsquery_parameter_is_bound_first`, scope alias/contiguity, empty-set→FALSE).
- **Mutation probe (discriminating-test check)**: mutated `build_match_expr` in the SQLite adapter to treat quoted phrases as individual AND terms → `quoted_phrases_match_exactly_not_individual_terms` **FAILED** (exit 101), proving the phrase test kills real phrase-semantics breakage. Source restored (`git diff` clean afterwards).
- `cargo check -p gyre-server --tests` → **clean** (handler, MCP, and `tests/search_wiring.rs` compile against the new port shape).
- `bash scripts/check-arch.sh`, `check-byte-slice-truncation.sh`, `check-mem-port-contracts.sh` → all OK.

## Verifier must run (blocked in this sandbox by the 4 GiB wrapper on gyre-server test compilation)

- `cargo test -p gyre-server --test search_wiring` — HTTP-leg tests: phrase adjacency over the real middleware stack (`q=%22count%20quokkas%22`), facet narrowing + facet_counts over HTTP, `type:` facet overriding the `entity_type` param, `since:7d`/absolute bounds, facet-only browse.
- `cargo test -p gyre-server --lib mcp::tests::mcp_search_query_language` — MCP tool: quoted phrase via `tools/call`, facet filter, facet-over-arg precedence.
- Remote PostgreSQL run of the PG adapter (no in-repo PG harness): text queries must restrict via `s.tsv @@ q` (the SQL-shape tests guard the statement shape; a live parse/run check confirms it against a real PG).

## Test-inflation check

None found. The phrase tests use adjacent-vs-scattered document pairs where the scattered body contains both words (a term-semantics implementation fails); facet tests assert exact result-id sets including the OR-within/AND-across composition and the matches-nothing case; time-bound tests pin a 40-day-old doc against a fresh one and check both relative and absolute forms in both directions; the HTTP tests assert exact titles/totals. The one mutation probe run confirmed a representative test is discriminating.

## Minor (non-blocking, recorded for completeness)

- `crates/gyre-ports/src/search.rs:182-183` — the `facet_counts` trait doc-comment line "Aggregate facet counts over the index (spec §API `GET /search/facets`" is duplicated (an artifact of the diff). Cosmetic only.
- `mem_search.rs` `make_snippet` finds the needle in a lowercased copy but slices byte offsets out of the original text — latent non-char-boundary panic for inputs whose lowercasing changes byte length (e.g. `İ`). Pre-existing shape at base, preserved through the refactor, not introduced by this task.
- The uncommitted working-tree revert of `specs/tasks/task-154.md` frontmatter (`progress: not-started`, `commits: []`, Shipped section removed) was present at review start (executor checkpoint state), not a candidate-tree property; the candidate commit itself records the six product commits and shipped notes.
