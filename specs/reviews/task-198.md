# Review — task-198 (Spec Links — Persistent Forge-Maintained Spec Graph)

Spec: `spec-links.md` §Forge-Maintained Spec Graph.
Candidate under review: `b78aec96` (base `5196f444`). This round re-verified the
three durable findings carried forward from earlier task-198 review rounds
(dead test registration; cross-repo link-id collision with divergent-id test
seeding; self-confirming trailing assert in the parity test) and the full task
contract.

Verdict: **approved**.

## Probes (all at candidate `b78aec96`, evidence under /tmp/stage/review-evidence)

- `cargo test -p gyre-server --lib spec_registry::tests` — 46 passed, 0 failed,
  including all four section-closing tests (01).
- `cargo test -p gyre-server --lib staleness_query_parity_after_restart` —
  1 passed (test is genuinely registered with `#[tokio::test]`; durable finding
  "dead code" fixed by 2736e818) (02).
- `cargo test -p gyre-adapters --lib sqlite::spec_links` — 4 passed, 0 failed
  (round-trip over every `SpecLinkType` variant + all `Option` columns in
  Some/None forms, reopened-database durability, replace scoping, repo-scoped
  delete) (03).
- `cargo test -p gyre-server --lib mem::spec_link_contract_tests` — 3 passed
  (04).
- Reader modules: `spec_link_staleness` (6), `spec_patrol` (19), `api::graph`
  (23), `api::specs` (72), `merge_processor` (49), `migrations_create_tables`
  (1) — all 0 failed (10–13).
- Invariant checks at HEAD: `check-arch.sh`, `check-in-memory-state-stores.sh`,
  `check-migration-sql-portability.sh`, `check-mem-port-contracts.sh`,
  `check-task-commit-attribution.sh` — all PASS (05–09).

## Mutation probes (proof the section-closing tests are not self-confirming)

- **No-boot-load mutation** (temporarily made `load_spec_links_into_store`
  skip the load, simulating the pre-task empty-init; fully restored):
  `restart_rebuilds_spec_link_graph_from_persisted_table`,
  `staleness_query_parity_after_restart`, and the restart leg of
  `sync_replaces_links_scoped_to_source_repo` all FAIL — the tests genuinely
  detect the restart-degradation bug this task kills (14, exit 101).
- **Unscoped-id mutation** (temporarily reverted `spec_link_id` to the
  pre-fix `{path}:{type}:{target}` format with no repo scope; fully restored):
  `sync_replaces_links_scoped_to_source_repo` FAILS on the distinct-ids assert
  — the collision regression test genuinely covers the prior durable finding
  (15, exit 101).
- Working tree verified clean after both mutations (source edits restored;
  the `web/dist` dirt observed mid-review was the build script's environmental
  rebuild on a fresh sandbox, as the Shipped notes predicted — committed
  `web/dist` is byte-identical to base: `git diff 5196f444..HEAD -- web/dist`
  is empty).

## Verified against the durable findings

1. **Cross-repo id collision — fixed.** Link ids are now built by
   `gyre_domain::spec_links::spec_link_id(source_repo_id, path, type, target)`
   (repo-scoped), used at both production construction sites
   (`spec_registry.rs:413`, `api/specs.rs:2826` test). With the scoped id,
   `replace_for_source`'s `(source_repo_id, source_path)` delete can no longer
   miss another repo's row ahead of a same-id INSERT; `save`'s upsert-by-id is
   consistent with the same scoping. All three adapters (SQLite, Postgres, Mem)
   use identical `(source_repo_id, source_path)` scoping.
2. **Divergent-id test seeding — fixed.** `sync_replaces_links_scoped_to_source_repo`
   now performs two REAL `sync_spec_ledger` pushes (repo-a then repo-b, same
   manifest) using the production id construction, and asserts cache/table
   identity plus a third-boot reload of both repos' links.
3. **Self-confirming trailing assert — fixed.** The parity test's final block
   now drifts the ledger AFTER the restart and runs the REAL
   `spec_link_staleness::run_once` against the restarted graph, asserting
   stale + `stale_since` and that the verdict is written through to the
   durable table (also proven load-sensitive by the no-boot-load mutation).

## Verified against the task contract

- Migration `2026-10-08-000056_spec_links` with the exact 13-column spec schema
  (`source_sha TEXT NOT NULL DEFAULT ''`, `target_sha NOT NULL DEFAULT ''`
  documented as the `Option::None` mapping in both SQL adapters), indexes on
  `source_repo_id`/`source_path`/`target_path`, registered in `schema.rs`,
  portable SQL (check PASS).
- `source_sha` populated from `manifest_sha_by_path` — real blob SHAs computed
  from the pushed tree during sync (spec_registry.rs:321-336 → 484); the
  restart test asserts it is non-empty after a real push.
- `SpecLinkRepository` port (list_all / replace_for_source / save /
  delete_by_source_repo) + SQLite + Postgres + Mem adapters, wired via the
  `store!` macro; boot load in `build_state` (lib.rs:868-873) and the mem
  constructor (mem.rs:3419-3422). No production code mutates the in-memory
  store without the durable write-through (all direct `spec_links_store` pushes
  are inside `#[cfg(test)]` modules); repo deletion calls
  `delete_by_source_repo` (api/repos.rs:366).
- Exemption files unchanged in substance: `in-memory-state-stores` went 5 → 4
  (an entry REMOVED with the frozen count lowered to match — the SpecLinksStore
  alias is now an inline-marked hot cache over a durable port-backed repo);
  other files renumbered for line drift only, no entries added.
- Non-blocking note for future work: durable-write failures in
  `sync_spec_ledger`/staleness paths are logged (`warn!`) and skipped, so the
  hot cache and table can diverge until the next push — consistent with the
  pre-existing `ledger.save` error pattern and visible in logs; not a
  contract violation for this task.

`cargo test --all` (full workspace) remains owned by the verification stage per
the assignment.
