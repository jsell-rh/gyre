# Review: task-062 — View Query Grammar Core Types & Computed Reference Resolver

Spec: `view-query-grammar.md` §1–3. Commit: `ce4845cc` (task-file only; implementation predates it in `d7940e85`).

## Round 1

Scope verified:
- All 17 §3 computed references parse, validate (`validate_computed_expression`), and resolve
  (`resolve_computed_expression_inner`, `crates/gyre-domain/src/view_query_resolver.rs`).
  Edge-direction semantics match spec: `$callers`=incoming Calls BFS, `$callees`=outgoing,
  `$implementors`=incoming Implements sources, `$fields`=incoming FieldOf sources,
  `$descendants`=outgoing Contains, `$ancestors`=incoming Contains, set ops recurse correctly.
- Rust `ViewQuery`/`Scope`/`Emphasis` cover all seven §2 primitives; TypeScript
  `web/src/lib/types/view-query.ts` mirrors them field-for-field (Scope union, Emphasis,
  EdgeFilter, Zoom, Annotation).
- `cargo test -p gyre-domain view_query_resolver`: **115 passed, 0 failed** (verified with
  gcc/mold linker override; workspace `.cargo/config.toml` pins `clang` which is absent in this
  environment — environmental, not a code defect). Frontend `npm test` not runnable here
  (`vitest` not installed in node_modules) — could not verify the 53 validator tests.

Findings:

- [x] **F1 — Dead code: `compute_test_fragility_count` never called.**
  `crates/gyre-domain/src/view_query_resolver.rs:443-470` defines a private
  `compute_test_fragility_count(node_id, ...)` marked `#[allow(dead_code)]` with rationale
  "Kept for potential use in per-node metric queries." It is referenced nowhere in `crates/`
  or `web/` (grep confirms only the definition). Its single-node fragility computation is fully
  superseded by `compute_all_test_fragility` (the batch version used at line 1528 and in
  `$where(test_fragility, ...)`). Speculative dead code retained behind an `#[allow]` is a
  defect under the dead-code flaw class and violates clean-cutover (remove obsolete code).
  Fix: delete `compute_test_fragility_count` and its `#[allow(dead_code)]` attribute.

## Round 2

- [x] **F1 resolved.** Deleted `compute_test_fragility_count` (private, `#[allow(dead_code)]`)
  from `crates/gyre-domain/src/view_query_resolver.rs`. Its single-node computation was
  fully superseded by `compute_all_test_fragility` (used at the `$where(test_fragility, ...)`
  and `$test_fragility(...)` sites). `bfs_traverse` remains referenced by
  `compute_all_test_fragility`, so no cascading dead code. `cargo test -p gyre-domain
  view_query_resolver`: 115 passed, 0 failed. `cargo build -p gyre-domain`: clean, zero warnings.
