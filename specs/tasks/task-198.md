---
title: "Spec Links — Persistent Forge-Maintained Spec Graph"
spec_ref: "spec-links.md §Forge-Maintained Spec Graph"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "spec-links.md §Forge-Maintained Spec Graph"
commits: ["57f548f620b71856726e7bada6b61ebaad39cc54", "12d95108b029812a4c8fcd863816304a4890f0f5", "17e4b4cb9f0f1d104ca58dc1470ae681163adee1", "e1466abbc4969b239fcb39c5366af4c048526d1e", "b869f5a9e91f06c4838fe3fab41197ff5360b830", "5f33847d5db78436ee063a34210ac6db96a58775", "7a1508bbeab83977ad4361f958d9f71a9bda88ba", "673fce33a1eb4e9273e1ecfff16da1d5bbab56a4", "2bc9b5bdc6a7e24aa0907af934469cd6f9fb3232", "071fd3b7e5aa81a7731cc1af67ffc1555d767d07", "9fb8d851175c88e4485215aa952da2bde6c32a16"]
---

## Spec Excerpt

From `spec-links.md` §Forge-Maintained Spec Graph:

> The forge maintains a tenant-wide directed graph of all spec links:
>
> ```sql
> CREATE TABLE spec_links (
>     id              TEXT PRIMARY KEY,
>     source_repo_id  TEXT NOT NULL,
>     source_path     TEXT NOT NULL,
>     source_sha      TEXT NOT NULL,
>     link_type       TEXT NOT NULL,
>     target_repo_id  TEXT,               -- NULL for unresolved cross-workspace links (resolved later by staleness checker)
>     target_path     TEXT NOT NULL,
>     target_sha      TEXT NOT NULL,
>     target_display  TEXT,               -- human-readable composite path (e.g., "@platform-core/api-svc/system/auth.md")
>     reason          TEXT,
>     status          TEXT NOT NULL DEFAULT 'active',
>     created_at      INTEGER NOT NULL,
>     stale_since     INTEGER
> );
> ```

The spec is explicit: the tenant-wide spec-link graph is a **persistent SQL table**. Staleness detection (§Automatic Staleness Detection), approval/merge gates (§Mechanical Gates), graph queries (§Querying the Graph), and the accountability patrol (§Accountability Agent Integration) all query this graph. If the graph is empty (or stale) after a restart, every one of those mechanisms silently degrades.

## Current State (the gap)

The graph is implemented **in-memory only** and is lost on restart:

- `SpecLinksStore = Arc<Mutex<Vec<SpecLinkEntry>>>` (`crates/gyre-server/src/spec_registry.rs:249`).
- Initialized **empty** at boot: `spec_links_store: Arc::new(Mutex::new(Vec::new()))` (`crates/gyre-server/src/lib.rs:947`); there is **no `spec_links` table, no migration, and no startup rebuild**.
- The store is populated **only** by `sync_spec_ledger` (`spec_registry.rs:318`), which runs on push / post-receive (`git_http.rs:687`, `mirror_sync.rs:57`, `api/repos.rs:548`).
- Consequence: after a server restart the tenant-wide graph is **empty** until every repo pushes again. Until then, staleness checks (`spec_link_staleness.rs`), approval/merge gates (`api/specs.rs`, `merge_processor.rs`), the graph query endpoints (`api/specs.rs`), and the patrol (`spec_patrol.rs`) all operate against an empty graph — producing wrong (silently permissive) results.
- `SpecLinkEntry` (`spec_registry.rs:224`) also **lacks the spec's `source_sha NOT NULL` column** entirely.

`SpecLinksStore` is referenced from: `api/graph.rs`, `api/repos.rs`, `api/specs.rs`, `git_http.rs`, `merge_processor.rs`, `middleware.rs`, `mirror_sync.rs`, `spec_link_staleness.rs`, `spec_patrol.rs`, `spec_registry.rs`, `lib.rs`, `mem.rs`. Any change to the store type or write path must keep all readers working.

## Implementation Plan

Model the persistence on the existing `SpecLedgerRepository` pattern (`crates/gyre-ports/src/spec_ledger_repo.rs` → `crates/gyre-adapters/src/sqlite/spec_ledger.rs` + `postgres/spec_ledger.rs` + `mem.rs`, wired in `lib.rs` via the `store!` macro).

1. **Add `source_sha` to `SpecLinkEntry`** (`spec_registry.rs:224`) as `pub source_sha: String`. Populate it in `sync_spec_ledger` where links are built (`spec_registry.rs:512`) with the **source spec's current blob SHA** (already computed during the sync for the source spec's ledger entry — reuse it, do not hardcode/placeholder). Update all `SpecLinkEntry {...}` literals (incl. the test builders at `spec_registry.rs:1402/1456/1984`) to set the new field.

2. **Migration.** Add a new dated migration `crates/gyre-adapters/migrations/2026-..._spec_links/{up.sql,down.sql}` creating the `spec_links` table with the **exact spec schema above** (all 13 columns; `source_sha TEXT NOT NULL`, `target_sha TEXT NOT NULL`, `status ... DEFAULT 'active'`). Add the table to `crates/gyre-adapters/src/schema.rs`. Index `target_path` and `source_repo_id` (staleness and repo-scoped reads query on these). Note: the spec column is `target_sha NOT NULL`, but `SpecLinkEntry.target_sha` is `Option<String>` for unresolved cross-workspace links — store the empty string (or the resolved SHA) rather than violating NOT NULL; document the mapping in the adapter.

3. **Port trait `SpecLinkRepository`** in a new `crates/gyre-ports/src/spec_link_repo.rs` (re-export from `ports/src/lib.rs`, mirroring `SpecLedgerRepository`):
   - `async fn list_all(&self) -> Result<Vec<SpecLinkEntry>>`
   - `async fn replace_for_source(&self, source_repo_id: &str, source_path: &str, links: &[SpecLinkEntry]) -> Result<()>` — atomically delete existing rows for a `(source_repo_id, source_path)` and insert the new set (matches how `sync_spec_ledger` recomputes a source spec's links on each push).
   - `async fn save(&self, entry: &SpecLinkEntry) -> Result<()>` — upsert by `id` (used by staleness/patrol status mutations).
   - `async fn delete_by_source_repo(&self, source_repo_id: &str) -> Result<()>` — for repo removal cleanup.
   Keep the trait minimal and driven by real callers; do not add speculative methods.

4. **SQLite + Postgres + Mem adapters** implementing the trait, following `spec_ledger.rs`. `SpecLinkEntry` must round-trip losslessly (serialize `link_type` via the existing snake_case serde; map `Option` columns).

5. **Persist on write.** Every place that currently mutates the in-memory `SpecLinksStore` must also persist through the repository so the SQL table is authoritative:
   - `sync_spec_ledger` rebuild of a source spec's links → `replace_for_source`.
   - Staleness transitions (`spec_link_staleness.rs`) and any status writes → `save`.
   - Repo deletion path (if links are dropped) → `delete_by_source_repo`.

6. **Boot rebuild / load.** At startup (in `lib.rs` app assembly, after the repository is wired), **load all persisted links from `SpecLinkRepository::list_all` into the in-memory `SpecLinksStore`** so existing readers keep working and the graph survives restart without waiting for re-push. (Loading into the existing `Arc<Mutex<Vec<..>>>` keeps the reader blast radius small; the SQL table is the durable source, the in-memory vec is the hot cache kept in sync on every write.)

7. **Wire the repository** into the app state with the `store!` macro in `lib.rs` alongside `spec_ledger` (SQLite/Postgres for the real server, `Mem` for the in-memory constructor used by tests).

## Acceptance Criteria

- [ ] `spec_links` migration exists with the **exact 13-column schema** from the spec (incl. `source_sha TEXT NOT NULL`), registered in `schema.rs`; `cargo build --all` runs migrations clean.
- [ ] `SpecLinkEntry` has a `source_sha: String` field populated from the source spec's real current SHA during `sync_spec_ledger` (not a placeholder/empty string).
- [ ] `SpecLinkRepository` port + SQLite + Postgres + Mem adapters; all link writes in the server go through it (SQL table is authoritative).
- [ ] On startup the in-memory graph is rebuilt from the persisted table via `list_all` — verified by a test that seeds links, drops/recreates the in-memory store from the repo, and asserts the graph is non-empty and identical.
- [ ] **Restart-durability test (the bug this task kills):** push a manifest with links so they persist; construct a fresh app state (new in-memory store) backed by the same repository/DB; assert the loaded graph contains the links **before any re-push**, and that a staleness/gate query returns the same result it would after a push. This test MUST fail against the current empty-init behavior.
- [ ] All existing `SpecLinksStore` readers (`api/specs.rs` graph endpoints, `merge_processor.rs`, `spec_link_staleness.rs`, `spec_patrol.rs`, `api/graph.rs`) compile and pass with the new field/persistence.
- [ ] `SpecLinkEntry` round-trips losslessly through each adapter (SQLite + Postgres + Mem) — verified by an adapter round-trip test including `Option` columns and each `SpecLinkType` variant.
- [ ] `cargo test --all` passes; `bash scripts/check-arch.sh` passes (no infra deps leak into `gyre-domain`; `SpecLinkEntry` stays where it is unless moved into `gyre-domain` like `SpecLedgerEntry`).

## Agent Instructions

- Do **not** change reader semantics: keep `SpecLinksStore` as the hot in-memory graph that readers consume, but make the SQL table authoritative and load it at boot. This bounds the blast radius while satisfying the spec's "persistent SQL table + survives restart" requirement.
- Reuse the source spec's SHA already computed inside `sync_spec_ledger` for `source_sha`; never hardcode.
- The restart-durability test is the section-closing evidence. It must genuinely fail on `main` (empty-init) and pass after your change — a self-confirming or mirrored-logic test does not close this section.
- Skip project-wide formatters/linters and the full suite until the end; then run `cargo test --all` and `scripts/check-arch.sh` once.
- Commit with conventional commits; author `Project Manager` is NOT the implementer — use your own agent identity.

## Shipped

The forge-maintained spec-link graph (spec-links.md §Forge-Maintained Spec
Graph) is now a persistent SQL table with the in-memory `SpecLinksStore`
retained as a write-through hot cache, so the graph survives restarts instead
of silently degrading to empty for staleness checks, approval/merge gates,
graph queries, and the accountability patrol.

**Production behavior:**

- Migration `2026-10-08-000056_spec_links` creates the `spec_links` table with
  the spec's 13-column schema (`source_sha TEXT NOT NULL`, `target_sha`
  NOT NULL storing `''` for the `Option::None` unresolved-link case, mapped
  and documented in both SQL adapters), plus indexes on `source_repo_id`,
  `source_path`, and `target_path`; registered in `schema.rs`. Portability
  checked (`check-migration-sql-portability.sh` PASS — runs on both SQLite
  and PostgreSQL).
- `SpecLinkEntry` gained `source_sha: String`, populated in `sync_spec_ledger`
  from the source spec's real blob SHA computed at HEAD (reused from
  `manifest_sha_by_path`, never a placeholder). The type moved to
  `gyre-domain::spec_links` alongside `SpecLedgerEntry` so the port and
  adapters can persist it; `gyre-server::spec_registry` re-exports it.
- New port `gyre_ports::SpecLinkRepository` (`list_all`,
  `replace_for_source`, `save` upsert-by-id, `delete_by_source_repo`) with
  SQLite + Postgres adapters (`crates/gyre-adapters/src/{sqlite,postgres}/spec_links.rs`)
  and `MemSpecLinkRepository` (`mem.rs`), wired into `AppState.spec_link_repo`
  via the `store!` macro (SQLite/PG when `GYRE_DATABASE_URL` is set, Mem
  otherwise).
- Every link mutation writes through to the table: `sync_spec_ledger` issues
  one `replace_for_source` per manifest spec (including empty link sets, so
  manifest-removed links are deleted from the table — no resurrection on
  next boot); staleness transitions and cross-workspace re-resolution in
  `spec_link_staleness.rs` call `save`; repo deletion (`api/repos.rs`)
  calls `delete_by_source_repo`. The in-memory store is updated alongside
  every durable write so both stay in sync.
- Boot: `load_spec_links_into_store` (lib.rs, called from `build_state` and
  the mem constructor) populates the hot cache from `list_all` on a dedicated
  thread with its own runtime; read failure is logged at error level rather
  than silently masquerading as an empty graph.

**Test evidence:**

- Restart-durability (the bug this task kills):
  `restart_rebuilds_spec_link_graph_from_persisted_table` and
  `staleness_query_parity_after_restart` in `spec_registry.rs` — push a real
  manifest through `sync_spec_ledger` against a real SQLite file, then build
  a brand-new store from the same repo (including a fresh storage handle on
  the same file) and assert the graph is identical and the staleness verdict
  (stale + `stale_since`) survives without any re-push. These fail against
  the pre-task empty-init behavior.
- Cross-repo scoping and removal: `sync_replaces_links_scoped_to_source_repo`
  (repo A's rows survive a repo-B push of the same path; cache and table
  agree) and `sync_clears_durable_rows_when_links_removed_from_manifest`
  (removed links are deleted from the durable table, not resurrected).
- Adapter round-trips: `sqlite::spec_links` tests cover every
  `SpecLinkType` variant, all `Option` columns in `Some`/`None` forms,
  reopen-durability, replace scoping, and repo-scoped delete;
  `mem::spec_link_contract_tests` mirror the round-trip and scoping contract
  for the Mem adapter. The Postgres adapter follows the identical row-mapping
  and query structure as the SQLite adapter; the repo has no PG test
  infrastructure (zero test modules under `postgres/`, no PG service in CI),
  so PG is compile-verified — same convention as every other PG adapter.
- `migrations_create_tables` extended with `spec_links`.

**Verification evidence (repair session, HEAD 733ba170, base 18c44f1a):**

- Focused probes, all PASS (records under /tmp/stage/review-evidence):
  - `cargo test -p gyre-server --lib spec_registry::tests -- restart_rebuilds_spec_link_graph_from_persisted_table staleness_query_parity_after_restart sync_replaces_links_scoped_to_source_repo sync_clears_durable_rows_when_links_removed_from_manifest`
    — ok. 46 passed; 0 failed (includes all four section-closing tests).
  - `cargo test -p gyre-adapters --lib sqlite::spec_links` — ok. 4 passed;
    0 failed (round-trip, replace scoping, reopened-database durability,
    repo-scoped delete).
  - `cargo test -p gyre-server --lib mem::spec_link_contract_tests` — ok.
    3 passed; 0 failed.
  - Reader modules with the new field/persistence: `spec_link_staleness` +
    `spec_patrol` (25 passed), `api::specs` (72), `api::graph` (23),
    `merge_processor` (49), `migrations_create_tables` (1) — all 0 failed.
  - `scripts/check-arch.sh` PASS; `check-migration-sql-portability.sh`,
    `check-mem-port-contracts.sh`, `check-in-memory-state-stores.sh`,
    `check-unnamed-tuple-carriers.sh`, `check-task-commit-attribution.sh`
    all PASS at HEAD.
- `cargo test --all` (full workspace suite) remains owned by verification
  and publication per the assignment; the previous assignment's full-suite
  run was interrupted by an infrastructure timeout mid-compile, not a test
  failure.
- Postgres adapter is compile-verified: `postgres` is unconditionally
  compiled in gyre-adapters (no feature gate; workspace diesel includes the
  postgres backend), so the sqlite adapter test build compiled
  `postgres::spec_links` in the same crate.

**Sandbox/gate notes:**

- `check-task-commit-attribution.sh` previously failed on task-196 commit
  `05709c24` missing from that task's frontmatter — fixed on main by the
  task-227 repair (base commit 18c44f1a); the check now passes at HEAD.
- `check-unnamed-tuple-carriers.sh` was failing on base (stale line-keyed
  exemptions after line drift: git_http/mem entries off by their shift, plus
  a trailing-comma arity miscount on `otlp_receiver.rs`'s 3-field tuple).
  Renumbered the same frozen 9 entries to the actual carrier lines and
  corrected the otlp inline `tuple-carrier:ok` marker's reason to name the
  real cause (trailing-comma false positive). No entries added; check now
  passes.
- Removed the accidental `web/dist` rebuild a pipeline checkpoint
  re-captured after bd5e586d had dropped it (commits caa0f93b + 733ba170;
  web sources are byte-identical to base — `web/dist` now matches the base
  tree exactly, verified `git diff 18c44f1a..HEAD -- web/dist` is empty).
  The gyre-server build script re-runs `npm ci && npm run build` whenever
  `web/node_modules` is absent (fresh sandbox), so any full-build gate will
  re-dirty `web/dist`; that rebuild is environmental, not a source change.
