# TASK-019 Review — R1

**Reviewer:** Verifier  
**Date:** 2026-04-09  
**Verdict:** `needs-revision` (2 findings)

---

## Findings

- [-] [process-revision-complete] **F1: Cycle detection excludes `extends` links — spec only excludes three types**

  **Location:** `crates/gyre-server/src/spec_registry.rs:1009-1019`

  **Spec reference:** spec-links.md §Cycle Detection:
  > - `conflicts_with` is bidirectional by nature (not a cycle)
  > - `references` and `supersedes` are excluded from cycle detection

  The spec explicitly names only three link types excluded from cycle detection: `conflicts_with`, `references`, and `supersedes`. The `extends` type is NOT in the exclusion list — it is a directed link type (A extends B) and can form genuine cycles (A extends B extends A).

  The implementation uses a wildcard match arm that catches ALL non-`depends_on`/`implements` types, including `extends`:

  ```rust
  match link.link_type {
      SpecLinkType::DependsOn | SpecLinkType::Implements => { ... }
      _ => {} // Skip conflicts_with, references, supersedes, extends.
  }
  ```

  The comment on line 1019 explicitly states `extends` is skipped, contradicting the spec. The match should include `Extends`:

  ```rust
  SpecLinkType::DependsOn | SpecLinkType::Implements | SpecLinkType::Extends => { ... }
  ```

  A test for `extends` cycle detection is also missing.

- [-] [process-revision-complete] **F2: Merge gate `conflicts_with` check is unidirectional — spec requires bidirectional**

  **Location:** `crates/gyre-server/src/merge_processor.rs:381-402`

  **Spec reference:** spec-links.md §Cycle Detection (line 158):
  > `conflicts_with` is bidirectional by nature (not a cycle)

  spec-links.md §Merge Gates:
  > If the spec has a `conflicts_with` link to an approved spec, the MR is blocked until the conflict is resolved

  The merge gate Check 2 only matches outbound `conflicts_with` links (`link.source_path == spec_path`). Since `conflicts_with` is bidirectional, an MR referencing spec B should also be blocked when spec A has a `conflicts_with` link targeting spec B and spec A is approved.

  Scenario: link `source=spec-a, target=spec-b, type=conflicts_with`. Spec A is approved.
  - MR referencing `spec-a` → blocked (source_path == spec_path matches, looks up spec-b)
  - MR referencing `spec-b` → **not blocked** (source_path != spec-b, skipped)

  The fix: also check the reverse direction — when `link.target_path == spec_path` for `ConflictsWith` links, look up `link.source_path` for approval status.

  The test `merge_gate_blocks_conflicts_with_approved` only tests the outbound case (the MR references `spec-a` which is the source of the `conflicts_with` link), so it does not detect this bug.
