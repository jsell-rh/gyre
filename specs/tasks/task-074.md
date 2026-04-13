---
title: "LSP Call Graph — Python pyright Integration"
spec_ref: "lsp-call-graph.md §5 Python, §10 Phase 3"
depends_on:
  - task-072
progress: not-started
coverage_sections:
  - "lsp-call-graph.md §5 Per-Language Implementation (Python)"
  - "lsp-call-graph.md §10 Implementation Phases (Phase 3)"
commits: []
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
