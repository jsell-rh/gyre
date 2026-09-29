# Coverage: Spec Lifecycle Automation

**Spec:** [`system/spec-lifecycle.md`](../../system/spec-lifecycle.md)
**Last audited:** 2026-09-29 (deep re-audit against git_http.rs + spec_patrol.rs. Verified §1,§2 (hook wired post-push git_http.rs:667, no CI/polling), §3,§6 (correct title/labels/priority, tested :3834), §9 (real revoke_all_for_path DB update on M/D/R, sqlite+postgres adapters). §8 demoted to not-started (hollow). §4,§5,§7 kept implemented — Partial, notes corrected.)
**Coverage:** 8/13 (3 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Principle | 2 | verified | - | "Specs first" bidirectional enforcement is real: process_spec_lifecycle post-receive hook (git_http.rs:1361) runs same-tick on push, no external CI/polling. |
| 2 | Automatic Task Creation on Spec Change | 2 | verified | - | process_spec_lifecycle (git_http.rs:1361-1540) wired into async post-receive block (git_http.rs:667) after successful push; diffs old..new on default branch, forge-native. |
| 3 | Trigger: New Spec Added | 3 | verified | - | classify_spec_change 'A' -> "Implement spec: {path}", labels [spec-implementation, auto-created], priority Medium (git_http.rs:1279-1286); tested (git_http.rs:3834). Minor: task stores spec_path not spec_ref@blob_sha — cosmetic. |
| 4 | Trigger: Existing Spec Modified | 3 | implemented | - | Partial (code-verified 2026-09-29) — 'M' -> "Review spec change: {path}", spec-drift-review, High, correct & tested (git_http.rs:3844). But task description (git_http.rs:1482) is generic; omits spec-required old_sha->new_sha delta and affected-references list (no binding-ledger query). Note previously falsely claimed "Includes affected references." |
| 5 | Trigger: Spec Deleted | 3 | implemented | - | Partial (code-verified 2026-09-29) — 'D' -> "Handle spec removal: {path}", spec-deprecated, High (git_http.rs:1292-1296). Core task creation genuine; description omits spec-required affected-references list and remove/supersede/revert decision guidance. |
| 6 | Trigger: Spec Renamed/Moved | 3 | verified | - | 'R' -> "Update spec references: {old} -> {new}", spec-housekeeping, Medium (git_http.rs:1297-1304); parse_spec_changes handles R with old/new path. |
| 7 | Task Deduplication | 2 | implemented | - | Partial (code-verified 2026-09-29) — dedups on identical title against non-Done tasks (git_http.rs:1470-1476); skips duplicate. Diverges from spec: dedup keyed on title not spec_ref+label, and does NOT update the existing task with the new SHA delta (spec §Task Deduplication) — it skips silently. |
| 8 | Accountability Integration | 2 | task-assigned | task-204 | Task-age accountability checks (spec-drift-review open > 1 Ralph cycle, spec-implementation in Backlog > N days, modified specs with no task) + escalation to workspace orchestrator. Distinct from spec_patrol.rs (spec-links graph patrol). |
| 9 | Spec Approval Interaction | 2 | verified | - | Auto-invalidate active approvals on M/D/R via spec_approvals.revoke_all_for_path (git_http.rs:1421-1460); sqlite adapter does real diesel::update setting revoked_at/by/reason on non-revoked rows for the path (sqlite/spec_approval.rs:206-234), postgres mirror present. |
| 10 | What This Does NOT Do | 2 | n/a | - | Anti-requirements — no implementable code. |
| 11 | Configuration | 2 | task-assigned | task-109 | Watched paths hardcoded as ["specs/system/", "specs/development/"] in git_http.rs:1270. Spec requires per-repo TOML config with watched_paths, ignored_paths, priorities, etc. |
| 12 | Implementation Notes | 2 | n/a | - | Implementation guidance — describes forge-native hook approach (already implemented). |
| 13 | Relationship to Existing Specs | 2 | n/a | - | Cross-reference section — no implementable requirement. |
