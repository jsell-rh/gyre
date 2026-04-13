# Review: TASK-013 — Briefing Data Assembly (Cross-Workspace & Exceptions)

**Reviewer:** Verifier
**Round:** R4
**Verdict:** `complete` (0 findings)

---

## R1 Findings (all resolved)

- [-] [process-revision-complete] **F1: Cross-workspace filter direction is inverted relative to spec and link convention.** — Fixed in `ec71a1f1`. Filter now correctly checks `source_repo_id IN workspace AND target_repo_id NOT IN workspace`. Test data updated to match.
  **Process fixes:** Covered by implementation checklist item #53 (directional filter verification — spec narrative first, map roles to data model fields, derive predicates from mapping, test from spec narrative before code) and verifier target "Inverted directional filter."

- [-] [process-revision-complete] **F2: Missing `actions` field on exception items — spec violation.** — Fixed in `ec71a1f1`. `actions: Vec<String>` added to `BriefingItem` with static labels per exception type.
  **Process fixes:** Covered by implementation checklist item #54 (spec-excerpt response schema field completeness — enumerate every field in the spec excerpt's JSON schema and verify the struct includes each one).

- [-] [process-revision-complete] **F3: Gate results not filtered by gate result timestamp — old failures leak into briefing.** — Fixed in `ec71a1f1`. Inner filter now checks `gr.finished_at.map_or(false, |t| t >= since)`. Test case added for MR updated after `since` with gate result finished before `since`.
  **Process fixes:** Covered by implementation checklist item #55 (nested time-scope propagation — child entity timestamp filtering) and `scripts/check-nested-time-scope.sh` (pre-commit enforced).

## R2 Findings (all resolved)

- [-] [process-revision-complete] **F4: Exception `entity_type` values don't match the spec-defined type enum.** — Fixed in `bdd23253`. Changed `"assertion_failure"` → `"spec_assertion_failure"` and `"mr_revert"` → `"reverted"`. Tests updated.
  **Process fixes:** Covered by implementation checklist item #57 (type discriminator value conformance — verify string discriminator values match spec-defined values and existing serde conventions) and `scripts/check-type-discriminator-values.sh` (pre-commit enforced).

- [-] [process-revision-complete] **F5: Cross-workspace filter used `created_at` which cannot detect target changes.** — Fixed in `bdd23253`. Filter now includes `stale_since` disjunction; title changed to avoid "updated" claim; timestamp prefers `stale_since` when available.
  **Process fixes:** Covered by implementation checklist item #58 (timestamp semantic verification — read field's assignment sites, identify what event triggers the assignment, compare with spec's intended trigger condition, prefer purpose-built fields like `stale_since`).

## R3 Findings

- [-] [process-revision-complete] **F6: Missing `source_workspace_slug` field in cross_workspace items — spec-excerpt field omission.** — Fixed in `bb039b3d`. `source_workspace_slug: Option<String>` added to `BriefingItem`. Populated from `target_display` via `strip_prefix('@')` extraction (`graph.rs:908-912`). Tests assert on expected value `"platform-core"`.
  **Process fixes:** Covered by implementation checklist item #54 (spec-excerpt response schema field completeness).

- [-] [process-revision-complete] **F7: `stale_since` disjunction added in R2 has zero test coverage — untested code path.** — Fixed in `bb039b3d`. Two dedicated tests added: `briefing_cross_workspace_stale_since_includes_recently_stale_link` (created_at=500 < since=1500, stale_since=2000 >= since → included, timestamp=2000) and `briefing_cross_workspace_stale_since_excludes_old_stale_link` (both before since → excluded).
  **Process fixes:** Covered by implementation prompt Needs-Revision Workflow Step 2b (test coverage for fix-round code changes — new conditional branches require dedicated positive and negative tests).

## R4 Verification

All R1–R3 findings verified resolved. Systematic re-review of the full implementation found no new findings:

- **Cross-workspace filter direction:** source IN workspace, target NOT IN workspace ✓
- **Time scoping:** `created_at >= since || stale_since >= since` ✓
- **source_workspace_slug extraction:** correct `@slug/...` parsing ✓
- **Gate failure inner filter:** `finished_at >= since` prevents old-failure leakage ✓
- **Spec assertion failures:** type/workspace/timestamp filters correct ✓
- **MR reverts:** `revert()` updates `updated_at` (domain model line 117), filter sound ✓
- **MCP parity:** delegates to shared `assemble_briefing`, three-step `since` resolution matches REST ✓
- **Test coverage:** 10 dedicated unit tests + 3 integration tests + 4 MCP tests cover positive, negative, and edge cases ✓
