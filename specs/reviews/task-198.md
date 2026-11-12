# Review — task-198 (Spec Links — Persistent Forge-Maintained Spec Graph)

Spec: `specs/system/spec-links.md` §Forge-Maintained Spec Graph.
Candidate: `bf6940793d93ff2ecf9ad18bc4f917b260476006` (base `f98b179253b9e3f12cbc9a165532cacf709244d9`).
Verdict: **approved** — the section-closing behavior is real, durable, and independently probed; no blocking findings.

## What was reviewed

Diff `f98b179..bf69407` (30 files; the task surface plus gate-required repairs), full read of:
migration + schema, `gyre-ports/src/spec_link_repo.rs`, `gyre-domain/src/spec_links.rs`,
SQLite/Postgres/Mem adapters, `spec_registry.rs` (sync write-through + 4 new tests),
`lib.rs` (store! wiring + `load_spec_links_into_store`), `spec_link_staleness.rs`,
`api/repos.rs` (repo deletion), and every remaining `SpecLinksStore` touch point
(`api/specs.rs`, `api/graph.rs`, `merge_processor.rs`, `spec_patrol.rs`, `git_http.rs`,
`mirror_sync.rs`, `middleware.rs`, `mem.rs`).

## Probes run (evidence: /tmp/stage/review-evidence)

- `bash scripts/check-migration-versions.sh` → OK ( Diesel version `20261108000061` is
  distinct from `20261112000061`; no duplicates).
- `bash scripts/check-migration-sql-portability.sh` → OK (portable DDL only).
- `bash scripts/check-in-memory-state-stores.sh` → OK (exemption count 5→2 is the
  sanctioned remediation — the script itself says "delete the line and lower
  FROZEN_EXEMPTION_COUNT; never raise it"; the remaining `SpecLinksStore` alias carries
  the `in-memory-state-stores:ok` hot-cache marker backed by the durable table).
- `bash scripts/check-unnamed-tuple-carriers.sh` → OK (exemptions file byte-identical to
  base; flagged carriers were fixed with named structs, not re-pointed).
- `bash scripts/check-arch.sh` → OK. `bash scripts/check-mem-port-contracts.sh` → OK.
- `cargo test -p gyre-adapters spec_links` → **4/4 pass** (round-trip of every
  `SpecLinkType` + all `Option` columns in Some/None forms, reopen-durability on a fresh
  `SqliteStorage` handle over the same file, replace scoping, repo-scoped delete).
- `cargo test -p gyre-adapters migrations` → **4/4 pass** (`migrations_create_tables`
  now asserts `spec_links`; pending-migrations-zero after startup).
- `cargo check -p gyre-server --tests` → **pass** (server + full test target,
  incl. the new restart-durability tests; web build via build.rs succeeded).
- Commit attribution: every non-process task-198 product commit is in the candidate's
  `commits:` frontmatter (verified against `git show bf69407:specs/tasks/task-198.md`;
  the working-tree failure of `check-task-commit-attribution.sh` is the controller's
  contract-restore of `specs/tasks/task-198.md`, not candidate content).

## Verified working

- **Durable graph is real.** Migration `2026-11-08-000061_spec_links` creates all 13
  spec columns (`source_sha TEXT NOT NULL` included; `target_sha` NOT NULL stores `''`
  for the `Option::None` unresolved-link case — mapping documented in both SQL
  adapters), indexes on `source_repo_id`/`source_path`/`target_path`, registered in
  `schema.rs`; both SQLite and PG run the shared embedded migrations at startup.
- **Write-through is complete.** Audited every non-test mutation of
  `spec_links_store`: `sync_spec_ledger` → one `replace_for_source` per manifest spec
  (empty sets included, so manifest-removed links are deleted, not resurrected on boot);
  `spec_link_staleness.rs` `mark_link_status` + cross-workspace re-resolution → `save`;
  `api/repos.rs::delete_repo` → `delete_by_source_repo` (durable first, then cache).
  All other uses (`api/specs.rs`, `api/graph.rs`, `merge_processor.rs`,
  `spec_patrol.rs`) are read-only against the hot cache — reader semantics unchanged,
  as instructed.
- **Boot rebuild is real.** `load_spec_links_into_store` runs in `build_state` (after
  the `store!`-selected SQLite/PG/Mem repository is wired) and in the mem/test
  constructor; read failure is logged at error level. The restart tests additionally
  reopen a brand-new `SqliteStorage` handle on the same file, killing the
  same-handle-cache loophole.
- **Restart-durability evidence is genuine, not self-confirming.**
  `restart_rebuilds_spec_link_graph_from_persisted_table` seeds via a real
  `sync_spec_ledger` push against a real SQLite file, then asserts a fresh store loaded
  via `list_all` is identical pre/post restart; `staleness_query_parity_after_restart`
  runs the production `spec_link_staleness::run_once` against the restarted graph and
  asserts the stale verdict is re-derived and persisted. Both fail trivially against the
  base empty-init behavior (no persistence, no load path existed).
- **Cross-repo id scoping.** `spec_link_id` prefixes the declaring repo, so two repos
  with the same spec path never collide in the durable table
  (`sync_replaces_links_scoped_to_source_repo` proves both rows survive a push from the
  other repo and a subsequent restart).
- **`source_sha` is real.** Populated from the manifest SHA map computed at HEAD during
  sync (same blob SHA used for the ledger entry); the restart test asserts it is
  non-empty.
- **Gate repairs are the sanctioned remediation path.** Tuple carriers → named structs
  (`StaleInboundLink`, `ConversationBlob`, `ChainConstraints`, `ReceiverHandle`) fixed
  sites whose exemption lines were invalidated by line shifts, exactly as
  `check-unnamed-tuple-carriers.sh` demands ("refactor to named structs when touched");
  the exemptions files were restored byte-identical to base, never grown.

## Non-blocking observations (recorded for completeness)

- The migration adds `DEFAULT ''` to `source_repo_id`/`source_sha`/`target_sha` beyond
  the spec's literal DDL. Additive only — column set, types, and nullability match the
  spec exactly; adapters always supply values.
- If the source blob SHA lookup fails during sync, `source_sha` degenerates to `''`
  (same pre-existing failure mode as the ledger's `current_sha` in that path).
- Durable-write failures in sync/staleness/delete paths warn and continue (cache/table
  can diverge until the next push) — consistent with the existing ledger save handling
  in the same function.
- `crates/gyre-adapters/src/sqlite/mod.rs` test comment says "migration 000056" but the
  actual migration is `2026-11-08-000061` (cosmetic).
- `accumulate_chain_constraints` root-input semantics changed from last-Signed-wins to
  first-Signed-wins, unifying with `api/provenance.rs`'s existing first-wins root find;
  behavior-preserving for valid single-root chains. Bundled gate repair, documented in
  commit `bdbccb488`.
- Postgres adapter is compile-verified only (no PG test infrastructure in the repo —
  same convention as every sibling PG adapter); row mapping is identical to SQLite.

## Deferred to the independent verifier (build-wrapper constraints)

- `cargo test --all`, in particular the gyre-server targets:
  `spec_registry::tests::{restart_rebuilds_spec_link_graph_from_persisted_table,
  staleness_query_parity_after_restart, sync_replaces_links_scoped_to_source_repo,
  sync_clears_durable_rows_when_links_removed_from_manifest}`,
  `mem::spec_link_contract_tests`, `spec_link_staleness`, `api::specs`,
  `api::graph`, `merge_processor`, `spec_patrol`.
- `bash scripts/checks.sh` (full gate set) and the web suite.

Test-inflation check: none found. The adapter round-trips assert field-by-field
equality against constructed entries; the restart tests assert graph equality across a
fresh handle and re-derive a staleness verdict through the production checker; the
cross-repo and removal tests assert both cache and table state.
