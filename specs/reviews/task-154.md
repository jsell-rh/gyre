# Review — task-154 (Implement search query language: quoted phrases, faceted filtering)

Spec: `specs/system/search.md` §Query Language (§Simple Search, §Quoted Phrases, §Faceted Filtering).
Candidate under review: `10cc51adf174d71366f68eabea062711426ee2ad` (product commit `c8f14874`, docs `420339337`, process `10cc51adf`), base `cd4c04fe`.
Verdict: **needs-revision** — one material code defect (F1: PostgreSQL adapter lost the full-text match predicate); the SQLite/mem/REST/MCP legs are genuinely implemented and tested.

## Probes run

Evidence saved under `/tmp/stage/review-evidence/` (summary: `evidence-summary.md`; PG probe: `pg-sql-shape-probe.txt`).

- `cargo test -p gyre-domain --lib search_query` → **25 passed** (parser: terms, phrases, facets, OR-within-facet, malformed input, time bounds relative/absolute/leap/invalid).
- `cargo test -p gyre-adapters --lib sqlite::search` → **16 passed** (phrase adjacency vs scattered words, facet AND/OR composition, time facets, facet-only browse, facet_counts honoring filters, plus all pre-existing search tests).
- `cargo test -p gyre-adapters --lib mem_search` → **10 passed** (same contract mirrored in code).
- `cargo test -p gyre-server --test search_wiring` → **1 passed** (phrases, facet narrowing, facet-only browse, facet-over-param precedence, since/until — through the real middleware router against the SQLite FTS5 backend).
- `cargo test -p gyre-server --lib mcp_search` → **1 passed** (`mcp_search_query_language` over MCP with mem-backed state).
- PG SQL-shape probe (temporary in-crate test calling the candidate's own `build_filters`, file restored bit-identical, sha256 verified) → captured the exact production SQL; see F1.
- `bash scripts/check-arch.sh`, `bash scripts/check-task-commit-attribution.sh` → OK.

## Verified working (no findings)

- **Parser is real and forgiving.** `gyre-domain/src/search_query.rs` extracts terms (split on non-alphanumeric boundaries, matching the FTS `unicode61` tokenizer), quoted phrases (unterminated quote runs to end; empty spans dropped), and whitelisted `key:value` facets — the whitelist is exactly the spec's facet table (type/workspace/repo/status/priority/assigned/author/label/approval/scope/since/until). Malformed facets (`type:` with no value) are dropped, unknown keys fall back to terms; nothing errors. `since:`/`until:` resolve relative (`7d`/`24h`/`2w`) and absolute (`YYYY-MM-DD`, leap-aware, since=start-of-day, until=end-of-day) to epoch seconds via a dependency-free `days_from_civil`.
- **SQLite FTS5 leg enforces the grammar in the index query.** MATCH expression is quoted terms + phrases (safe against FTS5 syntax injection — parser guarantees terms are alphanumeric and phrases contain no `"`); facet filters are real bound-parameter WHERE clauses (`IN` lists on entity_type/workspace_id/repo_id columns, `json_extract` paths on metadata), AND across keys / OR within a key; `since:`/`until:` filter on the timestamp persisted under the reserved `UPDATED_AT_KEY` (FTS5 virtual tables cannot be ALTERed — the workaround is legitimate and the key is stripped from result facets and facet-count aggregations). Filters apply before LIMIT; facet-only queries browse without a MATCH clause. Tests prove adjacency (phrase matches only the adjacent-token doc), facet composition, and time exclusion.
- **Mem adapter mirrors the contract in code** (phrase = exact case-insensitive substring, AND/OR facet composition, time bounds, facet-only browse), with parity tests.
- **REST handler and MCP tool integrate the parser** with the documented precedence (a `type:`/`workspace:` facet in `q` overrides the `entity_type`/`workspace_id` params; params remain fallback shortcuts). The `search_wiring` test drives phrases, facets, precedence and time bounds through the full middleware stack; the MCP test drives the same grammar over `tools/call`. Index-on-create sites (tasks/agents/MRs) pass the entity's real epoch-second update time.
- **Port layering is sound**: the query-language types live in `gyre-domain` and are re-exported through `gyre-ports` (the ports→domain dependency direction pre-exists at base; arch check passes). `SearchQuery::parse` keeps raw and parsed forms consistent; existing constructors migrated without behavior change (all pre-existing search tests pass unchanged in intent).
- **Docs/coverage updated** (api-reference rows for `/api/v1/search` and `gyre_search` document the language and precedence).

## Findings

### F1 (code, material) — PostgreSQL adapter dropped `s.tsv @@ q`; text queries do not filter rows on PG

`crates/gyre-adapters/src/postgres/search.rs`, `build_filters()`. When the rewrite replaced the old fixed statements (`WHERE s.tsv @@ q AND …` in both `search()` and `facet_counts()` at base cd4c04fe, lines 236/343) with the dynamic `PgWhereBuilder`, the tsv match predicate was never emitted. The tsquery is reserved first (`FROM search_index s, websearch_to_tsquery('english', $1::text) q`) but is used only for `ts_rank`/`ts_headline` in the SELECT — a cross join does not filter rows.

Probe (no PG server exists in this sandbox, so the production SQL itself was captured via a temporary in-crate test of the candidate's own `build_filters`, restored afterwards):

```
query: jwt "merge queue" type:task
SQL=FROM search_index s, websearch_to_tsquery('english', $1::text) q WHERE s.entity_type IN ($2::text)
```

`grep '@@' crates/gyre-adapters/src/postgres/search.rs` at the candidate → zero matches. Consequences on the PG backend: every text query returns all rows passing only the facet filters (§Simple Search AND-of-terms and §Quoted Phrases are not enforced), and `facet_counts(Some(q))` with text aggregates the whole index rather than the text-filtered match set. This is a regression from the base, where both statements filtered on `s.tsv @@ q`.

Fix: in `build_filters()`, push `s.tsv @@ q` as a WHERE clause when `has_text` (mirroring the SQLite leg's `search_index MATCH ?`), covering both `search()` and `facet_counts()`.

### F2 (docs) — coverage/task claims overstate the PG leg

`specs/coverage/system/search.md` rows 11/12 and the task's Shipped notes claim the PG leg passes terms and quoted phrases through `websearch_to_tsquery` with phrase semantics. Per F1 the tsquery never filters rows, so the claim is not true of the shipped code. The rows were marked `implemented` partly on the strength of a backend whose text filtering regressed; after F1 is fixed the PG leg still has no runtime verification in this repo (no PG harness) and should be flagged for remote verification rather than asserted.

## Notes for the verifier

- SQLite (default backend), mem, REST and MCP legs are genuinely done; only the PG leg blocks approval.
- The PG defect is mechanically checkable without a live PG: after the fix, `grep '@@' crates/gyre-adapters/src/postgres/search.rs` must show the tsv predicate inside the WHERE construction, and a live PG probe should confirm a text query returns only matching rows (e.g. index two docs, search a term present in one, assert the other is absent — the current candidate returns both).
