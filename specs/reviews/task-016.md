# Review: TASK-016 — Spec Links: Staleness Job & Approval Gate Enforcement

**Reviewer:** Verifier  
**Round:** R3  
**Verdict:** `needs-revision`

---

## R1 Findings (all addressed)

- [x] [process-revision-complete] **F1: Missing push-time inbound staleness detection (spec §Automatic Staleness Detection)**
- [x] [process-revision-complete] **F2: Missing drift-review task creation (spec §Automatic Staleness Detection step 3, acceptance criterion 3)**
- [x] [process-revision-complete] **F3: Missing approval invalidation for `extends` links (spec §Approval Gates)**
- [x] [process-revision-complete] **F4: No test coverage for `extends` push-time drift behavior (acceptance criterion 6)**

---

## R2 Findings

- [x] [process-revision-complete] **F5: Inbound extends drift-review tasks assigned to wrong repo/workspace**

  Fixed: `inbound_stale_links` now captures `l.source_repo_id.clone()` and resolves workspace_id from the source spec's ledger entry. Task creation uses the link's repo/workspace, not the pushed repo's. Test `inbound_extends_staleness_full_side_effects` uses distinct `source_repo_id: "repo_LINK_SOURCE"` and asserts `task.repo_id == "repo_LINK_SOURCE"`.

- [x] [process-revision-complete] **F6: Duplicate drift-review task creation for same-repo extends links**

  Fixed: Step 6b filter now includes `!manifest_paths.contains(&l.source_path)` to exclude links whose source is in the current manifest (already processed in step 6). Test `no_duplicate_drift_review_for_same_repo_extends` verifies only 1 task is created.

- [x] [process-revision-complete] **F7: Drift-review task creation limited to `extends` links — spec requires ALL link types**

  Fixed: Task creation moved from the `Extends` match arm to a generic `if link.status == "stale"` block before the match (step 6 line 536-547, step 6b line 662-672). Test `drift_review_task_created_for_non_extends_stale_links` verifies tasks for `DependsOn` and `Implements` links.

---

## R3 Findings

- [ ] **F8: Drift-review tasks created for `references` links — spec says "No mechanical enforcement"**

  spec-links.md §Link Types defines `references` as:

  > | `references` | Informational link (no enforcement) | No mechanical enforcement. For human/agent navigation only. |

  The R2 fix for F7 correctly made task creation generic across all link types per §Automatic Staleness Detection step 3. However, the F7 fix over-corrected: `references` links are explicitly excluded from all mechanical enforcement by the Link Types table. Creating a drift-review Task entity is mechanical enforcement — it generates a tracked work item that shows up in the team's task list.

  Both code paths create tasks for `references` links:
  - Step 6 (outbound, `spec_registry.rs:536-547`): `if link.status == "stale"` — no type guard.
  - Step 6b (inbound, `spec_registry.rs:662-672`): `create_drift_review_task` called for all link types — no type guard.

  Staleness **marking** (setting `status = "stale"`, step 2) should still apply to `references` — it's informational state tracking, not enforcement. Only task **creation** (step 3) should exclude `references`.

  **Fix:** Add a type guard before task creation in both step 6 and step 6b:
  ```rust
  if link.status == "stale" && link.link_type != SpecLinkType::References {
      create_drift_review_task(...).await;
  }
  ```
  Alternatively, add the guard inside `create_drift_review_task` itself so the exclusion is centralized. Add a test that creates a `references` link, triggers staleness, and asserts no drift-review task is created.
