# Review — task-201 (Persistent full-text search backend: SQLite FTS5 + Postgres tsvector)

Spec: `search.md` §Search Index (Technology, Index Schema), Design Principle 4.
Base `6a9739e5` → candidate `ddd6cc0c`. Diff scoped to 7 files: new
`gyre-adapters/src/sqlite/search.rs` (+621), new `gyre-adapters/src/postgres/search.rs`
(+229), module registrations in both `mod.rs` files, `store!` wiring in
`gyre-server/src/lib.rs`, new `gyre-server/tests/search_wiring.rs` (+195), and the
task's own record. Port surface (`gyre-ports/src/search.rs`) and `gyre-domain`
untouched — hexagonal boundary intact (`check-arch.sh` pass).
Verdict: **approved**.

## Round 1 (this review, checkout at `ddd6cc0c`)

### Prior finding verified fixed

The round-1 finding on candidate `0bd8416a` (merge `72bbae0a` had reduced
`specs/tasks/task-200.md` to a single mangled line) is repaired: at `ddd6cc0c`
the file is byte-identical to assigned base `6a9739e5` (131 lines, `diff`
empty). Repair commit `0956e428`; no other spec/task files touched by this
candidate.

### Test runs (all via `/tmp/stage/bin/cargo`, evidence in `/tmp/stage/review-evidence/`)

- `cargo test -p gyre-adapters --lib sqlite::search` → **8 passed, 0 failed**.
  Covers: schema assertion against `sqlite_master` (exact specced columns +
  `tokenize='porter unicode61'`), porter stemming (`running`→`runs`), AND-of-terms,
  hostile-input safety, facet round-trip via `metadata` JSON, `**` snippet markers,
  bm25 score ordering + (0,1] normalization, entity_type/workspace_id filters,
  upsert-replaces + delete, `reindex_all` clear-count, durability across reopen.
- `cargo test -p gyre-server --test search_wiring` → **1 passed, 0 failed**.
  Proves `build_state` under `GYRE_DATABASE_URL=sqlite://<temp file>` wires
  `state.search` to the FTS5 adapter via `store!`, and drives the real
  POST /api/v1/tasks → GET /api/v1/search flow through the full router
  middleware stack (`tower::ServiceExt::oneshot`): stemmed match, snippet
  markers, positive bm25-derived score, facet round-trip (`status=backlog`
  from the real `tasks.rs:209` production callsite), and durability across a
  second `build_state` on the same file.
- **Mutation probe (restored, `git status` clean after)**: temporarily reverted
  the `store!` wiring back to `Arc::new(MemSearchAdapter::new())` →
  `sqlite_url_wires_fts5_search_port` **FAILS** at the porter-stemming kill
  condition ("porter stemming must match runs~running; empty/mem-backed result
  means state.search is NOT the FTS5 adapter"). The test genuinely kills a
  mem/substring regression — confirmed `MemSearchAdapter` is lowercase
  `contains` matching (`mem_search.rs:30-47`), which cannot stem.
- `bash scripts/check-arch.sh` → pass. `check-migration-sql-portability`,
  `check-migration-versions`, `check-in-memory-state-stores` → all pass (the
  FTS5/tsvector DDL correctly lives on per-backend init paths, NOT the shared
  embedded `migrations/` dir, which runs on both engines).
- Focused clippy (`-p gyre-adapters --lib --tests`, `-p gyre-server --lib
  --tests`, `-W clippy::all`): zero diagnostics attributable to any new file
  (`sqlite/search.rs`, `postgres/search.rs`, `search_wiring.rs`, lib.rs wiring).
  Pre-existing warnings elsewhere in the crate are unchanged baseline.

### Verified against the task contract

- **Exact specced FTS5 schema** — `FTS5_DDL` is the spec's DDL verbatim;
  created on the SQLite-only init path (`new_for_tenant`, which `new`
  delegates to) and asserted against `sqlite_master` by test.
- **Real FTS semantics, not LIKE** — MATCH expression builds each
  non-alphanumeric-split token into a quoted phrase (adjacent phrases = AND;
  quoting forecloses MATCH-syntax injection — hostile-input test), ranked by
  `bm25(search_index)` inverted to (0,1] higher-is-better, snippet via
  `snippet(search_index, 6, '**', '**', '…', 32)` (column 6 = `body`;
  verified against column order). Filters are WHERE clauses on the UNINDEXED
  columns, not post-filters.
- **PG adapter** — `tsv tsvector GENERATED ALWAYS AS (to_tsvector('english',
  title || ' ' || body)) STORED` + GIN index on the PG-only init path;
  `websearch_to_tsquery` (implicit AND), `ts_rank` scoring, `ts_headline`
  with `**` markers, same filter/limit/upsert (atomic `ON CONFLICT … DO
  UPDATE`) semantics. Compiles clean (included in the gyre-adapters lib
  build/clippy). Selected by `store!` identically to every other port.
- **No fabricated scopes** — tenant_id derived from the real `workspaces` row;
  empty string when unknown, never a literal `"default"` fallback.
- **`reindex_all`** clears and returns the count dropped; the
  rebuild-from-domain half is correctly delegated to task-202 (exists,
  `depends_on: [task-201]`, not-started) rather than faked.
- **Zero new external dependencies** — no `Cargo.toml` changes in the diff.
- **Sandbox-restricted item, not a code defect**: no PostgreSQL server is
  installed in this sandbox (`psql`/`initdb` absent), so the PG adapter's
  runtime behavior is compile-verified only here. Deferred to verification on
  a PG-capable host: run the PG-backed search path under
  `GYRE_DATABASE_URL=postgres://…`. Likewise `cargo test --all` and the
  `api_integration` suite are workspace-wide and belong to the verification
  stage (the review wrapper rejects `--all`/`--all-targets`); note
  `api_integration` contains no FTS search tests (only admin audit search),
  and the router-level search flow is covered by `search_wiring`.
