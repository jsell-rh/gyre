# Review — task-201 (Persistent full-text search backend: SQLite FTS5 + Postgres tsvector)

Spec: `specs/system/search.md` §Search Index (Technology, Index Updates context, Index Schema); task `specs/tasks/task-201.md`.
Assignment: base `6bf777a6a44f28052ed5af28bf6fb013fde6df48`, candidate `3f2f62f667e64aa3704304157afbcd582ad7b918`.
Verdict: **complete** — approved.

## Scope inspected

`git diff 6bf777a6..3f2f62f6` touches exactly: `gyre-adapters/src/sqlite/search.rs` (+621, new), `gyre-adapters/src/postgres/search.rs` (+229, new), `sqlite/mod.rs`/`postgres/mod.rs` (init hooks + module decls, +5 each), `gyre-server/src/lib.rs` (wiring, +5/−1), `gyre-server/tests/search_wiring.rs` (+195, new), and the two task bookkeeping files (`task-201.md`, `task-200.md` frontmatter). No scripts, exemptions, docs, or unrelated tasks touched; `git diff --stat -- scripts/ specs/coverage/` is empty.

## Spec conformance (§Search Index → Technology / §Index Schema)

- **Exact FTS5 schema.** `FTS5_DDL` (sqlite/search.rs:32–44) is the specced `CREATE VIRTUAL TABLE IF NOT EXISTS search_index USING fts5(...)` verbatim — column order, UNINDEXED on tenant/workspace/repo, `metadata`, `tokenize='porter unicode61'`. The schema test asserts against `sqlite_master` including the tokenizer string.
- **Dialect isolation honored.** DDL runs on the SQLite-only init path (`new_for_tenant`, after `run_pending_migrations`) and the PG-only path (`PgStorage::new_for_tenant`), NOT in the shared diesel `migrations/` dir — verified empty of `fts5`/`search_index` by grep. `check-migration-sql-portability.sh` and `check-migration-versions.sh` pass. This matters: the shared dir is embedded for both backends and FTS5 syntax would break PG startup.
- **PG tsvector backend.** `TSVECTOR_DDL` creates `search_index` with `tsv tsvector GENERATED ALWAYS AS (to_tsvector('english', title || ' ' || body)) STORED` + GIN index; search uses `websearch_to_tsquery` (implicit AND), `ts_rank`, `ts_headline` with `**` markers, same filter/limit/upsert semantics (`ON CONFLICT (entity_type, entity_id) DO UPDATE`). No test coverage (no PG server in this environment or CI — see Verification limits), but the SQL is standard and mirrors the SQLite adapter's tested contract. This matches the task plan, which specifies creation on the PG-only init path without a runnable PG test requirement.
- **Zero external search dependencies.** No new crates; `Cargo.toml` untouched. FTS5 comes from the existing bundled `libsqlite3-sys` (workspace `features = ["bundled"]`), tsvector from PG itself.
- **Wiring via `store!`.** `lib.rs:991–997`: `search: store!(dyn gyre_ports::SearchPort, gyre_adapters::MemSearchAdapter::new())` — PgStorage → SqliteStorage → mem fallback, identical to every sibling repository. The mem adapter survives only as the zero-DB fallback (also used by `mem.rs` test state), which the task plan explicitly permits.

## Implementation correctness (focused probes)

- **bm25 semantics.** Engine control probe: `bm25()` returns ≤ 0 (more negative = more relevant); the adapter orders `ORDER BY rank` (ascending, best first) and maps to `score = inverted/(inverted+1)` ∈ (0,1] — monotonic, higher = better. The bm25-ordering test (3×`jwt` doc outranks 1×`jwt` doc) exercises real relevance, not a constant.
- **AND-of-terms + injection safety.** `build_match_expr` splits on non-alphanumerics and quotes each token as a phrase; adjacent phrases are AND in FTS5. Engine probe confirms quoted-phrase AND returns only docs containing all terms and that phrase quoting neutralizes `AND`/`OR`/`NEAR`/`" * - ( ) ^ :` — the hostile-query test passes without a MATCH parse error. The one dynamic `format!` SQL (search.rs:181) interpolates only the `BODY_COL` const and literal SQL — all user input remains bind parameters.
- **Filter clauses at index level.** The exact emitted SQL (`MATCH ?1 AND (?2 IS NULL OR entity_type = ?2) AND (?3 IS NULL OR workspace_id = ?3) ORDER BY bm25 ... LIMIT ?4`) was replayed against raw SQLite with no-filter/et/ws/combined-exclusion/limit cases — all behave as the adapter tests assert. Diesel binds `Option<String>` as NULL for the `Nullable<Text>` params, which the `?N IS NULL` arms consume.
- **Upsert + durability.** Delete-then-insert inside a transaction keyed on `(entity_type, entity_id)`; engine probe confirms the pattern leaves no stale rows; tests cover upsert replacement, delete, reopen durability (the property MemSearchAdapter lacks).
- **Facets.** `serde_json` round-trip through the `metadata` column (HashMap<String,String> ↔ JSON object); malformed metadata deserializes to empty rather than erroring — acceptable degradation, facets are advisory.
- **Tenant derivation is safe.** `tenant_id` is looked up from the real `workspaces` row (NotFound → empty string, never a fabricated `"default"`); explicit early-return on other errors. This is the pattern the repo's fabricated-scope invariants require — `check-fabricated-scope-defaults.sh`, `check-scope-literal-defaults.sh` both pass. Note the workspaces lookup is unfiltered by the storage's own tenant scope, so a workspace id belonging to another tenant still resolves its real tenant — correct here, since the column is descriptive and access scoping is task-203's explicit assignment.
- **`reindex_all` is not faked.** Clears the FTS table and returns the count dropped; the port doc comment's "number of documents indexed" rebuild-from-domain half is the task-202 coordinator per the task plan ("do NOT stub a fake rebuild here"). No hollow rebuild claiming completion.

## Test quality (no inflation)

The 8 adapter tests + 1 wiring test each carry a kill condition tied to the specced behavior:
- `porters_stemming_proves_fts_not_substring` — `running`→`runs` fails under any substring/LIKE/mem regression (mem's `contains("running")` on "…runs…" is false).
- Snippet `**` markers, bm25 ordering + (0,1] score range, facet round-trip, AND semantics, empty-query, hostile-query, filter combinations, upsert staleness, delete, reopen durability, and the `sqlite_master` schema assertion.
- The wiring test additionally proves `build_state` under `GYRE_DATABASE_URL=sqlite://<file>` selects the FTS5 adapter (stemming + markers + cross-`build_state` durability kill a mem-wiring regression) and drives the real POST /api/v1/tasks → GET /api/v1/search flow through the full router stack via `oneshot` (valid where loopback TCP is unavailable, per sandbox capabilities).

Weakness noted and accepted: `snippet.contains("**")` in the wiring test is looser than the adapter test's `**jwt**`/`**quokkas**` token-marker assertions; the adapter-level assertions carry the precision.

## Verification runs (this checkout at `3f2f62f6`, evidence in /tmp/stage/review-evidence/)

- `cargo test -p gyre-adapters --lib sqlite::search` → **8 passed, 0 failed**.
- `cargo test -p gyre-adapters --lib` (full) → **352 passed, 0 failed, 12 ignored** — no init-path regressions from the new `ensure_fts_table` hook.
- `cargo test -p gyre-server --test search_wiring` → **1 passed, 0 failed**.
- `python3 scripts/check-clippy-diff.py 6bf777a6` → exit 0 (changed lines clean; 1145 pre-existing warnings elsewhere). The module-scoped `#![allow(clippy::redundant_field_names)]` in both search.rs files is justified in comments: diesel_derives 2.3.7 generates the redundant field init in a dummy module that escapes item-level allows. The allow is scoped to files whose only structs are `QueryableByName` row carriers — not a blanket repo-wide suppression.
- `python3 scripts/check-rustfmt-diff.py 6bf777a6` → exit 0.
- `bash scripts/check-arch.sh` → pass. Mechanical battery all exit 0: migration-sql-portability, migration-versions, in-memory-state-stores, task-commit-attribution, inert-enforcement, fabricated-scope-defaults, scope-literal-defaults, lossy-secret-conversion, fail-open-ref-resolution, relative-path-defaults, forwarded-header-trust.
- Engine-level control probes (python3 sqlite3) validating porter stemming, bm25 sign, snippet markers, and the exact filter-SQL shape — see `task-201-sqlite-engine-controls.txt`.

## Verification limits (host/CI items, not code defects)

This sandbox cannot `accept()` on TCP (capabilities.json, errno 95), so the listener-binding `api_integration` suite and `cargo test --all`'s listener-dependent tests cannot run here. Required on a listener-capable host / GitHub CI: `cargo test --all`. Postgres adapter behavior has no runnable test in any environment currently available to this pipeline (no PG server); CI runs above plus a future PG-backed suite are the appropriate home — the task's acceptance criteria asked for `store!` selection when `GYRE_DATABASE_URL=postgres://…` is set, which is structurally guaranteed by the same macro arm every other PG repository uses.

## Findings

- (none)

Pre-existing state confirmed unchanged by this diff: task/agent index docs carry `workspace_id: None` at the callsites (coverage row 6/8 note) — that is task-202/task-203 scope, explicitly out of this task's plan, and the adapter stores empty-string then, which the workspace filter correctly does not match (None filter → no WHERE arm). The mem.rs test-state hardcoding of MemSearchAdapter is test-only state, not production wiring.

— Reviewer, 2026-10-10
