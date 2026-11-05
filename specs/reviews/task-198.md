# Review — task-198 (Spec Links — Persistent Forge-Maintained Spec Graph)

Spec: `specs/spec-links.md` §Forge-Maintained Spec Graph (persistent tenant-wide `spec_links` SQL table; `source_sha TEXT NOT NULL`; graph survives restart).
Candidate under review: `37067828658ff015a53cfa23b18c1609490ec739` (base `110180fbb248d6fa408492c0f9a3ec4a5a724372`); all 14 commits in the task's `commits:` frontmatter are ancestors of HEAD (`git merge-base --is-ancestor` per commit: OK ×14); `scripts/check-task-commit-attribution.sh` PASS.
Verdict: **approved** (interrupted prior round's compile-only log under `/tmp/stage/review-resume` re-validated from scratch; no prior verdict carried forward).

## What was verified (independent evidence, `/tmp/stage/review-evidence/`)

Diff inspection (base..HEAD, 29 files) plus focused probes, all run at the exact candidate with `/tmp/stage/bin/cargo` (workspace-wide invocations deferred to verification per assignment):

- `cargo test -p gyre-server --lib spec_registry::tests::` → **46 passed, 0 failed** (`spec-registry-tests.log`), including the four section-closing tests: `restart_rebuilds_spec_link_graph_from_persisted_table`, `staleness_query_parity_after_restart`, `sync_replaces_links_scoped_to_source_repo`, `sync_clears_durable_rows_when_links_removed_from_manifest`.
- `cargo test -p gyre-adapters --lib sqlite::spec_links` → **4 passed, 0 failed**; `mem::spec_link_contract_tests` → **3 passed**; `spec_link_staleness` → **6 passed**; `spec_patrol` → **19 passed** (`adapter-and-reader-tests.log`).
- Reader modules: `api::specs` → **72 passed**; `merge_processor` → **49 passed**; `api::graph` → **23 passed**; `mem::` → **13 passed**; `migrations_create_tables` → **1 passed** (now asserts `spec_links`) (`reader-module-tests.log`, `mem-tests.log`).
- Gate scripts at the candidate (`gate-scripts.log`): `check-arch.sh`, `check-migration-versions.sh`, `check-migration-sql-portability.sh`, `check-in-memory-state-stores.sh`, `check-fabricated-scope-defaults.sh`, `check-unnamed-tuple-carriers.sh`, `check-mem-port-contracts.sh`, `check-task-commit-attribution.sh` — all PASS.

## Why this closes the section (not fakery)

- **Schema**: migration `2026-10-08-000056_spec_links` creates all 13 spec columns with the spec's names/types/nullability (`source_sha TEXT NOT NULL`, `target_sha TEXT NOT NULL`, `status TEXT NOT NULL DEFAULT 'active'`), indexes on `source_repo_id`/`source_path`/`target_path`, registered in `schema.rs`; portability check passes (runs on both SQLite and PG).
- **`source_sha` is real**: populated in `sync_spec_ledger` from `manifest_sha_by_path` — the source spec's blob SHA computed by `get_blob_sha` at the pushed HEAD (same value the ledger entry uses). The restart test asserts it is non-empty from a real push. On git lookup failure it degrades to `""` with a `warn!` — mirroring the ledger's own `current_sha` handling, not a placeholder path.
- **SQL table is authoritative, write-through everywhere**: `sync_spec_ledger` → `replace_for_source` per manifest spec (including empty link sets, so manifest-removed links are deleted — `sync_clears_durable_rows_when_links_removed_from_manifest` proves no resurrection); staleness transitions and cross-workspace re-resolution in `spec_link_staleness.rs` → `save`; repo deletion (`api/repos.rs::delete_repo`) → `delete_by_source_repo`. The in-memory `SpecLinksStore` is updated alongside every durable write; readers keep their semantics (all reader modules green).
- **Boot rebuild is real and load-bearing**: `load_spec_links_into_store` (dedicated thread + own runtime, error-logged on failure) is called from both `build_state` (production; `spec_link_repo` wired via `store!` sharing the same `sqlite_db`/`pg_db` storage Arc as every other durable repo) and the mem/test constructor.
- **The restart-durability test kills the bug it claims to kill**: it pushes a real manifest through `sync_spec_ledger` against a real SQLite file, then builds a brand-new empty store and populates it **only** via the production loader. Under empty-init semantics (loader a no-op) the assertions `!after_restart.is_empty()` / `after_restart == before_restart` fail directly; the test additionally re-opens a fresh `SqliteStorage` handle on the same file and repeats, pinning the write path (file durability) independently of the load path. `staleness_query_parity_after_restart` goes further: it runs the real `spec_link_staleness::run_once` against the restarted graph, asserts the stale verdict plus `stale_since` is re-derived from the persisted pinned SHA vs the (independently updated) ledger, and asserts the verdict is written back through `save` — an empty restarted graph yields zero links and fails the assertions. Not self-confirming, not mirrored logic.
- **Cross-repo scoping fixed a real latent collision**: link ids are now repo-scoped (`spec_link_id`), and `replace_for_source` keys on `(source_repo_id, source_path)`; `sync_replaces_links_scoped_to_source_repo` proves two repos with the identical spec path both survive in cache, table, and after restart (the old unscoped id would have collided on the table's PRIMARY KEY).
- **Round-trips**: SQLite adapter tests cover every `SpecLinkType` variant and all `Option` columns in `Some`/`None` forms (`None` ↔ `''` documented in both SQL adapters), reopen-durability, replace scoping, repo-scoped delete; mem contract tests mirror the same. Postgres adapter is compile-verified with identical row mapping — the repo has no PG test infrastructure (zero test modules under `postgres/`), same convention as every other PG adapter.

## Verification-repair changes in the candidate (inspected, sound)

- `git_http.rs process_spec_lifecycle` no longer fabricates a `"default"` workspace on repo-lookup failure — workspace-scoped SpecChanged/TaskCreated emissions are skipped with `warn!` (the `emit_reconciliation_completed` pattern); the fabricated-scope exemption entry is deleted and `FROZEN_EXEMPTION_COUNT` lowered 5→4. Root fix, not an exemption shuffle; `check-fabricated-scope-defaults.sh` PASS.
- `otlp_receiver.rs` replaces the positional 3-tuple with a named `ReceiverHandle` struct (the check's own recommended remediation for the rustfmt-incompatible inline marker); semantics unchanged.
- `SpecLinksStore` alias dropped from `in-memory-state-stores` exemptions with an `in-memory-state-stores:ok` marker — legitimate: the store is now the hot cache of the port-backed `spec_links` table (count lowered 5→4, no additions); `check-in-memory-state-stores.sh` PASS.

## Non-blocking notes (recorded for completeness)

- The migration adds `DEFAULT ''` to `source_repo_id`, `source_sha`, and `target_sha` beyond the spec's literal SQL. All 13 columns, types, and NOT NULL constraints are exact, every adapter insert sets all columns explicitly, and the task text itself endorses `''` as the `Option::None` representation for `target_sha` — so no data path can silently rely on the defaults. Cosmetic deviation only.
- Durable-write failures in `sync_spec_ledger`/`spec_link_staleness` are logged (`warn!`) rather than propagated, so cache and table can diverge on a DB failure mid-push. Acceptable for push-path resilience (failing a whole push over the link table would be worse) and the divergence self-heals on the next push/boot load; noted as a possible future hardening.
- Postgres adapter is compile-verified only (no PG test infrastructure exists in the repo — established convention).
- Coverage matrix row for `spec-links.md` §5 remains `task-assigned` pending this review's approval; the section-closing evidence above supports promoting it.

## Deferred to verification (per assignment)

Full `cargo test --all` / all-target Clippy / full `scripts/check-*.sh` battery at the exact candidate; a live server-restart probe (build_state boot-load against a pre-seeded SQLite file over HTTP) was not needed — the loader is exercised directly by the section-closing tests against the same production code path.
