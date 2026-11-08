# Review: TASK-153 — Search Autocomplete and Facets Endpoints

**Reviewer:** Independent review (Gyre pipeline)
**Date:** 2026-11-08
**Comparison base:** c5deca801a331e049f35b181f42d898df24e7e2b
**Candidate:** 12a36efd959dad85215964254962473e99c51748
**Verdict:** approved

---

## Scope inspected

`git diff c5deca80..12a36efd9` — 12 files: `gyre-ports/src/search.rs` (+26),
`gyre-server/src/api/search.rs` (+86), `api/mod.rs`, `abac_middleware.rs`,
all three adapter search modules (sqlite +339, postgres +153, mem +217),
`tests/search_wiring.rs` (+111), docs and coverage/task bookkeeping. No
production code outside task scope was touched.

## Acceptance criteria — all verified on the exact candidate tree

- **`GET /api/v1/search/suggest?q={prefix}`** — real handler
  (`suggest_handler`, api/search.rs) over a new `SearchPort::suggest` method
  implemented in all three adapters: SQLite FTS5 `title LIKE 'prefix%' ESCAPE
  '\'` with `%`/`_`/`\` escaped so user input matches literally, PG
  `ILIKE ... ESCAPE`, mem lowercased `starts_with`. Trimmed-empty prefix
  returns `[]` at both handler and port level (never "every title"). Limit
  defaults to 10, capped at 50.
- **`GET /api/v1/search/facets`** — `facet_counts(None)` whole-index
  aggregation in all three adapters: built-in `type` (entity_type) and
  `workspace` (non-empty workspace_id) facets plus every document facet
  key/value from the index metadata JSON.
- **`facet_counts` on `GET /api/v1/search`** — the handler runs a second
  aggregation with the same terms + `entity_type`/`workspace_id` filters and
  the result-page `limit` ignored, so counts describe the filtered match set,
  not the truncated page and not the whole index. The SQLite/PG aggregation
  WHERE clauses are byte-for-byte the `search()` clauses minus LIMIT; mem
  mirrors `search()`'s `score_doc` term matching and filters.
- **SearchPort trait** — `suggest` and `facet_counts` added with no default
  impl; all three implementors (`SqliteStorage`, `PgStorage`,
  `MemSearchAdapter`) provide them; nothing else implements the trait.
- **Routes** registered in `api/mod.rs:615-617` with ABAC resolver entries
  (`abac_middleware.rs:372-374`, resource `search`) — both sit behind the
  standard auth middleware; 401 legs asserted in the wiring test.

## Independent probe evidence (/tmp/stage/review-evidence/)

- `cargo test -p gyre-adapters search` — **18 passed, 0 failed**
  (adapters-search-tests.txt), including the new SQLite
  `suggest_prefix_matches_titles_case_insensitively`,
  `suggest_escapes_like_wildcards_in_prefix`,
  `facet_counts_whole_index_and_filtered_match_set` and the mirrored mem
  contract tests.
- `cargo test -p gyre-server --test search_wiring` — **1 passed**
  (server-search-wiring.txt): full-middleware-router legs for suggest
  (prefix match, empty prefix, non-matching prefix), 401 without token on
  both new endpoints, `facet_counts` on the `/search` response over the
  filtered match set, and whole-index `/facets` counts.
- **Mutation probe** (mutation-probe.txt): temporarily neutering
  `SqliteStorage::suggest` and `facet_counts` (env-gated early return) makes
  exactly the three new adapter tests FAIL (3 failed / 8 passed) — the tests
  are load-bearing, not self-confirming. Mutation reverted; working tree
  restored byte-identical to the candidate (`git status` clean, verified).
- Mechanical gates on the candidate: `check-rustfmt-diff.py` (9 files clean),
  `check-arch.sh`, `check-abac-route-registry.sh`,
  `check-task-commit-attribution.sh` — all exit 0 (mechanical-checks*.txt).
- Build sanity: `cargo check -p gyre-ports` clean; the full gyre-server test
  binary built without errors (all three adapters compile against the
  extended trait).
- Commit integrity: `fe303595` (the frontmatter commit) carries the complete
  product diff; product files are byte-identical between `fe303595` and the
  round-11 merge head `46fac1eb0` (empty diff over all six search files).

Note: the implementer's "sandbox cannot reach crates.io" restriction no
longer held in this review sandbox — the suites above were executed here,
not deferred.

## Non-blocking observations (recorded for future tasks, not defects in scope)

1. **Cross-tenant aggregation (pre-existing platform property).** The search
   port's `SearchQuery` has no tenant field, so `/search`, `/search/suggest`,
   and `/search/facets` all operate across the whole index for any
   authenticated caller. The task explicitly required parity with the
   existing `/search` endpoint, and spec §Access Scoping remains an open
   platform-wide gap (already tracked for the MCP path in coverage row 15).
   The new endpoints introduce no new exposure class beyond what `/search`
   already had, but a future task should thread caller scope through the
   search port.
2. **`total` is still the page length**, not the full match count (the spec
   example has `total: 14` = sum of type facet counts). Pre-existing
   behavior of `search_handler`, unchanged and out of this task's plan.
3. **Narrative/frontmatter mismatch:** the Shipped note claims the
   frontmatter "now also records the round-11 merge commit 46fac1eb0", but
   the `commits:` list contains only `fe303595`. Cosmetic — the listed
   commit carries the entire product diff and the attribution gate passes.
4. **PG adapter code is compile-verified only** (no live PostgreSQL in this
   sandbox; consistent with the repo's existing PG test posture). Its SQL
   mirrors the exercised SQLite/mem semantics and the existing `search()`
   query shapes, including the same `websearch_to_tsquery` match set.
5. SQLite `LIKE` case-insensitivity is ASCII-only while the mem adapter
   uses full Unicode `to_lowercase` — trivial cross-adapter divergence in
   case folding, no contract specifies more.

## Verdict

The task contract (four §API endpoints, `facet_counts` response field,
port method with adapter implementations, route registration, endpoint and
adapter tests) is met by real production code, verified by independently
executed focused suites and a killing mutation probe. **Approved.**
