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

---

## Independent Review (pipeline checkpoint)

**Reviewer:** Independent review agent
**Candidate:** `f8e99449ba5b7e485631b284d74e9a607f30e881` (base `078ec12a5745379c254fe7dd00261f02e28d86cb`)
**Verdict:** `approved` — 0 findings

### Scope of the candidate diff

Base→candidate touches only `specs/tasks/task-013.md` (+52 lines): a `## Shipped`
section with `###` subheadings, per the executor's contract-restoration
instruction. The protected contract text (frontmatter through the Git Commits
list) is byte-identical to the assignment (script-verified against the assigned
body). No production code changed in the candidate itself.

### Production-code verification (code rides the base)

- Carrier claim verified: `d7940e857` (PR #627) is an ancestor of the assigned
  base and carried the reviewed implementation into
  `crates/gyre-server/src/api/graph.rs` (+863/−128).
- `assemble_briefing` (graph.rs:1123) implements both sections with real
  queries, no stubs: cross_workspace reads `spec_links_store` with the
  directional filter `source_repo_id IN workspace AND target_repo_id NOT IN
  workspace`, since-scoped by `created_at >= since OR stale_since >= since`,
  repo-filter aware, `source_workspace_slug` extracted from `@slug/...`
  target_display, `spec_path` = local affected spec. exceptions has three real
  sources: failed gate results for workspace MRs with the inner
  `finished_at >= since` filter, `SpecAssertionFailure` notifications filtered
  to workspace + repo scope, and `Reverted` MRs with `reverted_at`/`updated_at
  >= since`.
- Field semantics verified against producers: assertion-failure
  `source.entity_id` is set to the failing spec path by graph_extraction.rs
  (:1408), so its use as `spec_path` is correct. Notification `created_at` is
  epoch seconds — unit-consistent with `since`.
- Wiring: REST route registered (api/mod.rs:984) with ABAC mapping
  (abac_middleware.rs:455); repo filter validated against the path workspace
  (404/Forbidden); MCP briefing resource delegates to the shared
  `assemble_briefing` (mcp.rs:3115). Empty sections are `Vec` fields — always
  serialized as `[]`.

### Probe evidence

- Focused probe at the candidate: `SKIP_WEB_BUILD=1 cargo test -p gyre-server
  --lib briefing_` — 26 passed, 0 failed, exit 0
  (`review-evidence/task-013-briefing-tests.log`).
- Mutation probes (each restored; pristine sha256 verified, tree clean,
  suite re-green after restore; logs: `review-evidence/mutation1-stale-since.log`,
  `mutation2-gate-time-filter.log`, `mutation3-direction.log`):
  1. Removed the `stale_since` disjunction →
     `briefing_cross_workspace_stale_since_includes_recently_stale_link` FAILS
     (exit 101).
  2. Removed the gate-result inner `finished_at >= since` filter →
     `briefing_exceptions_gate_failures_excludes_old_gate_results` FAILS
     (exit 101).
  3. Inverted the directional filter → 2 cross_workspace tests FAIL
     (exit 101).
  The tests are behavior-coupled, not self-confirming.
- `scripts/check-task-commit-attribution.sh` passes (docs-only commits are not
  product-surface; frontmatter `commits: []` is protected contract text).

### Non-blocking observations

- Assertion-failure sourcing uses `list_recent(200)` (the most generous cap of
  the established pattern — sibling call sites use 10–100); the
  `NotificationRepository` port offers no workspace+type+since-scoped query.
  In deployments generating >200 notifications newer than an in-window
  assertion failure, that failure would be omitted from the briefing
  (under-report only; workspace filtering after fetch cannot leak). A future
  port extension could remove the bound.
- The flat response envelope (`entity_type`/`description` at item level, not
  the spec's nested `sections` with `type`/`summary`) predates this task —
  `completed`/`in_progress` already used `BriefingItem` at the assigned base —
  and is documented in the HSI coverage matrix (row 45). Not in this task's
  acceptance criteria.
