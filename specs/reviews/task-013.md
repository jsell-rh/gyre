# Review: TASK-013 — Briefing Data Assembly (Cross-Workspace & Exceptions)

**Reviewer:** Verifier
**Round:** R2
**Verdict:** `needs-revision` (2 findings)

---

## R1 Findings (all resolved)

- [x] **F1: Cross-workspace filter direction is inverted relative to spec and link convention.** — Fixed in `ec71a1f1`. Filter now correctly checks `source_repo_id IN workspace AND target_repo_id NOT IN workspace`. Test data updated to match.

- [x] **F2: Missing `actions` field on exception items — spec violation.** — Fixed in `ec71a1f1`. `actions: Vec<String>` added to `BriefingItem` with static labels per exception type.

- [x] **F3: Gate results not filtered by gate result timestamp — old failures leak into briefing.** — Fixed in `ec71a1f1`. Inner filter now checks `gr.finished_at.map_or(false, |t| t >= since)`. Test case added for MR updated after `since` with gate result finished before `since`.

## R2 Findings

- [ ] **F4: Exception `entity_type` values don't match the spec-defined type enum — consumers matching spec values will miss these items.**
  - **Spec (HSI §9, line 1316):** The response field definition states:
    - `"spec_assertion_failure"` — spec assertion validation failure
    - `"reverted"` — MR with Reverted status
  - **Implementation (`graph.rs:956`):** Uses `"assertion_failure"` instead of `"spec_assertion_failure"`.
  - **Implementation (`graph.rs:976`):** Uses `"mr_revert"` instead of `"reverted"`.
  - **Impact:** Any downstream consumer (UI, CLI, MCP client) checking for the spec-defined values (`"spec_assertion_failure"`, `"reverted"`) will fail to match these exception items. Note that other parts of the codebase already use `"reverted"` for `MrStatus::Reverted` (see `merge_deps.rs:265`, `specs.rs:1156`, `merge_requests.rs:202`), so the briefing is inconsistent with both the spec and existing serialization conventions.
  - **Tests also use wrong values:** Lines 2116 (`"assertion_failure"`) and 2153 (`"mr_revert"`) — tests must be updated alongside the fix.
  - **Fix:** Change line 956 to `entity_type: "spec_assertion_failure".to_string()` and line 976 to `entity_type: "reverted".to_string()`. Update the test filter predicates at lines 2116 and 2153 to match.

- [ ] **F5: Cross-workspace filter comment claims "target that changed" but `created_at` filter cannot detect target changes — only detects link recreation on source push.**
  - **Spec (HSI §9, line 1271):** "platform-core updated idempotent-api.md. Your payment-retry.md depends on it." — The trigger for cross_workspace entries is the external dependency (target) being updated.
  - **Comment (`graph.rs:871`):** "Our spec (source) depends on an external spec (target) that changed." — Correct description of spec intent.
  - **Filter (`graph.rs:880`):** `link.created_at >= since` — But `created_at` is set to `now` each time the source spec's repo is pushed (`spec_registry.rs:495`), because the link store does a full refresh per source-spec push (`spec_registry.rs:545-551`). This timestamp reflects when the local repo was last pushed, NOT when the external dependency changed.
  - **Consequence:** (a) Every push to any workspace repo causes ALL its cross-workspace links to appear as "Dependency updated" in the next briefing — false positives. (b) If the external spec is updated but the local repo isn't re-pushed, the link's `created_at` isn't refreshed and the briefing misses the update — false negative.
  - **`SpecLinkEntry.stale_since`** (`spec_registry.rs:227`) was designed for exactly this purpose ("Timestamp when link became stale — target SHA advanced") but is never populated in the current codebase.
  - **Fix:** Add `stale_since` to the filter disjunction so the feature works correctly when staleness detection is implemented upstream:
    ```rust
    && (link.created_at >= since
        || link.stale_since.is_some_and(|t| t >= since))
    ```
    This preserves current behavior (new links appear) AND adds support for detecting target changes when `stale_since` is eventually populated. Also update the title from `"Dependency updated: {target_path}"` to `"Cross-workspace dependency: {target_path}"` to avoid claiming the dependency was "updated" when the filter cannot verify this. Update the `timestamp` field (line 898) to prefer `stale_since` when available: `timestamp: link.stale_since.unwrap_or(link.created_at)`.
