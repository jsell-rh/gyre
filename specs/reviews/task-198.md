# Review — task-198 (Spec Links — Persistent Forge-Maintained Spec Graph)

Spec: `specs/spec-links.md` §Forge-Maintained Spec Graph (persistent tenant-wide `spec_links` SQL table; graph must survive restart for staleness detection, mechanical gates, graph queries, and the accountability patrol).
Base `63aa1486`, candidate `9fa13438` (round 2 — a prior review on checkpoint `c957f32b`'s candidate found a cross-repo id collision plus two test-quality defects; this round re-reviews the fixed candidate).
Verdict: **needs-revision** (one blocking finding).

## Round 2

### Prior-round findings — all three resolved

- **Cross-repo link-id collision (prior blocking):** fixed. Link ids are now built by `gyre_domain::spec_links::spec_link_id(source_repo_id, source_path, link_type, target)` (`crates/gyre-domain/src/spec_links.rs:88`), scoped by the declaring repo so two repos carrying the same spec path get distinct primary keys, matching the `(source_repo_id, source_path)` scoping of `replace_for_source`/`delete_by_source_repo` in every store. `sync_replaces_links_scoped_to_source_repo` now pushes the *identical* manifest as repo-a then repo-b through real `sync_spec_ledger` calls (production id construction, no hand-built ids) and asserts cache == durable table == restarted graph with 2 distinct rows — the exact collision scenario from round 1. Verified green.
- **Divergent id-space test seeding (prior):** fixed by the rewrite above.
- **Self-confirming trailing assert in the parity test (prior minor):** fixed in the rewrite — the test now runs the real `spec_link_staleness::run_once` against the restarted graph and re-derives `stale` + `stale_since` from the ledger SHA vs the persisted pinned SHA, then asserts the verdict is written through to the durable table. (But see F1: this test never executes at the candidate.)

### Implementation verified working (no findings)

- **Migration** `2026-10-08-000056_spec_links` with the spec's 13 columns, registered in `schema.rs`, next unused sequence number, portable across SQLite/PG (`check-migration-sql-portability.sh` PASS). `Option` ↔ NOT NULL mapping (`''` for `None`) documented in both SQL adapters. Indexes on `source_repo_id`, `source_path`, `target_path`.
- **`source_sha`** populated in `sync_spec_ledger` from `manifest_sha_by_path` (the source spec's real blob SHA at HEAD via `get_blob_sha`); `restart_rebuilds_spec_link_graph_from_persisted_table` asserts it is non-empty.
- **Port + adapters**: `gyre_ports::SpecLinkRepository` (`list_all`, `replace_for_source` atomic delete+insert, `save` upsert-by-id, `delete_by_source_repo`), SQLite + Postgres + Mem implementations; wired via `store!` into `AppState.spec_link_repo`. SQLite round-trip tests cover every `SpecLinkType` variant and all `Option` columns in both `Some`/`None` forms (4 passed), Mem contract tests mirror the contract (3 passed). PG compile-verified in the same crate build (no PG test infra in repo — same convention as other PG adapters).
- **Write-through**: `sync_spec_ledger` issues `replace_for_source` per manifest spec including empty link sets (removed links deleted from the table, no resurrection — `sync_clears_durable_rows_when_links_removed_from_manifest`); staleness transitions and cross-workspace re-resolution in `spec_link_staleness.rs` call `save` (errors logged, not silent); repo deletion in `api/repos.rs` calls `delete_by_source_repo`.
- **Boot load**: `load_spec_links_into_store` populates the hot cache from `list_all` in both `build_state` and the mem constructor; read failure logged at error level. `restart_rebuilds_spec_link_graph_from_persisted_table` proves the graph (and a fresh storage handle on the same DB file) survives without re-push.
- **Gates**: `check-arch.sh` PASS, `check-in-memory-state-stores.sh` PASS (SpecLinksStore retained as documented hot cache of the durable table per the task's own instruction), exemption-file edits are renumbers plus one entry *removal* (shrink, not growth), `web/dist` byte-identical to base.

Focused probes: `spec_registry::tests` 45 passed / 0 failed (incl. `restart_rebuilds…`, `sync_replaces…`, `sync_clears…`), `sqlite::spec_links` 4/4, `mem::spec_link_contract_tests` 3/3. Evidence: `/tmp/stage/review-evidence/03-focused-suites.txt`.

### Findings

- [ ] **F1 (blocking, test): section-closing restart-durability test `staleness_query_parity_after_restart` is dead code — it never runs, yet is claimed as shipped evidence.** `crates/gyre-server/src/spec_registry.rs:2414` defines `async fn staleness_query_parity_after_restart()` with **no `#[tokio::test]` attribute** (the neighboring tests at lines 2262/2302/2535/2662 all have it; line 2413 is a doc comment). Consequences, all reproduced:
  - `cargo test -p gyre-server --lib staleness_query_parity_after_restart` → `running 0 tests` (filter matches nothing).
  - `cargo test --no-run` emits `warning: function 'staleness_query_parity_after_restart' is never used`.
  - The 4-filter run cited in the Shipped notes as "46 passed; includes all four section-closing tests" actually yields **45 passed**, with this test contributing zero.
  - The Shipped claim "These fail against the pre-task empty-init behavior" is therefore false for this test — a test that never executes cannot fail against anything.
  - The acceptance criterion "assert … that a staleness/gate query returns the same result it would after a push" is covered **only** by this dead test: `restart_rebuilds_spec_link_graph_from_persisted_table` compares graph identity (status included) and `source_sha`, but never runs the staleness checker against the restarted graph.

  Temporary probe (fully restored; working tree verified identical to the candidate afterward): adding the missing `#[tokio::test]` makes the test run and **pass** in isolation — the body is sound and mutation-sensitive (real `spec_link_staleness::run_once` against a restarted graph, verdict re-derived from ledger SHA vs persisted pinned SHA, durable write-through asserted). Evidence: `/tmp/stage/review-evidence/01-dead-parity-test.txt`, `/tmp/stage/review-evidence/02-dead-test-body-passes-when-enabled.txt`.

  Remedy: add the one missing attribute line, re-run the focused suites, and correct the shipped evidence so the pass counts and fail-on-main claims match reality. With that fix this review would approve — no other defects were found.
