# Review — task-163: Dependency graph persistent storage

- **Candidate**: `3ea1057ce306d8215cf1589224d4366add59049c`
- **Base**: `dbf9edaeae5c8a5441bd9ff50c9dff3cec99172a`
- **Round**: 9 (independent review)
- **Verdict**: approved

## What was reviewed

The diff from the assigned base touches 13 files, all on task-163-owned surface:
the two SQLite adapters (`sqlite/breaking_change.rs`, `sqlite/dependency_policy.rs`),
their Postgres mirrors, `schema.rs` table definitions, migration
`2026-10-08-000060_dep_breaking_policies`, the `store!` wiring in `lib.rs`
(10 lines), the SCIM test-token race fix (round 4, test-only), the
`task163_dependency_persistence.rs` integration tests, and this task's file.
`web/` is untouched vs base; no scripts or spec-contract changes.

## Independent evidence

1. **Code inspection**: every port method is a real Diesel query against the
   r2d2 pool via `spawn_blocking`. `acknowledge` returns `count > 0` for
   missing ids; the policy adapter upserts on `workspace_id` and *rejects*
   unknown behavior strings rather than defaulting; a missing policy row
   returns the domain default per the port contract. Postgres mirrors are
   logic-identical. `schema.rs` matches the migration DDL column-for-column.
   The `store!` wiring places both stores behind SqliteStorage/PgStorage in
   DB-backed mode with mem fallback only in pure in-memory mode — identical to
   every sibling store, and all production consumers
   (`git_http.rs`, `api/dependencies.rs`, `merge_processor.rs::trigger_cascade_tests`)
   route through the wired fields.
2. **Static gates at HEAD** (all exit 0): migration-versions,
   migration-sql-portability, arch, mem-port-contracts, in-memory-state-stores,
   task-commit-attribution, abac-route-registry, dead-message-kinds,
   inert-enforcement, `check-rustfmt-diff.py dbf9edae` (10 files clean),
   `git diff --check dbf9edae..HEAD` clean.
3. **Deterministic migration un-shadowing probe** (sqlite3, no cargo): all 68
   `up.sql` files applied in Diesel version order to a fresh DB —
   `breaking_changes` and `dependency_policies` created with full column sets,
   and every previously-collided upstream table (`spec_lifecycle_configs`,
   `spec_assertion_results`, `spec_approval_events` with its round-7 columns)
   remains intact. No shadowing at version 000060.
4. **Test strength**: the integration test is a persistence killer — records
   written through one `build_state` must be observed by a *fresh* `build_state`
   over the same DB file, which fails against mem-only wiring. The adapter unit
   suites include the fresh-storage-instance variant of the same kill, plus
   scoping, pre-acknowledged-field preservation, and acknowledge-missing→false.
5. **Contract integrity**: acceptance criterion 6 reads verbatim
   `- [ ] \`cargo test --all\` passes`; the task-file diff vs base is additions
   only plus frontmatter bookkeeping, with `commits:` matching the assigned
   list exactly.

## Transport restriction (recorded, not a defect)

crates.io and index.crates.io are connection-refused from this sandbox
(curl exit 7, empty CARGO_HOME, no target dir), so Rust suites could not be
re-executed this round. Evidence chain in lieu of a local run: the candidate's
only changes after the round-8 repaired head `c5864610e` are task-file process
notes (`git diff c5864610e..HEAD` touches no Rust), and the task-163 adapter
and test files are byte-identical to the round-6 head `202c9a2d1` where the
focused suites (adapters 7/4/3, integration 2, trigger_cascade 8,
api::dependencies 64) all passed; the only surface deltas since are migration
directory renames with unchanged SQL content. `cargo test --all` — the task's
unchecked sixth criterion, owned by the verification stage — must be run by
remote CI at the exact candidate head. Details and exact commands:
`/tmp/stage/review-evidence/transport-restriction.txt`.

## Findings

None. No stubs, no hollow enforcement, no scope-handling defects, no fake
implementations, and no self-confirming tests were found in the candidate diff.
