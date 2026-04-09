# Review: TASK-016 — Spec Links: Staleness Job & Approval Gate Enforcement

**Reviewer:** Verifier  
**Round:** R2  
**Verdict:** `needs-revision`

---

## R1 Findings (all addressed)

- [x] [process-revision-complete] **F1: Missing push-time inbound staleness detection (spec §Automatic Staleness Detection)**
- [x] [process-revision-complete] **F2: Missing drift-review task creation (spec §Automatic Staleness Detection step 3, acceptance criterion 3)**
- [x] [process-revision-complete] **F3: Missing approval invalidation for `extends` links (spec §Approval Gates)**
- [x] [process-revision-complete] **F4: No test coverage for `extends` push-time drift behavior (acceptance criterion 6)**

---

## R2 Findings

- [-] [process-revision-complete] **F5: Inbound extends drift-review tasks assigned to wrong repo/workspace**

  In step 6b (inbound staleness detection, `spec_registry.rs:639-662`), `create_drift_review_task` is called with `source_repo_id` and `source_workspace_id` from `sync_spec_ledger`'s parameters — these are the **pushed repo's** IDs. For inbound links, the extending spec (the link's source) lives in a **different** repo. The link's own `source_repo_id` field (`SpecLinkEntry.source_repo_id`) contains the correct repo ID, but the `stale_extends` collection (line 628-636) only captures `(source_path, target_path)` — the link's `source_repo_id` is discarded.

  Result: drift-review tasks for inbound extends links are created in the **pushed** repo (the target spec's repo) instead of the **extending** spec's repo. The extending spec's team doesn't see the task in their repo.

  The test `inbound_extends_staleness_full_side_effects` doesn't catch this because `make_test_link` hardcodes `source_repo_id: Some("repo1")` and `create_drift_review_task` is called with `Some("repo1")` — identical IDs for both the pushed repo and the link's source repo. A test that distinguishes them (e.g., link source = "repo_A", pushed repo = "repo_B") and asserts `task.repo_id == "repo_A"` would detect the bug.

  **Fix:** Include `source_repo_id` (and resolve its workspace_id) in the `stale_extends` collection. Pass the link's `source_repo_id` to `create_drift_review_task` instead of `sync_spec_ledger`'s `source_repo_id`. Also update the test to use distinct repo IDs for the pushed repo vs. the inbound link's source repo.

- [-] [process-revision-complete] **F6: Duplicate drift-review task creation for same-repo extends links**

  When an extends link's source AND target are both in the current manifest and the target's SHA changed, both step 6 (outbound) and step 6b (inbound) process the same link's side effects:

  1. Step 6 (outbound, line 547-584): Marks the link stale (`stale_since = now`), creates drift-review task, invalidates approval.
  2. Step 6 (line 590-597): Replaces store links — the stale link (with `stale_since = now`) is added to the store.
  3. Step 6b (line 606-662): The `stale_extends` filter (line 628-636) collects extends links with `stale_since == Some(now)` AND `target_path` in `changed_set`. The link from step 6 matches **both** criteria — it was processed in step 6 but is still collected here.
  4. Step 6b applies side effects again → **second** drift-review task created (duplicate). Approval invalidation is idempotent, but task creation is not.

  Concrete scenario: Specs A and B in the same repo. A extends B. Push changes B's SHA. Step 6 processes A→B (stale, task #1). Step 6b sees A→B still matches the filter (stale_since == now, B in changed_set) and creates task #2.

  **Fix:** In step 6b's `stale_extends` filter, exclude links whose `source_path` is in the current manifest's `source_paths` set (which were already processed in step 6). Extract `source_paths` from the step 6 scope so it's accessible in step 6b:
  ```rust
  .filter(|l| {
      l.link_type == SpecLinkType::Extends
          && l.stale_since == Some(now)
          && changed_set.contains(l.target_path.as_str())
          && !manifest_source_paths.contains(&l.source_path)  // exclude outbound (already processed)
  })
  ```

- [-] [process-revision-complete] **F7: Drift-review task creation limited to `extends` links — spec requires ALL link types**

  spec-links.md §Automatic Staleness Detection defines a 6-step algorithm where steps 1-3 are generic (all link types) and steps 4-6 are type-specific notes:

  > 1. Queries spec_links for all links where target_path matches the changed spec
  > 2. Marks those links as stale
  > **3. Creates drift-review tasks in the source specs' repos**
  > 4. For `extends` links: the extending spec may need to be updated
  > 5. For `depends_on` links: implementation work may need to be re-evaluated
  > 6. For `supersedes` links: the superseded spec should already be deprecated

  Step 3 applies generically to ALL stale links — "in the source specs' repos" with no type qualifier. The implementation creates drift-review tasks only for `extends` links (step 6 line 574, step 6b line 653). When a `depends_on`, `implements`, `supersedes`, or `conflicts_with` link becomes stale, no drift-review task is created — the source spec's team has no tracked work item to re-evaluate the relationship.

  **Fix:** In both step 6 and step 6b, create drift-review tasks for ALL stale links, not just extends. The `match link.link_type` block (line 531) should call `create_drift_review_task` for all link types when the link is stale, not just inside the `Extends` arm. The `extends`-specific side effects (drift_status, approval invalidation) remain extends-only.
