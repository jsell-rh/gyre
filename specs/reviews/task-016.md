# Review: TASK-016 — Spec Links: Staleness Job & Approval Gate Enforcement

**Reviewer:** Verifier  
**Round:** R1  
**Verdict:** `needs-revision`

---

## Findings

- [-] [process-revision-complete] **F1: Missing push-time inbound staleness detection (spec §Automatic Staleness Detection)**

  The spec says: *"When any spec changes (new SHA), the forge: 1. Queries `spec_links` for all links where `target_path` matches the changed spec. 2. Marks those links as `stale`."*

  This describes inbound detection at push time: when spec B gets a new SHA, find all existing links from OTHER specs that point TO B and mark them stale. The `sync_spec_ledger` function (`spec_registry.rs:504-521`) only checks outbound links — links FROM the pushed manifest — verifying each link's target SHA against the ledger. It does NOT scan the links store for existing links whose `target_path` matches any spec that just changed in this push.

  Inbound staleness detection only happens in the daily background job (`spec_link_staleness::run_once`), creating up to a 24-hour delay for detecting stale links when a target spec changes. The spec says this should happen "when any spec changes" (immediate, at push time).

  **Fix:** After updating ledger entries in step 4 of `sync_spec_ledger` (where changed SHAs are detected at line 353), scan the links store for all links where `target_path` matches any spec whose SHA changed, and mark those links as `"stale"` with `stale_since = now`.

- [-] [process-revision-complete] **F2: Missing drift-review task creation (spec §Automatic Staleness Detection step 3, acceptance criterion 3)**

  The spec §Automatic Staleness Detection step 3 says *"Creates drift-review tasks in the source specs' repos."* The task's acceptance criterion says *"Extends parent spec change triggers drift-review in extending specs."*

  Neither the push-time handler (`sync_spec_ledger`) nor the staleness background job (`run_once`) creates any drift-review task. The code marks `drift_status = "drifted"` on the extending spec (`spec_registry.rs:550`) but does not create a task entity. A drift-review task is a tracked work item that prompts review of the extending spec; simply flagging `drift_status` does not fulfill this requirement.

  **Fix:** When an `extends` link becomes stale (either at push time or in the staleness job), create a drift-review task (or notification with actionable context) in the extending spec's repo, in addition to marking `drift_status`.

- [-] [process-revision-complete] **F3: Missing approval invalidation for `extends` links (spec §Approval Gates)**

  The spec's Approval Gates table says for `extends`: *"When target changes, source's approval is invalidated (it may need to incorporate the parent's changes)."*

  The push-time handler (`spec_registry.rs:539-554`) marks the extending spec's `drift_status` as `"drifted"` but does NOT reset `approval_status`. If the extending spec was previously `Approved`, it retains its `Approved` status after the parent changes. This violates the spec's explicit statement that "source's approval is invalidated."

  Compare with the auto-invalidation that already works when a spec's OWN content changes (`spec_registry.rs:362-364`: `existing.approval_status = ApprovalStatus::Pending`). The same invalidation should apply when a parent spec changes via an `extends` link.

  **Fix:** In the `Extends` handling block, after marking `drift_status = "drifted"`, also set `source_entry.approval_status = ApprovalStatus::Pending` to invalidate the extending spec's approval.

- [-] [process-revision-complete] **F4: No test coverage for `extends` push-time drift behavior (acceptance criterion 6)**

  The acceptance criterion says *"Tests cover each link type enforcement."* Tests exist for `DependsOn` (2 tests in `specs.rs`), `Supersedes` (1 test in `specs.rs`), and the staleness job (6 tests in `spec_link_staleness.rs`). However, there is NO test verifying that a pushed spec change triggers `extends`-link staleness marking or drift status update in `sync_spec_ledger`.

  **Fix:** Add a test that calls `sync_spec_ledger` with a manifest containing an `extends` link whose `target_sha` differs from the ledger's current SHA, and asserts that: (a) the link is marked `"stale"`, (b) the extending spec's `drift_status` is set to `"drifted"`, and (c) the extending spec's `approval_status` is invalidated (per F3 fix).
