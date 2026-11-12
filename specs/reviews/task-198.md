# Review — task-198 (Spec Links — Persistent Forge-Maintained Spec Graph)

Spec: `specs/system/spec-links.md` §Forge-Maintained Spec Graph (coverage row 5).
Base `3afece9d7` → candidate `d25698ceb` (includes rebase repair `b14d3748c` for durable finding 81c434f1).
Verdict: **approved** — no blocking findings. The persistence is real, wired on every mutation path, and the restart-durability evidence is a genuine kill-test suite.

## Probes run (checkout at `d25698ceb`; working tree carries only the controller's staged task-contract revert)

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → exit 0 (7m09s).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` → exit 0 (3m30s) — all test targets, including the Postgres adapter (unconditionally compiled) and every touched `SpecLinksStore` reader, type-check.
- `cargo test -p gyre-adapters spec_links` → **4 passed** (`round_trips_every_link_type_and_option_columns`, `links_survive_reopened_database`, `replace_for_source_swaps_link_set`, `delete_by_source_repo_scopes_to_repo`) — direct evidence the migration creates the table on a fresh SQLite file and rows survive a reopened storage handle.
- `cargo test -p gyre-domain` → 474 passed.
- `bash scripts/check-arch.sh` → OK (SpecLinkEntry moved to `gyre-domain::spec_links`, re-exported from `spec_registry` — hexagonal boundary intact).
- `bash scripts/check-migration-versions.sh` → OK. Verified the full diesel version of `2026-11-08-000061_spec_links` is `20261108000061` — distinct from main's `2026-10-08-000061_dep_breaking_policies` (`20261008000061`); no silent drop.
- `bash scripts/check-in-memory-state-stores.sh`, `check-unnamed-tuple-carriers.sh`, `check-migration-sql-portability.sh`, `check-mem-port-contracts.sh` → all OK.
- Full `scripts/check-*.sh` sweep on candidate vs. a throwaway worktree of base `3afece9d7`: every advisory check that fails on the candidate fails identically on base (pre-existing, `|| true` in CI). All CI-hard-enforced checks (arch, hierarchy, abac-route-registry, migration-versions, byte-slice-truncation, dead-message-kinds, sandbox-sweep, no-em-dash, api-auth) pass on the candidate.
- `bash scripts/check-task-commit-attribution.sh` → **passes at the candidate commit** (verified by temporarily restoring the committed task file, then re-applying the controller's staged revert byte-for-byte). The failure seen on the working tree is purely the controller's uncommitted `commits: []` staging, not a candidate defect.
- Grep sweep of all `spec_links_store` mutations outside `mod tests`: every one persists (see below). Worktree removed; working tree restored (`git status` shows only the controller's staged task-file revert).

## Verified working

- **Migration `2026-11-08-000061_spec_links`** carries the spec's 13 columns with correct nullability (`source_sha TEXT NOT NULL`, `target_sha TEXT NOT NULL`, `status TEXT NOT NULL DEFAULT 'active'`) plus indexes on `source_repo_id`, `source_path`, `target_path`; registered in `schema.rs` and `allow_tables_to_appear_in_same_query!`. The `DEFAULT ''` additions on three NOT NULL columns are a documented hardening for the `Option`-column mapping, not a schema violation. Portability: `CREATE TABLE/Index IF NOT EXISTS` — valid on both SQLite and PG; `INTEGER` + diesel `BigInt` matches the existing repo convention (`conversations` et al.).
- **`source_sha` is real**: `sync_spec_ledger` records each manifest spec's HEAD blob SHA in `manifest_sha_by_path` (the same SHA computed for the ledger entry) and stamps it on the entry; the restart test asserts non-empty. Degenerate case (missing blob → `""` + warn) only occurs when the manifest references a nonexistent file.
- **Port + adapters**: `gyre_ports::SpecLinkRepository` has exactly the four specced methods; SQLite and Postgres adapters follow the `spec_ledger.rs` pattern (spawn_blocking, transactional delete+insert in `replace_for_source`, upsert-by-id in `save`); `MemSpecLinkRepository` mirrors the same `(source_repo_id, source_path)` scoping contract. `None ↔ ''` mapping for `source_repo_id`/`target_sha` documented in both SQL adapters.
- **SQL table is authoritative on every write path** (grep-verified, no non-test mutation bypasses the repo): `sync_spec_ledger` → per-spec `replace_for_source` including empty link sets (removed links are deleted, not resurrected on boot); `spec_link_staleness.rs` `mark_link_status` + cross-workspace re-resolution → `save`; `api/repos.rs` `delete_repo` → `delete_by_source_repo` (durable first, then cache). All three sync callers (`git_http.rs`, `mirror_sync.rs`, `api/repos.rs`) pass the repo through.
- **Boot rebuild**: `load_spec_links_into_store` runs in `build_state` after the storage handle exists (migrations run in `SqliteStorage::new`/`PgStorage` construction, so the table exists before `list_all`) and in the mem test constructor; it joins before `AppState` is returned, and surfaces read failure at error level instead of silently treating it as "no links".
- **Restart-durability tests are genuine kill-tests** (each fails if the corresponding production behavior is disabled):
  - `restart_rebuilds_spec_link_graph_from_persisted_table` — real `sync_spec_ledger` push against a real SQLite file, fresh store + fresh storage handle on the same file, equality asserts against the pre-restart graph. Fails on empty-init (no load), on missing write-through (empty table), and on unpopulated `source_sha`.
  - `staleness_query_parity_after_restart` — drives the production `spec_link_staleness::run_once` against a restarted graph after the target's ledger SHA advances; asserts stale re-derivation AND write-through of the verdict to the durable table. Fails on empty-init and on missing persistence.
  - `sync_replaces_links_scoped_to_source_repo` — kills the repo-scoped id-collision bug (two repos, same spec path; ids now scoped by declaring repo via `spec_link_id`).
  - `sync_clears_durable_rows_when_links_removed_from_manifest` — kills link resurrection on boot.
- **Adapter round-trip**: SQLite test covers all six `SpecLinkType` variants and every `Option` column in both `Some`/`None` forms (ran, passed); the Mem contract tests mirror it (type-checked; deferred to the verifier's full run).
- **Rebase repair** (`b14d3748c`) resolved the `sqlite/mod.rs` `migrations_create_all_tables` conflict correctly — both task-198's `spec_links` and task-163's `breaking_changes`/`dependency_policies` entries kept.
- **Gate hygiene**: `in-memory-state-stores` exemptions reduced 5→2 with the frozen count lowered to match (a tightening); the surviving `SpecLinksStore` alias carries a legitimate inline `in-memory-state-stores:ok` marker backed by real write-through + boot reload — exactly the hot-cache design the task mandates. Tuple-carrier exemptions reduced (4-tuple replaced by the `StaleInboundLink` named struct); the two renumbered entries point at the same pre-existing tuples (`ConvMap`, `accumulate_chain_constraints`) whose old line numbers had gone stale on main. The `otlp_receiver.rs` `ReceiverHandle` refactor is behavior-preserving and removes a tuple-carrier hit without a new exemption.
- **Coverage matrix** row 5 → `implemented` with accurate notes (pending this review).

## Non-blocking observations (recorded, no action required)

- `crates/gyre-adapters/src/sqlite/mod.rs` test comment says "migration 000056" for the spec_links table; the actual migration is `2026-11-08-000061_spec_links`. Comment nit only.
- The Postgres adapter has no unit tests (no PG server in the test environment) — same as the existing `postgres/spec_ledger.rs` pattern; it is compiled and type-checked by `cargo check`.
- `replace_for_source` issues one transaction per manifest spec per push (N transactions for an N-spec manifest) — a performance nit, not a correctness issue.
- On a `get_blob_sha` failure (manifest references a missing spec file), `source_sha` is stored as `""` with a warn — degenerate input, NOT NULL still satisfied.

## Deferred to the independent verifier (server test compilation exceeds the review budget)

- `cargo test --all` (full suite, both Rust and `web && npm test`).
- Focused server tests for this task: `cargo test -p gyre-server restart_rebuilds_spec_link_graph_from_persisted_table staleness_query_parity_after_restart` and `cargo test -p gyre-server sync_replaces_links_scoped_to_source_repo sync_clears_durable_rows_when_links_removed_from_manifest` and `cargo test -p gyre-server spec_link_contract_tests` — all type-check clean under `cargo check -p gyre-server --tests`.
- CI rustfmt/clippy diff gates on the merged head.

Test-inflation check: none found. Every new test fails if the production behavior it names is disabled; no assertionless, conditional-guard, or mirrored-logic tests were added.
