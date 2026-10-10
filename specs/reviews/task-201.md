# Review: task-201 — Persistent full-text search backend (SQLite FTS5 + Postgres tsvector)

- **Candidate:** `07a84ccd7cbbd9e05a33a6124e1713de81dcb096`
- **Base:** `f4acb4ebcaf930ada2f1318b8aa2adbf244e720f`
- **Verdict:** **Approved** (independent evidence; `/tmp/stage/verdict.json`)
- **Date:** 2026-10-10

## What was reviewed

The full base→candidate diff: new `crates/gyre-adapters/src/sqlite/search.rs` (605 ln
incl. tests), new `crates/gyre-adapters/src/postgres/search.rs`, module wiring in
both `mod.rs` init paths, the one-line `store!` change in `gyre-server/src/lib.rs:994`,
the new `crates/gyre-server/tests/search_wiring.rs`, and task/frontmatter bookkeeping.

## Contract checks against task acceptance criteria

| Criterion | Evidence |
|---|---|
| FTS5 table with exact specced schema incl. `tokenize='porter unicode61'` | `FTS5_DDL` is the spec §Index Schema DDL verbatim; `fts5_table_exists_with_specced_schema` asserts columns + tokenizer against `sqlite_master` on a real temp SQLite file. Passed (8/8, evidence file `task-201-review-sqlite-adapter-tests.txt`). |
| Wiring via `store!` under `sqlite://` | `lib.rs:994` uses the same `store!` macro as every other repo; `search_wiring` integration test drives `build_state` + the real router via `oneshot` and proves porter stemming, `**` snippet markers, positive bm25-derived score, facet round-trip, and cross-`build_state` durability. Passed (1/1, `task-201-review-wiring-test.txt`). |
| Real FTS, not LIKE/substring | Porter-stemming kill condition (`running` matches indexed `runs`) in both adapter and wiring tests — a substring matcher fails it by construction. |
| AND-of-terms, entity_type/workspace_id filters as index WHERE clauses | `and_of_terms_semantics` + `entity_type_and_workspace_filters_apply_at_index_level` pass; SQL binds filters into the FTS query's WHERE, not post-filtered. |
| Facets via `metadata` JSON round-trip | asserted at port level and through the HTTP surface (`facets.status == "backlog"`). |
| Upsert/delete/reindex semantics | transactional delete-then-insert (FTS5 has no ON CONFLICT), `upsert_replaces_and_deletes` and `reindex_all_clears_and_reports_count` pass. `reindex_all` clears + returns count dropped; rebuild-from-domain is explicitly task-202, not faked here — matches the plan's instruction. |
| Postgres tsvector + GIN + ts_rank/ts_headline behind `store!` | Source-verified: STORED generated `tsv`, GIN index, `websearch_to_tsquery` (implicit AND), `ts_rank`, `ts_headline` with `**` markers, `ON CONFLICT` upsert, PG-only init path. `gyre-adapters` builds clean. Runtime PG verification not possible in this sandbox (no PG server; TCP `accept()` errno 95 per capabilities.json) — see Host verification. |
| Zero new external dependencies | `git diff base..candidate -- Cargo.toml Cargo.lock crates/*/Cargo.toml` is empty. |
| `check-arch.sh` | exit 0 — "Architecture lint passed" (`task-201-review-check-arch.txt`); FTS SQL lives only in `gyre-adapters`, dialect DDL kept out of the shared migrations dir (portability check respected). |

## Test-quality assessment

The wiring test would fail on every plausible regression: a revert to
`MemSearchAdapter` fails the stem match, the marker assertion, and the durability
assertion; a LIKE-backed adapter fails stemming; dropped facets fail both the port
and HTTP assertions. Hostile-query input is quoted into literal FTS5 phrases (no
MATCH syntax injection / parse errors). The env-var poke is safe because
`search_wiring` is a dedicated test binary and no other `gyre-server` test binary
reads `GYRE_DATABASE_URL`.

## Observations (non-blocking)

- The interrupted-run recovery dragged in `web/dist` bundle-hash churn (commit
  `2eb2618b`, not in the frontmatter list). `web/src` is unchanged in the diff, so
  the committed bundle is a rebuild of unchanged sources; cosmetic process noise
  only.
- Base commit `f4acb4eb` is task-189-labeled and was added to task-189's `commits:`
  list in this change — pre-existing attribution drift fixed, required for
  `check-task-commit-attribution` to pass.

## Host verification required (sandbox restrictions, not code defects)

This sandbox cannot bind/accept TCP (errno 95) and has no PostgreSQL server:

- `cargo test --all`
- `cargo test -p gyre-server --test api_integration`
- A PG-backed smoke of `SearchPort` against a real `postgres://` database
  (`build_state` under a PG URL; index a task, search a stemmed term, assert
  snippet markers + facets) — the PG adapter compiles and is source-verified but
  has no executed runtime probe here.

Evidence: `/tmp/stage/review-evidence/task-201-independent-review.txt` and
sibling files (`task-201-review-sqlite-adapter-tests.txt`,
`task-201-review-wiring-test.txt`, `task-201-review-check-arch.txt`,
`task-201-review-killswitch-design.json`).
