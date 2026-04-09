# Review: TASK-013 — Briefing Data Assembly (Cross-Workspace & Exceptions)

**Reviewer:** Verifier
**Round:** R1
**Verdict:** `needs-revision` (3 findings)

---

## Findings

- [-] [process-revision-complete] **F1: Cross-workspace filter direction is inverted relative to spec and link convention.**
  - **Spec (HSI §9):** "platform-core updated idempotent-api.md. Your payment-retry.md depends on it." — The cross_workspace section shows external specs that OUR specs depend on, where the external spec has changed.
  - **Link convention (`spec_registry.rs:484-486`):** `source_path = entry.path` (the local spec being scanned), `target = the dependency it references`. When `link_type = DependsOn`: source DependsOn target.
  - **Implementation (`graph.rs:868-872`):** Filters `target_repo_id` in workspace AND `source_repo_id` NOT in workspace → finds links where EXTERNAL specs depend on US.
  - **Should filter:** `source_repo_id` in workspace (our spec declares the link) AND `target_repo_id` NOT in workspace (the dependency is external). This matches the spec narrative: our spec depends on an external spec that was updated.
  - **Test data also inverted (`graph.rs:1905-1912`):** The test creates a link with `source = external-repo` and `target = repo-1` (local), so it matches the inverted filter. Both filter and test verify the wrong relationship.
  - **Fix:** Swap the filter conditions: `source_repo_id.is_some_and(|sid| ws_repo_ids.contains(sid))` AND `target_repo_id.is_some_and(|tid| !ws_repo_ids.contains(tid))`. Update test data to use the correct direction (source = local spec, target = external spec). Update title/description to reference the external dependency (target) that changed.

- [-] [process-revision-complete] **F2: Missing `actions` field on exception items — spec violation.**
  - **Spec (HSI §9, line 1316):** `"exceptions": [{"type": "...", "entity_id": "...", "summary": "...", "actions": [...]}]`
  - **Task plan (step 2):** "Actions are static labels per type (e.g., gate_failure → ['View Diff', 'View Output', 'Override', 'Close MR'])"
  - **Implementation:** `BriefingItem` has no `actions` field. Exception items serialize without actions, dropping the spec-required data entirely.
  - **Fix:** Add `pub actions: Vec<String>` to `BriefingItem` (defaulting to empty for non-exception sections), and populate with static labels per exception type: gate_failure → `["View Diff", "View Test Output", "Override", "Close MR"]`, assertion_failure → appropriate actions, mr_revert → appropriate actions. Alternatively, create a separate `ExceptionItem` type with the `actions` field.

- [-] [process-revision-complete] **F3: Gate results not filtered by gate result timestamp — old failures leak into briefing.**
  - **Implementation (`graph.rs:901-907`):** Filters MRs by `mr.updated_at >= since`, then includes ALL failed gate results for those MRs regardless of when the failure occurred.
  - **Semantic issue:** If an MR was updated recently (e.g., reviewer added at time 2000, after `since=1500`) but the gate failure happened at time 500 (before `since`), that old gate failure appears in the briefing. The briefing shows "since your last visit" — old failures shouldn't appear.
  - **Test gap (`graph.rs:1981-2014`):** The test creates both MR (`updated_at: 2000`) and gate result (`finished_at: Some(2000)`) with timestamps after `since=1500`, so the test passes even without the missing filter.
  - **Fix:** Add `gr.finished_at.map_or(false, |t| t >= since)` to the inner gate result filter at line 907:
    ```rust
    for gr in results.iter().filter(|gr| {
        gr.status == GateStatus::Failed
            && gr.finished_at.map_or(false, |t| t >= since)
    }) {
    ```
    Add a test case with an MR updated after `since` but gate result finished before `since` to verify the old failure is excluded.
