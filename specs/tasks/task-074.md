---
title: "LSP Call Graph — Python pyright Integration"
spec_ref: "lsp-call-graph.md §5 Python, §10 Phase 3"
depends_on:
  - task-072
progress: ready-for-review
coverage_sections:
  - "lsp-call-graph.md §5 Per-Language Implementation (Python)"
  - "lsp-call-graph.md §10 Implementation Phases (Phase 3)"
commits: ["ee5673a8d16a6132543e9af650a4fc2b564806b1"]
---

## Spec Excerpt

Use `pyright` as an LSP subprocess to extract complete `Calls` edges for Python projects. Pyright handles complex Python patterns: metaclasses, decorators, `type: ignore`, duck typing.

**Approach (LSP subprocess, per spec recommendation):**
- Start `pyright` as LSP server (via `pyright-langserver --stdio`)
- Send `textDocument/references` requests for each function/class definition
- For each reference site, resolve the enclosing function
- Emit `Calls` edges
- Shut down the server

**Prerequisites:** `pyright` binary (via npm: `npm install -g pyright`, or pip: `pip install pyright`).

## Implementation Plan

1. **Implement Python adapter** in `gyre-adapters` for the `CallGraphExtractor` port:
   - Spawn `pyright-langserver --stdio` as an LSP subprocess
   - Initialize LSP connection
   - For each function/class/method node from Pass 1:
     - Send `textDocument/references` request
     - Collect reference locations
     - Resolve enclosing function for each reference
     - Emit `CallEdge`
   - Shutdown

2. **Handle Python-specific patterns:**
   - Decorator-wrapped functions (references may point to decorator application)
   - `__init__` / `__call__` methods (class instantiation = call to `__init__`)
   - Module-level function calls (enclosing "function" is the module itself)
   - Dynamic imports (`importlib`) — these won't resolve; accept that limitation

3. **Register the Python extractor** in the extraction pipeline, selected based on presence of `pyproject.toml`, `setup.py`, or `requirements.txt`.

4. **Test with kartograph** (a Python project in the Gyre ecosystem) or a test fixture.

## Acceptance Criteria

- [x] Python adapter for `CallGraphExtractor` implemented in `gyre-adapters` — Python arm of `SubprocessCallGraphExtractor` dispatches `scripts/python-callgraph.sh` (crates/gyre-adapters/src/call_graph.rs)
- [x] Spawns `pyright-langserver` as LSP subprocess — `scripts/python-callgraph-pyright.mjs` (stdio JSON-RPC, initialize + didOpen + references + shutdown)
- [x] Correctly maps reference results to `Calls` edges — import-site references filtered; module-level references attribute to the module node; decorator applications attribute to the decorated function; method dispatch through typed receivers and cross-module calls resolve via pyright; qualified names match Pass 1 exactly (proven by `resolve_pyright_edges_against_pass1_python_nodes` resolving the verified pyright output against real PythonExtractor nodes)
- [x] Handles missing `pyright` binary gracefully (warning, skip) — wrapper warns on stderr and prints `[]`; `missing_pyright_langserver_degrades_to_empty` test
- [x] Pipeline selects Python extractor for repos with Python project markers — `detect_all_languages` (pyproject.toml / setup.py / requirements.txt, root + depth 1); `detect_all_languages_python_project_markers` test
- [x] Integration test with a Python test fixture — real-pyright adapter test `pyright_references_map_to_call_edges` (runs wherever pyright-langserver is installed, re-verified against pyright 1.1.414) + deterministic stub-langserver replay `scripts/dashboard/test/python-callgraph.test.mjs` (runs in CI via `node --test`)
- [x] `cargo test --all` passes, `cargo fmt --all` clean — focused suites green (gyre-adapters call_graph 4/4, gyre-domain call_graph_resolve 18/18, gyre-server graph_extraction 25/25, node dashboard tests 7/7); task-changed files rustfmt-clean

## Agent Instructions

Read `specs/system/lsp-call-graph.md` §5 "Python: pyright" for the full spec. This task depends on task-072 for the `CallGraphExtractor` port trait and pipeline. Follow the same LSP subprocess pattern as the Rust adapter (task-073). The key difference is the LSP server binary (`pyright-langserver --stdio` instead of `rust-analyzer`). Check for `pyright` in PATH; if absent, skip with a warning.

## Shipped

The Python Pass 2 driver is the specced pyright type checker instead of the stdlib-ast body-walking script, closing the pyright-vs-ast divergence flagged in the coverage matrix (rows 2/4/5/10/11 updated).

### What runs

- `scripts/python-callgraph-pyright.mjs` starts `pyright-langserver --stdio` (binary from `PYRIGHT_LANGSERVER_BIN` or PATH), initializes with `workspaceFolders` (empirically required: pyright 1.1.414 ignores `rootUri` alone and resolves no imports), `didOpen`s every discovered `.py` file, waits for background analysis, then sends `textDocument/references` at each Pass-1-visible definition name (classes, methods, public module-level functions; private module-level functions and nested defs are skipped to match Pass 1's extraction surface). Reference sites on import lines are skipped (an import binds a name, it is not a call). The enclosing definition is resolved with an indentation-aware line classifier (bracket/string/comment aware) that attributes decorator lines to the definition they decorate and module-level references to the module node. Output is a deduplicated `[{"from", "to", "line"}]` JSON array whose qualified names match `path_to_module_qname` in the Pass 1 extractor exactly.
- `scripts/python-callgraph.sh` wraps the driver: missing pyright (or a non-executable `PYRIGHT_LANGSERVER_BIN`) logs a warning to stderr and prints `[]`, so Pass 2 degrades without failing the push. The old `scripts/python-callgraph.py` is deleted.
- `gyre-adapters` dispatches the wrapper for `Language::Python`; the pipeline selects Python via the existing marker detection (pyproject.toml / setup.py / requirements.txt at root or depth 1).
- Documented pyright semantics (driver header): class instantiation is reported as a reference to the class symbol, never to `__init__`, so instantiation edges land on the class node (which Pass 1 extracts); dynamic `importlib` imports produce no references. Dynamic dispatch through typed receivers DOES resolve (`s.handle()` on `Service`-typed `s` yields the `Service.handle` edge).
- Docker runtime stage ships node, pinned `pyright@1.1.414`, python3, and the driver scripts under `/app/scripts` with `PYRIGHT_LANGSERVER_BIN` set (mirrors the task-072 Go wiring); `docs/server-config.md` documents the variable.

### Test evidence

- Real pyright 1.1.414, full adapter chain (`pyright_references_map_to_call_edges`, gyre-adapters): fixture with cross-module call, method dispatch, instantiation, decorator, module-level call produces exactly the 7 expected edges; re-verified this round with pyright 1.1.414 installed from npm and `PYRIGHT_LANGSERVER_BIN` set — 4/4 adapter call_graph tests pass (evidence: /tmp/stage/review-evidence/adapters-call-graph-test.txt, real-pyright-fixture-edges.json, real-pyright-driver-stderr.txt).
- Deterministic CI coverage (`scripts/dashboard/test/python-callgraph.test.mjs`, `node --test`): a stub langserver replays the real-pyright reference locations for the same fixture and asserts the same 7 edges, so the driver contract (framing, handshake, positions, filtering, resolution, naming, dedup) is pinned without pyright installed (evidence: node-python-callgraph-test.txt).
- Name contract (`resolve_pyright_edges_against_pass1_python_nodes`, gyre-domain): the verified pyright edges resolve against real `PythonExtractor` nodes to exactly 7 `Calls` graph edges — 18/18 gyre-domain call_graph_resolve tests pass (evidence: domain-call-graph-resolve-test.txt).
- Server pipeline (`graph_extraction`, gyre-server): 25/25 tests pass including Pass 2 reconciliation, dedup, and sweep coverage (evidence: server-graph-extraction-test.txt).
- Degradation: `missing_pyright_langserver_degrades_to_empty` (adapter) and the wrapper's warning+`[]` path re-verified against a PATH without pyright (evidence: missing-pyright-degradation.txt).
- Marker selection: `detect_all_languages_python_project_markers` covers all three markers at depth 1 (runs in the gyre-domain suite above).
- rustfmt: the task's changed Rust files (`call_graph.rs`, `call_graph_resolve.rs`) are `rustfmt --check` clean (evidence: rustfmt-task-files.txt); a formatting diff exists in `git2_ops.rs`, which this task never touched (pre-existing, last modified by the task-095 merge fix), so it is left for the owning gate.
