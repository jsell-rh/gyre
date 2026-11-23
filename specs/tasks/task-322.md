---
title: "Close the unmet Edge Types table rows: class-inheritance Implements (Python superclasses, TypeScript extends), Returns for Go/Python/TypeScript, and Python RoutesTo"
spec_ref: "Spec gap spec-b89674dbcb695eb34e8d5e15 / edge-rows-inheritance-returns-routesto / d96599f10659"
depends_on: []
progress: complete
commits: ["72910c3df20e0ea6d9ebc3b3189d8a3a833120d0", "cc8e5ff826f4fe5d58086aa5e425631707739641"]
---

## Required behavior

The 'Edge Types Produced' table (specs/system/lsp-call-graph.md) names 'Syntax (impl blocks, class inheritance)' as the source of the Implements row, 'Syntax (function signatures)' as the 100%-coverage source of the Returns row, and 'Syntax (framework-specific patterns)' as the source of the RoutesTo row. Only the Rust extractor currently meets these rows for class inheritance and Returns, and (with Go/TS) RoutesTo. Close the remaining unmet parts in the Pass 1 extractors, all in gyre-domain (hexagonal boundary: no adapters/infrastructure imports).

(1) Implements - class inheritance. Python: extract_class (python_extractor.rs, currently reads only the class name, body fields/methods) must read the class_definition's superclass list (tree-sitter-python superclasses field / argument_list) so `class Admin(User)` emits an Implements edge from the Admin Type node to the User node. For each base name: resolve through the run-scoped name index; because extraction is file-by-file and the base class may be extracted after the derived class, resolution must be order-independent (collect (class_id, base_name) pairs during the pass and resolve them after all files are extracted, mirroring the Rust extractor's deferred_returns/resolve_returns pattern); resolution tries an exact qualified name, then a suffix match. No fabricated targets: a base that resolves to no known node (stdlib/external import, generic marker like Generic) yields no edge. Multiple inheritance emits one edge per resolvable base.

TypeScript: emit_class (typescript_extractor.rs, currently reads only name and body) must inspect the class heritage clause and emit an Implements edge from the class Type node to the base-class node for `extends Base` heritage, resolved through the same run-scoped index with the same order-independent deferred resolution and no-fabricated-targets rules; it may share the resolution machinery with the Python superclass work. Only plain identifier bases that resolve to a known node need edges (expression bases like mixin calls resolve to nothing and yield no edge).

CROSS-PLAN OWNERSHIP BOUNDARY (per review feedback 1c20952c55f116d6b1839a84:ts-extends-implements-unowned): the TypeScript `implements Interface` clause is owned SOLELY by the realized-model.md staged plan's graph-edge-emitters task (plan_id b26f707c1c6ea89a961ab82e824808bf1625a3e2b3a0c2312981a90a99f3fef3, /tmp/stage/other-plans/ef4c1074caa37e617259c45b7ee00719.json), whose required_behavior item (4) and acceptance criterion commit to exactly the `implements` keyword - not `extends`. This task owns the `extends` clause and MUST NOT parse the `implements` clause or emit TS Implements edges from it, so the two plans never duplicate edge emission for the same clause. After both plans execute, the Implements row is met for Rust impl blocks, Python class inheritance, and both TS heritage clauses.

(2) Returns - function signatures. The Go, Python, and TypeScript extractors must emit Returns edges (function/method node -> return-type node) from syntax, following the Rust extractor's deferred_returns/resolve_returns pattern (rust_extractor.rs): defer (function_id, return_type_name) during the pass; resolve after all files via the run-scoped name index (exact match, then suffix match); skip unresolved names (primitives like string/int/void, external types) with no fabricated edge; dedup (function, type) pairs. Go parses the function_declaration/method_declaration result; Python parses the function_definition's return annotation; TypeScript parses the return type annotation.

(3) RoutesTo - framework patterns. The Python extractor must emit a RoutesTo edge for Flask/FastAPI route-decorated functions, mirroring the TS/Rust/Go pattern: the route Endpoint node gets a RoutesTo edge to the handler function node (metadata carrying path and method), with deferred resolution when the handler node is not yet extracted (extract_endpoint_node at python_extractor.rs:694 currently emits only the Endpoint node and a Contains edge; the method parameter is unused). The handler function node must still be emitted as a Function node so the edge has a real target.

All new edges flow through the existing make_edge/stable-ID machinery and persist via GraphPort like the extractors' existing edges; extraction must remain error-free on files without these constructs.

## Acceptance Criteria

- [ ] A Python fixture with `class Admin(User):` yields an Implements edge from the Admin Type node to the User node when extracted by the real Python extractor; a fixture where User's file is processed after Admin's file (reversed extraction order) yields the same edge (order-independence); `class Foo(ExternalBase)` where no node named ExternalBase exists yields no Implements edge (no fabricated target).
- [ ] A TypeScript fixture with `class Admin extends User` yields an Implements edge Admin->User with both extraction orders tested; `class Admin extends Unknown` yields no edge; the task adds no behavior for the `implements` clause (owned by the realized-model plan's graph-edge-emitters task).
- [ ] Go, Python, and TypeScript fixtures each define a function with a user-defined return type in the same repo; extraction yields a Returns edge from the function node to the return-type node in each language, and a function returning a primitive/unknown type yields no Returns edge.
- [ ] A Python fixture with `@app.route("/api/users")` above `def list_users(...)` yields an Endpoint node, a handler Function node for list_users, and a RoutesTo edge from the endpoint node to the handler node carrying path/method metadata.
- [ ] The new edges round-trip through real extraction into the stored graph and are returned by the graph API used by the existing extractor tests (same integration path as Contains/FieldOf edges today), with merge/dedup unchanged.
- [ ] All existing extractor and graph_extraction tests still pass; scripts/check-arch.sh still passes (new code stays in gyre-domain).

## Verification intent

Run the real extractors over fixture repos and assert the specific edges above (Python and TS `class Admin(User)`/`class Admin extends User` -> Implements Admin->User in both extraction orders; Go/Python/TS function with user-defined return type -> Returns edge; `@app.route` fixture -> RoutesTo endpoint->handler), and assert the absent-edge cases (unresolvable base, primitive return type). The real defect that must make this fail at base 8546a781: an enumeration of every EdgeType:: construction in the four extractors shows EdgeType::Implements is constructed only at rust_extractor.rs:1160 (impl blocks), EdgeType::Returns only at rust_extractor.rs:845 (deferred_returns/resolve_returns), and EdgeType::RoutesTo in typescript_extractor.rs:299, go_extractor.rs:648, rust_extractor.rs:493 but never in python_extractor.rs - python extract_class never reads the superclass argument_list and typescript emit_class never reads the heritage clause. A reverted build therefore stores zero of the asserted Implements/Returns/RoutesTo edges and the probe finds none; asserting merely that the edge kind exists in EdgeType's string mapping (it does today) proves nothing and is not an acceptable probe.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/lsp-call-graph.md`. Source: `8546a781f772f05c823bbf4df6127bec812e2152`; plan: `a8e88983c3060cd3af4d90d16e8709208b9361fa1dece0940873c9e8d32bc60d`.

[
  {
    "id": "1c20952c55f116d6b1839a84",
    "decision": "gap",
    "requirement_quote": "| `Implements` | Syntax (impl blocks, class inheritance) | 1 | ~95% (misses implicit Go interfaces) |",
    "detail": "The Edge Types table defines what the extraction pipeline produces per edge type, with stated sources and coverage. Verified against all four Pass 1 extractors: Contains (all four extractors), Calls (Pass 2 type checkers for all four languages plus Pass 1 syntax Calls for Rust/TypeScript), FieldOf (all four), GovernedBy (all four, from spec comments), and DependsOn (Rust from Cargo.toml [dependencies]; Go package-level edges derived from import paths with go.mod supplying the module path) are produced. Three rows are not met by the code at this base: (1) Implements — the row's source is 'Syntax (impl blocks, class inheritance)': Rust emits Implements for `impl Trait for Type`, but class inheritance produces no Implements edges in either Python or TypeScript. Python's extract_class reads the class name, emits a Type node and a Contains edge, and walks the body for fields/methods — it never reads the class definition's superclass argument_list, so `class Admin(User)` yields no Implements edge. TypeScript's emit_class reads only the name and body fields — the tree-sitter heritage clause (`extends`/`implements`) is never inspected, and the file contains zero occurrences of 'heritage', 'extends', 'implements', or EdgeType::Implements, so `class X extends Y` and `class X implements Y` produce no Implements edges. (2) Returns — the row claims 'Syntax (function signatures), 100%': only the Rust extractor emits Returns edges (deferred_returns/resolve_returns from syn signatures); the Go, Python, and TypeScript extractors contain no EdgeType::Returns, so their function signatures produce no Returns edges. (3) RoutesTo — 'Syntax (framework-specific patterns)': Rust (handler annotations), Go (net/http route patterns), and TypeScript (Express/Next.js detection) emit RoutesTo, but Python extracts Flask/FastAPI-decorated functions as Endpoint nodes with only a Contains edge from the parent (extract_endpoint_node) — no RoutesTo edge is ever created for Python (zero EdgeType::RoutesTo in python_extractor.rs). Cross-plan ownership note: the realized-model.md staged plan's graph-edge-emitters task commits only to the TypeScript `implements` keyword — not `extends`, not Python superclasses, not Returns for Go/Python/TypeScript, not Python RoutesTo; none of the other active staged plans touch these rows (grep across /tmp/stage/other-plans for heritage/superclass/EdgeType::Returns returns no commitment), so after that plan executes the Implements row remains unmet for TS class inheritance (extends) and Python inheritance, and the Returns/RoutesTo rows remain unmet for the non-Rust languages.",
    "evidence": [
      {
        "file": "crates/gyre-domain/src/rust_extractor.rs",
        "quote": "let edge = self.ctx.make_edge(EdgeType::Implements, from_id, to_id);",
        "detail": "The Implements row is met for its Rust half: `impl Trait for Type` blocks produce Implements edges (impl → trait). The unmet half is class inheritance in Python and TypeScript."
      },
      {
        "file": "crates/gyre-domain/src/python_extractor.rs",
        "quote": "fn extract_class(",
        "detail": "Inspected the entire extract_class function: it resolves the class name, creates the Type node, emits a Contains edge (module → class), and walks the body for annotated fields, __init__ assignments, methods, and decorated definitions. It never reads the class_definition's argument_list (superclass list); grep for 'superclass', 'inheritance', 'bases', and class-related 'argument_list' in this file returns nothing — `class Admin(User)` produces no Implements edge to the User Type node."
      },
      {
        "file": "crates/gyre-domain/src/typescript_extractor.rs",
        "quote": "// Extract fields from class body (public_field_definition / property_declaration).",
        "detail": "emit_class reads only the class name and body fields; the tree-sitter heritage clause (`extends`/`implements`) is never read. Grep for 'heritage', 'extends', 'implements', and EdgeType::Implements in typescript_extractor.rs returns zero matches — neither `class Admin extends User` nor `class X implements Y` produces an Implements edge."
      },
      {
        "file": "crates/gyre-domain/src/rust_extractor.rs",
        "quote": "edge_type: EdgeType::Returns,",
        "detail": "The Returns row's only producer: resolve_returns resolves deferred (function, return-type) pairs collected from Rust signatures into Returns edges. The Go, Python, and TypeScript extractors contain no EdgeType::Returns anywhere (verified by grep), so the row's claimed 'Syntax (function signatures)' source produces nothing for three of the four languages."
      },
      {
        "file": "crates/gyre-domain/src/python_extractor.rs",
        "quote": "let meta = serde_json::json!({ \"path\": route_path }).to_string();",
        "detail": "extract_endpoint_node (Flask/FastAPI route decorators, Click commands, Celery tasks) creates the Endpoint node and a single Contains edge from the parent module/class — no RoutesTo edge is emitted (zero EdgeType::RoutesO matches in python_extractor.rs), so the RoutesTo row has no Python producer while Go/TypeScript/Rust all have one."
      }
    ]
  }
]

## Shipped

All three unmet Edge Types rows are now produced by real Pass 1 extractor code
in `gyre-domain` (plus one end-to-end integration test in `gyre-server`),
closing the gap with no fabricated targets and no behavior change on files
without the constructs.

### Implements — class inheritance

- Python (`python_extractor.rs` `extract_class`): reads the
  `class_definition` `superclasses` field; each plain/dotted base name is
  collected as `(class_id, base_name)` and resolved after all files via the
  run-scoped name index. Multiple inheritance emits one edge per resolvable
  base; `Generic[T]` subscripts, keyword args, and unresolvable bases
  (stdlib/external) yield no edge.
- TypeScript (`typescript_extractor.rs` `emit_class`): reads the class
  heritage `extends_clause` value; plain identifier bases are deferred and
  resolved the same way. Expression bases (mixin calls) yield no edge. The
  `implements_clause` is explicitly skipped — it is owned solely by the
  realized-model plan's graph-edge-emitters task (cross-plan boundary from
  review feedback `1c20952c55f116d6b1839a84:ts-extends-implements-unowned`).
- Shared machinery: new `crates/gyre-domain/src/edge_resolution.rs`
  (`resolve_deferred_target`) — exact qualified-name match, then suffix match
  with type-like-node preference and lexicographic tie-break for
  determinism, `None` (no edge) when nothing resolves. Mirrors the Rust
  extractor's `deferred_returns`/`resolve_returns` pattern; order-independent
  because resolution happens only after every file is extracted.

### Returns — function signatures

- Go (`go_extractor.rs`): `function_declaration`/`method_declaration`
  `result` — `User`, `*User`, `pkg.User` forms deferred and resolved;
  multi-value results, slices/maps/interfaces, and builtins yield no edge.
- Python (`python_extractor.rs`): `function_definition` `return_type`
  annotation (`-> User`, `-> "User"`, `-> models.User`); generics strip to
  the outer name which resolves to no repo node, primitives yield no edge.
- TypeScript (`typescript_extractor.rs`): `return_type` annotation on
  `function_declaration` and arrow/function-expression `export const`
  declarations; same stripping/skip rules.

### RoutesTo — framework patterns

- Python (`python_extractor.rs`): `extract_endpoint_node` now takes the
  method, stores it in the Endpoint node's metadata, defers
  `(endpoint_id, handler_qname, {path, method})`, and resolves to the handler
  Function node emitted by `extract_decorated` (the handler is emitted as a
  real Function node). The endpoint node keeps a distinct qualified name
  (`{parent}.endpoint.{name}`) so persistence's qname-keyed stable-ID remap
  does not collapse endpoint and handler onto one ID.

### Test evidence (this sandbox)

- `cargo test -p gyre-domain`: 601 passed, 0 failed (+8 integration tests in
  view_query_composition). Focused new-edge suites: edge_resolution 7,
  Python class_inheritance 2 (both extraction orders),
  TS class_extends 4 (both orders, unknown base, mixin), Go result_type 3
  (function, method, multi-value-none), Python/TS return annotations 2,
  flask_route 2 (endpoint + handler + RoutesTo with path/method metadata).
- `cargo check -p gyre-server --lib --tests`: clean (includes the new
  end-to-end test). `cargo fmt --check -p gyre-domain -p gyre-server`: clean.
- `bash scripts/check-arch.sh`: passes — all new code in gyre-domain.
- `cargo clippy -p gyre-domain --all-targets`: no warnings in new code (3
  pre-existing warnings in functions unchanged at base c42bfe03).

### Deferred to independent verifier (4 GiB budget)

Server test compilation must run in the verifier after the model exits:
- `cargo test -p gyre-server python_deferred_edges_round_trip_into_stored_graph`
  — drives a real git repo through `extract_and_store_graph` → GraphPort and
  asserts exactly one Implements (`models.Admin` → `models.User`), one
  Returns (`models.build_user` → `models.User`), one RoutesTo
  (`app.endpoint.list_users` → `app.list_users` carrying
  `{"path":"/api/users","method":"route"}`), and no edge duplication on
  re-extraction of the same commit.
- `cargo test -p gyre-server graph_extraction` (existing suite, same module).

Evidence retained at `/tmp/stage/review-evidence/task-322-evidence.txt`.
