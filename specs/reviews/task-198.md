# Review — task-198 (Spec Links — Persistent Forge-Maintained Spec Graph)

Spec: `specs/system/spec-links.md` §Forge-Maintained Spec Graph.
Candidate under review: `717f7f0006258121c83d4d491565ec134fd2a816` (base `cfeefbda975211a6ac24a17e5920be6f8b23008b`).
Verdict: **approved** — the section-closing implementation is real and complete; all runtime evidence that this sandbox's transport restriction blocked is deferred to the independent verifier (list below).

## Probes run (exit codes in /tmp/stage/review-evidence/)

- `bash scripts/check-arch.sh` → exit 0.
- `bash scripts/check-migration-versions.sh` → exit 0 (000061 is the next unused sequence; 000060 is taken by an older migration).
- `bash scripts/check-migration-sql-portability.sh` → exit 0 (up.sql is dialect-neutral).
- `bash scripts/check-in-memory-state-stores.sh` → exit 0. The SpecLinksStore alias now carries the `in-memory-state-stores:ok` marker (port-backed hot cache), the exemption entry was deleted, and `FROZEN_EXEMPTION_COUNT` was lowered 5→4 — exactly the script's documented fix path; no exemption grew.
- `check-unnamed-tuple-carriers`, `check-fabricated-scope-defaults`, `check-scope-literal-defaults`, `check-byte-slice-truncation`, `check-inert-enforcement`, `check-lossy-secret-conversion`, `check-fail-open-ref-resolution`, `check-relative-path-defaults` → all exit 0.
- Rust compilation/test probes **blocked by sandbox transport restriction**: crates.io unreachable (connection refused) and no cached crate sources — infrastructure, not a code defect (evidence: `network-restriction.txt`). Deferred probes recorded below and in `summary.md`.

## Verified by full diff review (base..candidate, every hunk)

- **Migration** `2026-11-08-000061_spec_links`: all 13 spec columns with correct names/types/nullability/PK, `status TEXT NOT NULL DEFAULT 'active'`, indexes on `source_repo_id`, `source_path`, `target_path`; registered in `schema.rs` and the sqlite table-list test.
- **Port + adapters**: `gyre_ports::SpecLinkRepository` with exactly the four specced methods; SQLite and Postgres adapters mirror the proven `spec_ledger.rs` patterns (spawn_blocking, transaction-wrapped replace, `on_conflict(id)` upsert); Mem adapter enforces the same `(source_repo_id, source_path)` scoping. The `None ↔ ''` mapping for NOT NULL `source_repo_id`/`target_sha` is documented in both SQL adapters.
- **Write-through is complete**: every production mutation of `spec_links_store` persists — `sync_spec_ledger` (per-spec `replace_for_source`, including empty link sets so removed links are deleted from the table), `spec_link_staleness.rs` status transitions and cross-workspace re-resolution (`save`), `api/repos.rs::delete_repo` (`delete_by_source_repo`). Grep of every `spec_links_store.lock()` site confirms all other `mut` uses are test-only; production readers (api/specs.rs, api/graph.rs, merge_processor.rs, spec_patrol.rs) are unchanged reads, as the task instructed.
- **Boot rebuild**: `load_spec_links_into_store` runs in `build_state` (store!-wired SQLite/Postgres repo) and the mem test constructor; read failure is logged at error level, not masquerading as an empty graph. All AppState constructors covered.
- **source_sha** is populated from the source spec's real blob SHA at HEAD (reused from the manifest SHA map); the only empty-string path is the pre-existing `get_blob_sha` failure branch, which warns — the same degradation the ledger itself has.
- **The restart-durability tests are genuine, not self-confirming**: real git fixtures, a real `SqliteStorage` over a temp file (migrations run in the constructor), the real `sync_spec_ledger`, a fresh store + production loader, a fresh storage handle on the same DB file, and the real `spec_link_staleness::run_once` re-deriving `stale` from the persisted pinned SHA vs the ledger's current SHA — with the verdict asserted written back to the table. Against the pre-task empty-init behavior these assertions fail. Extra regressions cover the repo-scoped id collision (same spec path pushed by two repos) and manifest link removal clearing durable rows.
- **Attribution**: all non-merge task-198-labeled product-surface commits in base..candidate are recorded in the candidate's `commits:` frontmatter; the unrecorded labeled commits are two-parent merges/rebase-merges, which the gate excludes by rule.
- **Unrelated hunks explained**: `otlp_receiver.rs` (tuple→struct refactor) and `signal_chain.rs` (test builder field) are base-churn rebase repairs — mechanical, no behavior change.

## Deferred probes the verifier must run (transport-blocked in this review sandbox)

1. `cargo test --all` (AC).
2. `cargo test -p gyre-adapters --lib spec_link`.
3. `cargo test -p gyre-server --lib spec_registry` (restart-durability + regression tests).
4. `cargo test -p gyre-server --lib spec_link` (staleness write-through, mem contract tests).
5. `cargo check -p gyre-server`.
6. `bash scripts/check-task-commit-attribution.sh` against a checkout of `717f7f000` (its tree carries the full 20-SHA frontmatter; this review's working tree had the executor's assigned-contract restore with `commits: []`, which would fail the gate spuriously).

## Non-blocking observations

- The migration adds `DEFAULT ''` to three NOT NULL text columns beyond the spec's literal DDL; all specced columns/constraints match exactly and the defaults are never relied on (inserts always supply values). Compliant in substance.
- `sqlite/mod.rs` test comment says "migration 000056" where it means 000061 (stale comment).
- Durable-write failures in sync/staleness paths warn and continue (cache updated, table not) — logged divergence consistent with sibling adapters, not silently permissive.

Test-inflation check: none found — every new test drives production code (real git, real SQLite, real loader, real staleness checker) and fails against the pre-task empty-init behavior.
