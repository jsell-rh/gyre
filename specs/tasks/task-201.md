---
title: "Implement persistent full-text search backend (SQLite FTS5 + Postgres tsvector)"
spec_ref: "search.md §Search Index"
depends_on: []
progress: not-started
coverage_sections:
  - "search.md §Search Index"
  - "search.md §Technology"
  - "search.md §Index Schema"
commits: ["4b2a7b0730aed3a341aff3fd59ac70e4f31dd9d9", "06149be2c21de14e8a3c310cbe9dc6251e49bdfd", "a550da3739d298561cde9eaace74ae7d6fdd0097", "88ab2a656b4477456b83c1788ff22b6e2d91b2ec", "e4cf94f49c2222cda5030e4bd52270c7e721a068", "4319f2bb63eb94de4fc819ebdeaeb9ca6d30e4e8", "0c980dda5cfdd9cebe56cdabe4c1a0db74da56e0"]
---

## Spec Excerpt

From `search.md` §Search Index → Technology:

> SQLite FTS5 for single-node deployments. For Postgres deployments, use Postgres full-text search (`tsvector`/`tsquery`). Both are built-in - no external search infrastructure required (no Elasticsearch dependency).
> ... the default must work with zero external dependencies.

From `search.md` §Index Schema:

> ```sql
> CREATE VIRTUAL TABLE search_index USING fts5(
>     entity_type, entity_id,
>     tenant_id UNINDEXED, workspace_id UNINDEXED, repo_id UNINDEXED,
>     title, body, metadata,
>     tokenize='porter unicode61'
> );
> ```

From §Search Index → Index Updates: "The search index is updated synchronously on every write."

From Design Principle 4: "Full-text search must return results in <200ms ... This means a dedicated search index, not SQL LIKE queries."

## Problem (current state — code-verified)

Search is backed ONLY by `MemSearchAdapter` (in-memory `Vec` substring matcher, `crates/gyre-adapters/src/mem_search.rs`) wired in production at `crates/gyre-server/src/lib.rs:956` (hardcoded, NOT via the `store!` backend-selection macro). There is **no persistent FTS backend** and no `search_index` table. This is the root gap behind coverage rows §Search Index, §Technology, §Index Schema.

## Implementation Plan

1. **SQLite FTS5 adapter** — new file `crates/gyre-adapters/src/sqlite/search.rs`:
   - Implement `gyre_ports::search::SearchPort` for `SqliteStorage`.
   - Create the FTS5 virtual table with the EXACT schema above (`tokenize='porter unicode61'`, `tenant_id`/`workspace_id`/`repo_id` UNINDEXED, `metadata` column holding JSON facets). SQLite FTS5 DDL is dialect-specific and MUST NOT go in the shared diesel `migrations/` dir (that dir is embedded for both SQLite and Postgres via `embed_migrations!("migrations")` and FTS5 syntax would break PG). Add it to the SQLite-only migration list in `crates/gyre-adapters/src/sqlite/migrations.rs` (`CREATE VIRTUAL TABLE IF NOT EXISTS search_index USING fts5(...)`), bumping the version array.
   - `index(doc)`: upsert = `DELETE` existing `(entity_type, entity_id)` row then `INSERT`. Serialize `doc.facets` to JSON into `metadata`. Populate `tenant_id` (derive from workspace when available; store empty string if unknown).
   - `search(query)`: use FTS5 `MATCH` (default AND-of-terms). Rank with `bm25(search_index)`; return a real relevance `score` (normalize/invert bm25 so higher = better). Extract `snippet` via FTS5 `snippet(search_index, <body_col>, '**', '**', '…', 32)`. Honor `entity_type` and `workspace_id` filters as WHERE clauses on the UNINDEXED columns. Deserialize `metadata` JSON back into `SearchResult.facets`. Respect `limit`.
   - `delete(entity_type, entity_id)`: `DELETE` matching row.
   - `reindex_all`: clear the FTS table only (return count cleared). Full rebuild from domain entities is task-202 (server-level coordinator); do NOT stub a fake rebuild here.

2. **Postgres tsvector adapter** — new file `crates/gyre-adapters/src/postgres/search.rs`:
   - Implement `SearchPort` for `PgStorage`.
   - Create a `search_index` table with `title`/`body` text columns, `tenant_id`/`workspace_id`/`repo_id`/`entity_type`/`entity_id` columns, a `metadata jsonb`, and a `tsv tsvector` generated column (`to_tsvector('english', title || ' ' || body)`) with a GIN index. PG DDL is dialect-specific — create it in a PG-only init path (`postgres/mod.rs` init, raw SQL `CREATE TABLE IF NOT EXISTS ... / CREATE INDEX IF NOT EXISTS`), NOT the shared diesel migrations dir.
   - `search`: `plainto_tsquery`/`websearch_to_tsquery` against `tsv`, rank with `ts_rank`, snippet with `ts_headline`. Same filter/limit semantics as SQLite.
   - `index`/`delete`/`reindex_all`: mirror SQLite semantics.

3. **Wire via `store!` macro** — `crates/gyre-server/src/lib.rs:956`:
   - Replace the hardcoded `search: Arc::new(gyre_adapters::MemSearchAdapter::new())` with `search: store!(dyn SearchPort, mem::... /* MemSearchAdapter */)` so it uses PgStorage → SqliteStorage → in-memory fallback exactly like every other repository. `MemSearchAdapter` remains the zero-DB fallback only.
   - Export the new adapters from `gyre-adapters/src/lib.rs` / `sqlite/mod.rs` / `postgres/mod.rs` as needed.

4. **Migrate existing index callsites** — the 3 already-indexed types must keep working through the new backend (they call `SearchPort::index` unchanged): task (`api/tasks.rs`), mr (`api/merge_requests.rs`), agent (`api/agents.rs`). No callsite change needed if the port surface is unchanged; verify they exercise the SQLite adapter under `GYRE_DATABASE_URL=sqlite://...`.

## Acceptance Criteria

- [ ] Under `GYRE_DATABASE_URL=sqlite://<file>`, `state.search` is the FTS5 adapter (not MemSearchAdapter); a `search_index` FTS5 virtual table exists with the exact specced columns and `tokenize='porter unicode61'`.
- [ ] `index()` then `search()` round-trips against real SQLite: indexed task/mr/agent docs are found by term; results carry a bm25-derived `score` and an FTS5 `snippet` with match markers.
- [ ] Default AND-of-terms semantics preserved (multi-term query requires all terms).
- [ ] `entity_type` and `workspace_id` filters apply as index-level WHERE clauses.
- [ ] Postgres adapter implements `SearchPort` with tsvector + GIN + `ts_rank`/`ts_headline`, selected by `store!` when `GYRE_DATABASE_URL=postgres://...`.
- [ ] Zero new external dependencies (no Elasticsearch/Meilisearch); FTS5 and tsvector are built in.
- [ ] A test indexes several docs into a real temp SQLite FTS index and asserts term match, AND semantics, facet round-trip via `metadata`, snippet markers, and score ordering. The test MUST fail if the adapter falls back to substring/LIKE matching or drops facets.
- [ ] `cargo test --all` and `bash scripts/check-arch.sh` pass.

## Agent Instructions

- Read `crates/gyre-ports/src/search.rs` (port surface — keep it stable unless a real need arises), `crates/gyre-adapters/src/mem_search.rs` (behavior to match/replace), `crates/gyre-adapters/src/sqlite/mod.rs` + `sqlite/migrations.rs` (SQLite init + migration list pattern), `crates/gyre-adapters/src/postgres/mod.rs` (PG init pattern), and `crates/gyre-server/src/lib.rs:800-960` (the `store!` macro + wiring).
- Follow the hexagonal boundary: FTS SQL lives ONLY in `gyre-adapters`; `gyre-domain` stays infra-free.
- Do NOT run project-wide formatters/linters mid-task; run `cargo test --all` and `check-arch.sh` once at the end.
- On completion set `progress: ready-for-review` and record commit SHAs.
