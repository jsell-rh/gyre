# Review: TASK-024 — Dep Graph CLI Commands

**Reviewer:** Verifier  
**Round:** R1  
**Commit:** `bb7ff9d7`  
**Verdict:** needs-revision

---

## Findings

- [ ] **F1: `dot_output_edge_colors` test is self-confirming — tests nothing about production code.**  
  File: `crates/gyre-cli/src/main.rs:2430-2468`  
  The test constructs both `expected_color` and `actual_color` using identical inline `match` expressions (lines 2449-2454 and 2456-2462). It never calls `print_dot_graph`. It is comparing the same logic against itself — this test always passes regardless of what the production color mapping does. If someone changed the production `print_dot_graph` to map `"code"` to `"red"`, this test would still pass.  
  **Fix:** Capture the actual output of `print_dot_graph` (e.g., by redirecting stdout or refactoring to return a `String`) and assert on the captured output's `color=` values.

- [ ] **F2: `dot_output_produces_valid_syntax` test does not invoke the production `print_dot_graph` function.**  
  File: `crates/gyre-cli/src/main.rs:2380-2427`  
  The test manually reconstructs the expected DOT output using inline logic (iterating `graph["nodes"]` and `graph["edges"]` with its own format strings) and asserts on that reconstruction. It never calls `print_dot_graph`. If `print_dot_graph` had a bug (e.g., wrong format string, missing semicolons, incorrect quote escaping), this test would still pass because it only validates the test's own reconstruction logic.  
  **Fix:** Call `print_dot_graph` with captured stdout and assert on the actual output. The test should verify the production function's output, not a parallel reimplementation.
