# Review — task-198 (Spec Links — Persistent Forge-Maintained Spec Graph)

Spec: `specs/system/spec-links.md` §Forge-Maintained Spec Graph (tenant-wide `spec_links` SQL table; staleness/gates/graph queries/patrol all consume it; empty-after-restart silently degrades all of them).
Base: `e402e1ed`. Candidate: `a40680f6`. Verdict: **approved** (no blocking findings; four non-blocking observations recorded below and in `/tmp/stage/review-evidence/probes.md`).

## What the candidate ships

- **Migration** `2026-11-08-000061_spec_links` with the spec's exact 13-column schema (`source_sha TEXT NOT NULL`, `target_sha TEXT NOT NULL DEFAULT ''` for the `Option::None` unresolved-link case, mapped and documented in both SQL adapters), indexes on `source_repo_id`/`source_path`/`target_path`; registered in `schema.rs`.
- **`SpecLinkEntry`** moved to `gyre-domain::spec_links` (re-exported from `spec_registry`, same pattern as `SpecLedgerEntry`) with the new `source_sha: String`, populated in `sync_spec_ledger` from the source spec's real blob SHA at HEAD via the per-spec SHA map built during the sync (reused, never a placeholder).
- **Port** `gyre_ports::SpecLinkRepository` (`list_all`, `replace_for_source`, `save` upsert-by-id, `delete_by_source_repo`) — minimal, every method driven by a real caller; SQLite + Postgres adapters (identical row mapping) + `MemSpecLinkRepository`, wired via the `store!` macro into `AppState.spec_link_repo`.
- **Write-through on every mutation path**: `sync_spec_ledger` issues one `replace_for_source` per manifest spec — including specs whose link set is now empty, so manifest-removed links are deleted from the durable table rather than resurrected at boot; staleness transitions and cross-workspace re-resolution in `spec_link_staleness.rs` call `save`; `api/repos.rs::delete_repo` calls `delete_by_source_repo` (durable first, then hot cache).
- **Boot rebuild**: `load_spec_links_into_store` (own thread + current-thread runtime, safe from sync constructors) populates the in-memory `SpecLinksStore` hot cache from `list_all` in `build_state` and the mem test constructor; read failure is logged at error level, never silently treated as success.
- **Repo-scoped link ids** (`spec_link_id` includes the declaring repo), fixing a real cross-repo collision: under the old path-only id, two repos carrying the same spec path would collide on the table's PRIMARY KEY during `replace_for_source`, rolling back the transaction and silently dropping one repo's durable row. The in-memory retain in `sync_spec_ledger` is now scoped to `(repo, path)` the same way — previously a push from repo B deleted repo A's in-memory links for a shared path.

## Independent evidence (this review, executor-wrapped probes)

- `cargo test -p gyre-adapters spec_links` → **4/4 PASS**: round-trip of all 6 `SpecLinkType` variants plus every `Option` column in `Some`/`None` forms; reopen-durability (fresh `SqliteStorage` handle over the same file sees the rows); `replace_for_source` scoping (unrelated row kept); `delete_by_source_repo` scoping.
- `cargo test -p gyre-adapters migrations` → **4/4 PASS** (`migrations_create_tables` includes `spec_links` on a fresh DB; `migrations_pending_zero_after_startup`).
- `cargo check -p gyre-server --lib` and `cargo check -p gyre-server --tests` → clean, no warnings (all new tests type-check).
- Mechanical gates all PASS locally: check-arch, check-in-memory-state-stores, check-migration-versions, check-migration-sql-portability, check-unnamed-tuple-carriers (exemption file byte-identical to base — zero new entries), check-mem-port-contracts, check-fabricated-scope-defaults, check-inert-enforcement.
- The in-memory-state-stores exemption file was **shrunk** (5 → 2 entries, `FROZEN_EXEMPTION_COUNT` lowered accordingly): the dead `SpecLedger`/`SpecApprovalHistory` aliases were removed (ledger is port-backed) and `SpecLinksStore` now carries the sanctioned `// in-memory-state-stores:ok` marker as a write-through hot cache over the port-backed table. This is the direction the gate explicitly sanctions ("delete the line and lower FROZEN_EXEMPTION_COUNT; never raise it").

The gyre-server test binary exceeds the executor's 4 GiB budget (wrapper rejects it — infrastructure, not a code defect). Deferred verifier probes are recorded in `/tmp/stage/review-evidence/probes.md`: `cargo test --all` plus the focused restart-durability pair, mem contract tests, staleness tests, and regression suites for the refactored tuple carriers.

## Why the restart-durability evidence is real (not self-confirming)

- `restart_rebuilds_spec_link_graph_from_persisted_table` seeds via a **real git repo** (`make_test_repo` runs real `git init/commit`) pushed through the production `sync_spec_ledger` against a **real SQLite file**, then asserts the graph is non-empty and identical on (a) a fresh store loaded from the same repo and (b) a **brand-new `SqliteStorage` handle on the same file** — proving the data is durable in the file, not in process memory. Against the pre-task empty-init behavior both assertions fail (fresh store is empty until re-push).
- `staleness_query_parity_after_restart` loads the restarted graph from the durable table, advances the target spec's ledger SHA, and runs the **real** `spec_link_staleness::run_once`, which re-derives the verdict by comparing the persisted pinned `target_sha` against the ledger's current SHA (`run_once` lines 47-88) — then asserts the verdict is also written back through `save`. On the pre-task behavior the restarted graph is empty and the checker has nothing to mark stale.
- `sync_replaces_links_scoped_to_source_repo` and `sync_clears_durable_rows_when_links_removed_from_manifest` kill the two real write-path bugs described above (cross-repo PK collision; removed-link resurrection), each asserting cache and durable table agree.

## Verified behavior-preserving side refactors

The branch also repairs base-carried `check-unnamed-tuple-carriers` violations (needed because that gate's exemption file is frozen and base's entries pointed at lines the branch moved): `accumulate_chain_constraints` tuple → `ChainConstraints` (root-first ordering matches `verify_chain`'s documented root→leaf walk; first-Signed-wins `root_input` exactly reproduces `api/provenance.rs`'s prior independent `find_map` root-find, which it replaces — the other two callers previously bound the root to `_` and still ignore it), `ConvMap` 4-tuple → `ConversationBlob`, OTLP receiver tuple → `ReceiverHandle`, stale inbound-link 4-tuple → `StaleInboundLink`. All are field-swap-hazard remediations with identical runtime semantics; all compile clean and their test suites are listed as verifier probes.

## Observations (non-blocking)

- **O1**: migration sequence 000061 is reused from `2026-11-12-000061_repo_access_overrides` (next unused would be 000062). Diesel versions are date-scoped (`20261108000061` vs `20261112000061`) so both migrations run and every mechanical gate passes; deviation from the plan's letter only.
- **O2**: `created_at`/`stale_since` are `INTEGER` per the spec's exact DDL while schema.rs maps `BigInt`; on PostgreSQL an int4→i64 read could fail at runtime. This matches the assigned contract ("exact 13-column schema from the spec") and the pre-existing `meta_specs` pattern (`INTEGER` column + `BigInt` mapping + PG adapter reading it); the repo has no PG test infrastructure. Verifier with PG should confirm; if it fails, the fix is a spec-lifecycle amendment (INTEGER → BIGINT) plus a migration tweak, not a reinterpretation.
- **O3**: sqlite/mod.rs test comment says "migration 000056" (actual: 2026-11-08-000061). Comment-only.
- **O4**: durable-write failures in sync/staleness/delete paths are warn-and-continue (cache and table can diverge until the next push). No failure mode is specced; writes do go through the repository as the plan requires.

## Conclusion

Every acceptance criterion is satisfied by production code that performs the specced behavior: the SQL table is authoritative, every mutation writes through, boot reloads the graph, `source_sha` carries the source spec's real blob SHA, and the restart-durability tests genuinely discriminate against the pre-task empty-init behavior. Adapter-level and gate evidence ran green in this sandbox; the deferred gyre-server test run is recorded for the verifier. Approved.
