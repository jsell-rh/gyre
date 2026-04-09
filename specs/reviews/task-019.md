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

---

# TASK-019 Review — R2

**Reviewer:** Verifier  
**Date:** 2026-04-09  
**Verdict:** `needs-revision` (1 finding)

R1 findings F1 and F2 are resolved. F1 fix correctly inverts the match to explicitly exclude `ConflictsWith | References | Supersedes` and uses `_ =>` as the include arm (catching `Extends`, `DependsOn`, `Implements`). Tests added for extends cycles. F2 fix correctly adds bidirectional check in merge gate using `source_path == spec_path || target_path == spec_path` with correct resolution of the "other" spec path. Reverse-direction test added.

---

## Findings

- [-] [process-revision-complete] **F3: `get_conflicts` endpoint returns all `conflicts_with` links instead of only active conflicts**

  **Location:** `crates/gyre-server/src/api/specs.rs:1206-1215`

  **Spec reference:** spec-links.md §Querying the Graph (line 173):
  > `GET /api/v1/specs/conflicts` — All active conflicts

  spec-links.md §Link Status (line 121):
  > `conflicted` — A `conflicts_with` link where both specs are approved (violation)

  Task plan (task-019.md line 57):
  > query `spec_links` for all links with `status = "conflicted"` or `link_type = "conflicts_with"` where both specs are approved

  The implementation filters only by `link_type == ConflictsWith`:

  ```rust
  pub async fn get_conflicts(State(state): State<Arc<AppState>>) -> Json<Vec<SpecLinkResponse>> {
      let links = state.spec_links_store.lock().await;
      let mut result: Vec<SpecLinkResponse> = links
          .iter()
          .filter(|l| l.link_type == SpecLinkType::ConflictsWith)
          .cloned()
          .map(Into::into)
          .collect();
      result.sort_by(|a, b| a.id.cmp(&b.id));
      Json(result)
  }
  ```

  This returns ALL `conflicts_with` links regardless of whether both specs are approved. A `conflicts_with` link between two specs where one is `Pending` is a declared potential conflict, not an "active conflict." The spec defines an active conflict (status `conflicted`) as a `conflicts_with` link where both specs are approved — a violation state.

  Since no existing code ever sets the `conflicted` status (grep for `"conflicted"` across `crates/gyre-server/src/` returns only a doc comment), the endpoint must check both specs' approval status at query time. The fix: for each `ConflictsWith` link, look up both the source and target specs via `spec_ledger.find_by_path()` and only include the link if both have `approval_status == Approved`.

  The test `get_conflicts_returns_conflicts_with_links` does not detect this because the seeded data (`seed_spec_with_links`) happens to set both specs in the conflicts_with link (`system/core.md` and `system/ui.md`) to `ApprovalStatus::Approved`. A negative test is needed: seed a `conflicts_with` link where one spec is `Pending` and assert the endpoint excludes it.

---

# TASK-019 Review — R3

**Reviewer:** Verifier  
**Date:** 2026-04-09  
**Verdict:** `needs-revision` (1 finding)

R2 finding F3 is resolved. The `get_conflicts` endpoint now correctly collects `ConflictsWith` candidate links, releases the lock, then checks both specs' `approval_status` via `spec_ledger.find_by_path()` — only including links where both specs are `Approved`. A negative test (`get_conflicts_excludes_non_approved_conflicts`) seeds a `conflicts_with` link between an Approved and a Pending spec and asserts the endpoint returns empty.

---

## Findings

- [-] [process-revision-complete] **F4: `merge_gate_warns_on_unimplemented_depends_on` test has no assertions — tautological**

  **Location:** `crates/gyre-server/src/api/specs.rs:4262-4342`

  **Spec reference:** Acceptance criteria:
  > - Merge processor warns (does not block) on unimplemented `depends_on`
  > - Tests cover all merge gates, query endpoints, and cycle detection

  The test `merge_gate_warns_on_unimplemented_depends_on` calls `merge_processor::run_once(&state).await.unwrap()` at line 4333 and then ends with only comments — zero `assert!` statements. This is a "no panic = success" tautological test: the function always passes regardless of behavior.

  Compare with the other two merge gate tests in the same file:
  - `merge_gate_rejects_superseded_spec` (line 4170): `assert!(all.is_empty(), ...)`
  - `merge_gate_blocks_conflicts_with_approved` (line 4255): `assert!(all.is_empty(), ...)`
  - `merge_gate_blocks_conflicts_with_approved_reverse_direction` (line 4425): `assert!(all.is_empty(), ...)`

  All three blocking-gate tests assert on observable state. The warning-gate test asserts on nothing.

  If the `depends_on` code path were accidentally changed to block (e.g., someone adds `return Ok(())` after `update_status` on line 374 inside the depends_on check), this test would still pass — because it has no assertion to detect the difference between "blocked at spec link gate" and "passed spec link gate, failed later at git operations."

  The infrastructure to fix this already exists: `state.merge_queue.find_by_id(&entry_id)` returns the entry with its `error_message` field. The test stores `_entry_id` (line 4331) but never uses it. The fix: retrieve the entry after `run_once`, assert its `status == Failed` (it will fail at git operations), and assert its `error_message` does NOT contain `"spec link merge gate"` — proving the failure occurred downstream, not at the spec link gate.
