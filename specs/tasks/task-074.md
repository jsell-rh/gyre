---
title: "LSP Call Graph — Python pyright Integration"
spec_ref: "lsp-call-graph.md §5 Python, §10 Phase 3"
depends_on:
  - task-072
progress: complete
coverage_sections:
  - "lsp-call-graph.md §5 Per-Language Implementation (Python)"
  - "lsp-call-graph.md §10 Implementation Phases (Phase 3)"
commits: ["0cce4f05b002e95ae12b0d9733ae63de6ba79de0", "ee5673a8d16a6132543e9af650a4fc2b564806b1"]
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

- [ ] Python adapter for `CallGraphExtractor` implemented in `gyre-adapters`
- [ ] Spawns `pyright-langserver` as LSP subprocess
- [ ] Correctly maps reference results to `Calls` edges
- [ ] Handles missing `pyright` binary gracefully (warning, skip)
- [ ] Pipeline selects Python extractor for repos with Python project markers
- [ ] Integration test with a Python test fixture
- [ ] `cargo test --all` passes, `cargo fmt --all` clean

## Agent Instructions

Read `specs/system/lsp-call-graph.md` §5 "Python: pyright" for the full spec. This task depends on task-072 for the `CallGraphExtractor` port trait and pipeline. Follow the same LSP subprocess pattern as the Rust adapter (task-073). The key difference is the LSP server binary (`pyright-langserver --stdio` instead of `rust-analyzer`). Check for `pyright` in PATH; if absent, skip with a warning.

## Shipped

The Python Pass 2 driver is the specced pyright type checker instead of the stdlib-ast body-walking script, closing the pyright-vs-ast divergence flagged in the coverage matrix (rows 2/4/5/10/11 updated). The assigned contract above is preserved verbatim; per the repair finding, acceptance-criteria status lives here rather than in edited checkbox text.

### What runs

- `scripts/python-callgraph-pyright.mjs` starts `pyright-langserver --stdio` (binary from `PYRIGHT_LANGSERVER_BIN` or PATH), initializes with `workspaceFolders` (empirically required: pyright 1.1.414 ignores `rootUri` alone and resolves no imports), `didOpen`s every discovered `.py` file, waits for background analysis, then sends `textDocument/references` at each Pass-1-visible definition name (classes, methods, public module-level functions; private module-level functions and nested defs are skipped to match Pass 1's extraction surface). Reference sites on import lines are skipped (an import binds a name, it is not a call). The enclosing definition is resolved with an indentation-aware line classifier (bracket/string/comment aware) that attributes decorator lines to the definition they decorate and module-level references to the module node. Output is a deduplicated `[{"from", "to", "line"}]` JSON array whose qualified names match `path_to_module_qname` in the Pass 1 extractor exactly.
- `scripts/python-callgraph.sh` wraps the driver: missing pyright (or a non-executable `PYRIGHT_LANGSERVER_BIN`) logs a warning to stderr and prints `[]`, so Pass 2 degrades without failing the push. The old `scripts/python-callgraph.py` is deleted.
- `gyre-adapters` dispatches the wrapper for `Language::Python`; the pipeline selects Python via the existing marker detection (pyproject.toml / setup.py / requirements.txt at root or depth 1).
- Documented pyright semantics (driver header): class instantiation is reported as a reference to the class symbol, never to `__init__`, so instantiation edges land on the class node (which Pass 1 extracts); dynamic `importlib` imports produce no references. Dynamic dispatch through typed receivers DOES resolve (`s.handle()` on `Service`-typed `s` yields the `Service.handle` edge).
- Docker runtime stage ships node, pinned `pyright@1.1.414`, python3, and the driver scripts under `/app/scripts` with `PYRIGHT_LANGSERVER_BIN` set (mirrors the task-072 Go wiring); `docs/server-config.md` documents the variable.

### This repair round (contract restore + determinism fix)

- The production fix from `ee5673a8` is unchanged; the executor restored the assigned task document and this round re-records progress under `## Shipped` only, without editing the protected contract text.
- `0cce4f05` fixes a real test-ordering defect found by probing this round: `missing_pyright_langserver_degrades_to_empty` deleted `PYRIGHT_LANGSERVER_BIN` process-wide after asserting degradation, so whenever it grabbed `ENV_LOCK` first, `pyright_references_map_to_call_edges` silently skipped the real server chain even on hosts that configured the binary explicitly (observed as a 0.02s suite run where the wrapper alone takes ~1.6s). The ambient override is now saved and restored around the `/nonexistent` stub, so test order no longer decides whether the real chain runs; the configured-binary skip path is unchanged.

### Test evidence (fresh runs at this tree, pyright 1.1.414 installed from npm; evidence under /tmp/stage/review-evidence/)

- Real pyright full chain (`pyright_references_map_to_call_edges`, gyre-adapters): fixture with cross-module call, method dispatch, instantiation, decorator, module-level call produces exactly the 7 expected edges. Suite run 4/4 in every scheduling order with `PYRIGHT_LANGSERVER_BIN` set (four consecutive runs, 1.47–2.37s each, no skip — `adapters-call-graph-order-robust.txt`); isolated run 1/1 in 1.69s (`adapters-call-graph-real-pyright-nocapture.txt`). Without the variable and no pyright on PATH the suite still passes 4/4 with the documented skip warning (`adapters-call-graph-no-pyright.txt`). Wrapper-level probe on the same fixture emits the same 7 edges (`real-pyright-fixture-edges.json`, `real-pyright-driver-stderr.txt`).
- Deterministic CI coverage (`node --test scripts/dashboard/test/python-callgraph.test.mjs`): stub langserver replays the real-pyright reference locations for the same fixture and asserts the same 7 edges — pass (`node-python-callgraph-test.txt`).
- Name contract and marker selection (gyre-domain): `call_graph_resolve` 18/18 including `resolve_pyright_edges_against_pass1_python_nodes` and `detect_all_languages` 3/3 including `detect_all_languages_python_project_markers` (`domain-call-graph-resolve-test.txt`, `domain-detect-all-languages-test.txt`).
- Server pipeline (gyre-server): `graph_extraction` 25/25 including Pass 2 reconciliation, dedup, and sweep coverage (`server-graph-extraction-test.txt`).
- rustfmt: `crates/gyre-adapters/src/call_graph.rs` (the only gyre-adapters file this branch touches, per `git diff --stat` against the assigned base) is `rustfmt --check` clean (`rustfmt-task-files.txt`). Pre-existing formatting drift in `git2_ops.rs`, `sqlite/audit.rs`, and `sqlite/notification.rs` is untouched by this branch and left to the owning gate.
- Known limitation: the real-pyright adapter test needs pyright-langserver on the host (CI does not install it); the stub replay and name-contract tests run everywhere, and the Docker runtime image ships pinned pyright with `PYRIGHT_LANGSERVER_BIN` set.
