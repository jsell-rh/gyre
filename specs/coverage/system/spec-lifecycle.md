# Coverage: Spec Lifecycle Automation

**Spec:** [`system/spec-lifecycle.md`](../../system/spec-lifecycle.md)
**Last audited:** 2026-04-13 (full audit — reclassification from not-started)
**Coverage:** 9/13 (3 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Principle | 2 | implemented | - | "Specs first" enforcement via post-receive hook in git_http.rs. Forge reacts to spec changes automatically. |
| 2 | Automatic Task Creation on Spec Change | 2 | implemented | - | process_spec_lifecycle() in git_http.rs:1361-1540. Runs as forge-native post-receive hook. No external CI or polling. |
| 3 | Trigger: New Spec Added | 3 | implemented | - | Status 'A' mapped to "Implement spec: {path}" task. Label spec-implementation, priority Medium. |
| 4 | Trigger: Existing Spec Modified | 3 | implemented | - | Status 'M' mapped to "Review spec change: {path}" task. Label spec-drift-review, priority High. Includes affected references. |
| 5 | Trigger: Spec Deleted | 3 | implemented | - | Status 'D' mapped to "Handle spec removal: {path}" task. Label spec-deprecated, priority High. |
| 6 | Trigger: Spec Renamed/Moved | 3 | implemented | - | Status 'R' mapped to "Update spec references: {old} -> {new}" task. Label spec-housekeeping, priority Medium. |
| 7 | Task Deduplication | 2 | implemented | - | Checks for existing non-Done tasks with same title before creating duplicates (git_http.rs:1469-1476). |
| 8 | Accountability Integration | 2 | implemented | - | Spec graph patrol in spec_patrol.rs checks stale links, orphaned supersessions. |
| 9 | Spec Approval Interaction | 2 | implemented | - | Auto-invalidate approvals on M/D/R changes via spec_approvals.revoke_all_for_path() with "system:spec-lifecycle" reason (git_http.rs:1421-1460). |
| 10 | What This Does NOT Do | 2 | n/a | - | Anti-requirements — no implementable code. |
| 11 | Configuration | 2 | task-assigned | task-109 | Watched paths hardcoded as ["specs/system/", "specs/development/"] in git_http.rs:1270. Spec requires per-repo TOML config with watched_paths, ignored_paths, priorities, etc. |
| 12 | Implementation Notes | 2 | n/a | - | Implementation guidance — describes forge-native hook approach (already implemented). |
| 13 | Relationship to Existing Specs | 2 | n/a | - | Cross-reference section — no implementable requirement. |
