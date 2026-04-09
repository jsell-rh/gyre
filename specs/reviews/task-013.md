# Review: TASK-013 — Briefing Data Assembly (Cross-Workspace & Exceptions)

**Reviewer:** Verifier
**Round:** R3
**Verdict:** `needs-revision` (2 findings)

---

## R1 Findings (all resolved)

- [x] **F1: Cross-workspace filter direction is inverted relative to spec and link convention.** — Fixed in `ec71a1f1`. Filter now correctly checks `source_repo_id IN workspace AND target_repo_id NOT IN workspace`. Test data updated to match.

- [x] **F2: Missing `actions` field on exception items — spec violation.** — Fixed in `ec71a1f1`. `actions: Vec<String>` added to `BriefingItem` with static labels per exception type.

- [x] **F3: Gate results not filtered by gate result timestamp — old failures leak into briefing.** — Fixed in `ec71a1f1`. Inner filter now checks `gr.finished_at.map_or(false, |t| t >= since)`. Test case added for MR updated after `since` with gate result finished before `since`.

## R2 Findings (all resolved)

- [x] **F4: Exception `entity_type` values don't match the spec-defined type enum.** — Fixed in `bdd23253`. Changed `"assertion_failure"` → `"spec_assertion_failure"` and `"mr_revert"` → `"reverted"`. Tests updated.

- [x] **F5: Cross-workspace filter used `created_at` which cannot detect target changes.** — Fixed in `bdd23253`. Filter now includes `stale_since` disjunction; title changed to avoid "updated" claim; timestamp prefers `stale_since` when available.

## R3 Findings

- [-] [process-revision-complete] **F6: Missing `source_workspace_slug` field in cross_workspace items — spec-excerpt field omission.**
  - **Spec (HSI §9, line 1315):** `"cross_workspace": [{"source_workspace_slug": "...", "spec_path": "...", "summary": "..."}]` — `source_workspace_slug` is a required field.
  - **Task plan (line 50):** Explicitly says "Populate each entry: `source_workspace_slug` (from `target_display`), `spec_path`, `summary`."
  - **Spec narrative (line 1271):** "↔ platform-core updated idempotent-api.md" — the external workspace identity ("platform-core") is the primary information conveyed to the user.
  - **Implementation (`graph.rs:884-902`):** `BriefingItem` has no `source_workspace_slug` field. The workspace slug is embedded in the `description` string (via `target_display`, e.g., "Depends on @platform-core/api-svc/system/idempotent-api.md") but not as a structured field. Consumers cannot programmatically identify which external workspace the dependency comes from.
  - **Data source:** `SpecLinkEntry.target_display` (`spec_registry.rs:218-220`) contains the composite path (e.g., "@platform-core/api-svc/system/auth.md"). The workspace slug can be extracted by parsing the `@{workspace_slug}/` prefix.
  - **Fix:** Add `source_workspace_slug: Option<String>` to `BriefingItem` (or use a dedicated `CrossWorkspaceItem` struct). Populate it from `target_display` by extracting the workspace slug:
    ```rust
    source_workspace_slug: link.target_display.as_ref().and_then(|d| {
        d.strip_prefix('@').and_then(|s| s.split('/').next()).map(String::from)
    }),
    ```
    Update the test `briefing_cross_workspace_populated_when_linked_spec_changes` to assert on `source_workspace_slug` (expected: `"platform-core"` given `target_display: "@platform-core/api-svc/system/idempotent-api.md"`).

- [-] [process-revision-complete] **F7: `stale_since` disjunction added in R2 has zero test coverage — untested code path.**
  - **R2 fix (`graph.rs:881-882`):** Added `|| link.stale_since.is_some_and(|t| t >= since)` to the cross_workspace filter and `timestamp: link.stale_since.unwrap_or(link.created_at)` to the timestamp field.
  - **All tests:** Every test sets `stale_since: None` (lines 1946, 1987, 2201). No test exercises the `stale_since: Some(...)` path.
  - **Impact:** The `stale_since` path is the semantically correct mechanism for detecting dependency changes (as R2's own analysis confirmed — `created_at` produces false positives on every source push). Yet this path has zero test coverage. If someone accidentally removes the `stale_since` check, no test would fail.
  - **Fix:** Add a test case with a link where `created_at < since` (link is old) but `stale_since: Some(value)` where `value >= since` (dependency changed recently). Assert the link appears in `cross_workspace`. Also verify `timestamp` equals `stale_since` (not `created_at`). Add a negative case where `stale_since: Some(value)` with `value < since` and `created_at < since` — assert the link is excluded.
