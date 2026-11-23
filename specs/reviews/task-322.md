# Review: task-322 — Edge Types table rows (class-inheritance Implements, Go/Python/TS Returns, Python RoutesTo)

- **Candidate:** `e11bc8404bab38ac7a4910a54e9ac8e976c54fcb`
- **Base:** `c42bfe033949ef04f8c4c41681126f116ab6a9de`
- **Reviewer verdict:** **approved** (independent evidence below)

## Scope check

Task: close the three unmet Pass 1 producers of the Edge Types table in
`specs/system/lsp-call-graph.md` — Implements from class inheritance
(Python superclasses, TS `extends`), Returns from function signatures
(Go/Python/TypeScript), and RoutesTo from Python framework patterns —
all inside `gyre-domain`, with deferred order-independent resolution,
no fabricated targets, and no behavior on the TS `implements` clause
(owned by the realized-model graph-edge-emitters plan).

## Implementation reviewed (diff base..candidate)

- `crates/gyre-domain/src/edge_resolution.rs` (new): shared
  `resolve_deferred_target` — exact qualified-name match, then suffix
  match with type-like-node preference and lexicographic tie-break for
  determinism, `None` (no edge) on no match. 7 unit tests including
  determinism-under-ambiguity and partial-suffix non-match.
- `python_extractor.rs`: `extract_class` reads the `superclasses`
  argument_list (identifier/attribute bases only — `Generic[T]`
  subscripts and keyword args yield nothing); `extract_function_node`
  defers `-> Type` annotations (string annotations unquoted, generics
  stripped to the outer name, which resolves to no repo node);
  `extract_endpoint_node` defers a RoutesTo edge to the handler qname
  and the handler Function node is now emitted alongside the Endpoint
  node (distinct qnames `{parent}.endpoint.{name}` vs
  `{parent}.{name}` so qname-keyed stable-ID remapping cannot collapse
  them). Resolution happens only after all files (`extract()` calls
  the three `resolve_deferred_*` after `extract_python_files`).
- `typescript_extractor.rs`: `emit_class` reads the heritage clause and
  matches **only** `extends_clause` — `implements_clause` is explicitly
  `continue`d (cross-plan ownership boundary respected; verified by
  grep: no non-test code parses `implements`). Return annotations on
  `function_declaration` and `export const` arrow/function expressions
  are deferred.
- `go_extractor.rs`: `function_declaration`/`method_declaration`
  `result` — `type_identifier`, `qualified_type`, and one-level
  `pointer_type` unwrap; multi-value lists, slices/maps/interfaces,
  builtins yield no edge. Deferred + resolved after all files.
- `gyre-server/src/graph_extraction.rs`: one end-to-end round-trip
  test (`python_deferred_edges_round_trip_into_stored_graph`) through
  `extract_and_store_graph` → `GraphPort` → `list_nodes/list_edges`,
  asserting exactly one Implements (`models.Admin`→`models.User`), one
  Returns (`models.build_user`→`models.User`), one RoutesTo
  (`app.endpoint.list_users`→`app.list_users` with
  `{"path":"/api/users","method":"route"}`), and no duplication on
  re-extraction.
- Coverage row 7 of `specs/coverage/system/lsp-call-graph.md` updated
  with concrete source evidence; no unrelated rows touched.

## Independent probes (evidence in /tmp/stage/review-evidence)

1. **Baseline gap confirmation** (probe0a/0b): at base `c42bfe03`,
   `EdgeType::Implements`/`Returns`/`RoutesTo` are constructed in
   neither `python_extractor.rs` nor `typescript_extractor.rs` (grep);
   at the candidate, all three producers exist in the changed
   extractors.
2. **Real extractor tests** (probe1): `/tmp/stage/bin/cargo test -p
   gyre-domain` → 601 + 8 integration passed, 0 failed. Focused suites:
   python_extractor 20 (both extraction orders, unresolvable base,
   multiple inheritance, return annotation, flask route), 
   typescript_extractor 19 (extends both orders, unknown base, mixin
   expression, return annotations), go_extractor 16 (function/method
   result types, multi-value none), edge_resolution 7.
3. **Mutation probe** (probe2): disabling the production
   `resolve_deferred_implements()` call makes both
   `class_inheritance_*` tests FAIL — the tests are not
   self-confirming. File restored afterwards (verified via git status).
4. **Architecture** (probe-arch.txt): `scripts/check-arch.sh` passes —
   all new code in `gyre-domain`, no forbidden dependencies.
5. **Server type check** (probe3): `/tmp/stage/bin/cargo check -p
   gyre-server --lib --tests` → clean, exit 0 (compiles the new
   cfg(test) round-trip test; build.rs rebuilt web assets from source).
6. **Formatting** (probe4): `cargo fmt --check -p gyre-domain -p
   gyre-server` → clean.

## Deferred to the independent verifier (4 GiB budget)

Server test execution (codegen too large for this sandbox):
- `cargo test -p gyre-server python_deferred_edges_round_trip_into_stored_graph`
- `cargo test -p gyre-server graph_extraction` (existing suite)

## Notes

- Commit attribution: task frontmatter lists `cc8e5ff8` (implementation)
  and `72910c3d` (formatting), matching the two code commits on the
  branch; the remaining commits are process/coverage records.
- No downstream consumer of the previous Python Endpoint qname format
  exists in gyre-server/gyre-adapters/gyre-cli (grep at base), so the
  `{parent}.endpoint.{name}` rename is safe within this tree.
- Working tree carries an executor-owned modification to
  `specs/tasks/task-322.md` (frontmatter/Shipped section) that predates
  this review; review source files are unmodified from the candidate.
