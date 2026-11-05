# Review: task-074 — LSP Call Graph — Python pyright Integration

**Candidate:** `fbfcf388bf03f24dca7abaefd1dfc88f94bd98ec` (base `0cb71015861e41b082183b89e9a1924808ad114a`)
**Round:** R1
**Verdict:** Approved

## Scope inspected

Diff base..candidate: `scripts/python-callgraph-pyright.mjs` (new LSP driver),
`scripts/python-callgraph.sh` (new wrapper; old stdlib-ast
`scripts/python-callgraph.py` deleted), `crates/gyre-adapters/src/call_graph.rs`
(Python dispatch + tests), `crates/gyre-domain/src/call_graph_resolve.rs`
(name-contract + marker tests only), `scripts/dashboard/test/python-callgraph.test.mjs`
(stub-langserver node test), Dockerfile, docs/server-config.md,
specs/coverage/system/lsp-call-graph.md, specs/tasks/task-074.md.

## What was verified (evidence under /tmp/stage/review-evidence/)

pyright 1.1.414 (pinned version, installed to a temp prefix) was used as a real
server for every probe; no stub stood in for the production chain.

1. **Real-pyright full chain, wrapper level.** `scripts/python-callgraph.sh`
   on the task fixture (cross-module call, method dispatch, instantiation,
   decorator, module-level call) emits exactly the 7 expected edges
   (`real-pyright-wrapper-output.json`), exit 0.
2. **Adversarial fixture 2** (independent of the candidate's fixture):
   multi-line `from ... import (helper,)` correctly filtered as import-span
   (no spurious module→helper edge); decorator inside a class attributed to
   the decorated method (`pkg2.multi.C.m -> pkg2.deco.wrap`); comment
   containing a fake `def ... helper(9)` ignored; instantiation lands on the
   class node; `c.m()` dispatch resolves through the typed receiver
   (`fixture2-edges-v2.json`). A call inside a nested def is attributed to the
   nested def; its `from` qname has no Pass 1 node and `resolve_call_edges`
   drops it silently (`call_graph_resolve.rs:136-139`) — consistent with the
   documented Pass 1 extraction surface, not a false edge.
3. **Adapter integration test with real server.**
   `cargo test -p gyre-adapters call_graph` with `PYRIGHT_LANGSERVER_BIN` set:
   4/4 pass, real-chain test runs in 1.5s (skip path is 0.03s, so the real
   chain demonstrably executed; no skip message under `--nocapture`)
   (`adapters-call-graph-real.txt`, `adapters-real-pyright-single-nocapture.txt`).
   Without the variable and no pyright on PATH: 4/4 pass, real test skips
   (`adapters-call-graph-no-pyright.txt`).
4. **Degradation contract.** Wrapper with no pyright on PATH and with
   `PYRIGHT_LANGSERVER_BIN=/nonexistent/ple`: warning on stderr, `[]` on
   stdout, exit 0 in both cases (`wrapper-no-path.*`, `wrapper-badbin.*`).
5. **Deterministic CI coverage.** `node --test
   scripts/dashboard/test/python-callgraph.test.mjs` passes
   (`node-python-callgraph-test.txt`). The stub replays real-pyright reference
   locations and the assertion kills the regressions it claims to pin: an
   unfiltered import-site reference to `wrap` (line 1) would produce
   `app.main -> app.deco.wrap`, which is not in the expected set; wrong
   decorator attribution or wrong enclosing-def resolution likewise changes
   the pair set. Not a mirrored-logic test.
6. **Name contract.** `cargo test -p gyre-domain call_graph_resolve`: 18/18
   including `resolve_pyright_edges_against_pass1_python_nodes` and
   `detect_all_languages_python_project_markers` (root and depth-1 markers)
   (`domain-call-graph-resolve.txt`). Cross-checked the driver's
   `pathToModule`/`queryable` rules against the Pass 1 extractor source
   (`python_extractor.rs`: `path_to_module_qname`, private module-level
   functions skipped, all methods and classes extracted, nested defs not
   walked) — the claimed surface match is real.
7. **Pipeline.** `cargo test -p gyre-server graph_extraction`: 25/25
   (`server-graph-extraction-test.txt`).
8. **Mechanical gates.** rustfmt clean on both touched Rust files;
   check-arch, check-relative-path-defaults, check-byte-slice-truncation,
   check-dead-message-kinds, check-task-commit-attribution all pass at this
   tree. Both new scripts are mode 755.
9. **Docker wiring** (not buildable in this sandbox — pattern review only):
   runtime stage WORKDIR is `/app`, matching the adapter's cwd-based
   `find_workspace_file` discovery of `/app/scripts/python-callgraph.sh`;
   `langserver.index.js` is executable with a `#!/usr/bin/env node` shebang
   and `/usr/local/bin/node` is copied into the image, so
   `PYRIGHT_LANGSERVER_BIN=/usr/local/bin/pyright-langserver` (symlink) works
   under both the wrapper's `-x` check and the driver's `spawn`. Mirrors the
   verified task-072 Go wiring.

## Notes (non-blocking)

- Class instantiation edges land on the class node, not `__init__` — this is
  pyright's actual `textDocument/references` behavior (verified in probe 2),
  the class node is Pass-1-extracted, and the deviation from the plan's
  "instantiation = call to `__init__`" parenthetical is documented in the
  driver header. Semantically sound under the references approach the spec
  recommends.
- `pyright_references_map_to_call_edges` ends with `remove_var` rather than
  restoring the ambient value; the only other reader of the variable is the
  degradation test, which saves/restores its own override under the same
  `ENV_LOCK`. No observable order dependence remains (both orderings pass in
  both environment configurations — probes 3's with/without runs).
- Full `cargo test --all` was not run here (workspace-wide invocations are
  out of review scope); focused suites covering every file this branch
  touches are green. The verifier's full-suite run remains the gate for the
  `cargo test --all` acceptance criterion.

Task contract preserved verbatim in `specs/tasks/task-074.md`; progress
recorded under `## Shipped` per the standing contract-restore finding.
Coverage-matrix rows updated to `task-assigned` (accurate — task-072 defects
still open there), no section overclaimed as verified.
