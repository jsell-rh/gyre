---
title: "Briefing Data Assembly — Cross-Workspace & Exceptions"
spec_ref: "human-system-interface.md §9 (Briefing Detail)"
depends_on: []
progress: complete
review: specs/reviews/task-013.md
coverage_sections: []
commits: []
---

## Spec Excerpt

From `human-system-interface.md` §9:

> **Briefing endpoint:** `GET /api/v1/workspaces/:workspace_id/briefing` — Response:
> ```json
> {
>   "sections": {
>     "completed": [...],
>     "in_progress": [...],
>     "cross_workspace": [{"source_workspace_slug": "...", "spec_path": "...", "summary": "..."}],
>     "exceptions": [{"type": "gate_failure", "entity_id": "...", "summary": "...", "actions": [...]}],
>     "metrics": {...}
>   }
> }
> ```

> **Cross-Workspace** — platform-core updated idempotent-api.md. Your payment-retry.md depends on it.
>
> **Exceptions** — Gate failure: billing-service MR #47. cargo test failed (3 tests).

### Data Sources (from HSI §9):

| Section | Data Source |
|---|---|
| Cross-Workspace | Cross-workspace spec link watcher |
| Exceptions | Gate results + spec assertion failures + MR reverts |

## Current State

The briefing handler (`crates/gyre-server/src/api/graph.rs`) implements `completed` and `in_progress` sections but the `cross_workspace` and `exceptions` sections are explicitly stubbed:
- Line ~841: `// Section: cross_workspace — stub (empty for now).`
- Line ~844: `// Section: exceptions — stub (empty for now, future: gate failures).`

Both return empty arrays.

## Implementation Plan

1. **Cross-Workspace section:**
   - Access `state.spec_links_store` (`SpecLinksStore` — in-memory `Arc<Mutex<Vec<SpecLinkEntry>>>`) for cross-workspace spec link data
   - Filter for `SpecLinkEntry` records where `target_repo_id` matches a repo in this workspace (inbound links from other workspaces)
   - For each inbound link, check if the source spec was modified since the briefing `since` timestamp (via `SpecLedgerRepository`)
   - Populate each entry: `source_workspace_slug` (from `target_display`), `spec_path`, `summary` (description of the change)
   - Summary can be simple: "Updated by [agent/user] at [timestamp]" — no LLM needed

2. **Exceptions section:**
   - **Gate failures:** Query gate results with `status = "fail"` for MRs in workspace repos since `since`
   - **Spec assertion failures:** Query `Notification` table for `SpecAssertionFailure` type since `since`
   - **MR reverts:** Query MRs with `Reverted` status since `since`
   - Each entry: `type` (gate_failure/assertion_failure/mr_revert), `entity_id`, `summary`, `actions`
   - Actions are static labels per type (e.g., gate_failure → ["View Diff", "View Output", "Override", "Close MR"])

3. **Wire data sources:**
   - Access `state.spec_links_store` (already in `AppState`, see `crates/gyre-server/src/lib.rs`)
   - Inject `QualityGateRepository` (existing) for gate results
   - Inject `NotificationRepository` (existing) for assertion failures
   - Inject `MergeRequestRepository` (existing) for reverted MRs

4. **Add tests:**
   - Cross-workspace section populated when linked spec changes
   - Exceptions section populated with gate failures
   - Empty sections return empty arrays (not null)
   - Since-based filtering works correctly

## Acceptance Criteria

- [ ] `cross_workspace` section returns entries when dependent specs have changed since `since`
- [ ] `exceptions` section returns gate failures for workspace MRs since `since`
- [ ] `exceptions` section returns spec assertion failures since `since`
- [ ] `exceptions` section returns MR reverts since `since`
- [ ] Empty sections return `[]` (not null or omitted)
- [ ] Tests cover each section with mock data

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-server/src/api/graph.rs` — search for "cross_workspace" and "exceptions" to find the stubs
3. Read `crates/gyre-server/src/spec_registry.rs` for `SpecLinksStore`, `SpecLinkEntry` types and cross-workspace link handling
4. Read `crates/gyre-ports/src/quality_gate.rs` for gate result queries
5. Read `crates/gyre-ports/src/notification.rs` for notification queries
6. Read `crates/gyre-ports/src/merge_request.rs` for MR status queries
7. Fill in the stubbed sections with real data queries
8. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

Original pipeline branch (reviewed through R1–R4, reconciled into mainline
via PR #627):

- `6d718ba14` — feat(briefing): implement cross_workspace and exceptions sections (TASK-013)
- `8757a323b` — fix(briefing): address R1 findings — filter direction, actions field, gate timestamp (TASK-013)
- `5b08d8398` — fix(briefing): address R2 findings — exception type values, cross-workspace timestamp filter (TASK-013)
- `97dd32144` — fix(briefing): address R3 findings — source_workspace_slug field, stale_since test coverage (TASK-013)
- `bebc9b986` — review(task-013): R4 — complete, 0 findings (TASK-013)

Mainline carrier (ancestor of the assignment base; recorded in frontmatter
for review scoping):

- `d7940e857` — feat(explorer): explorer canvas, loop tooling, spec coverage tracking (#627) — carried the reviewed graph.rs briefing implementation (+991 lines), the task file, and the review record into main.

## Shipped

Complete. HSI §9 Briefing Detail `cross_workspace` and `exceptions`
sections are implemented in production code in
`crates/gyre-server/src/api/graph.rs` (`assemble_briefing`), shared by the
REST route and the MCP briefing resource; no stubs remain. Reviewed through
four rounds (R1–R3 findings fixed, R4 verdict `complete`, 0 findings — see
`specs/reviews/task-013.md`).

### Behavior

- **cross_workspace**: inbound spec links read from `state.spec_links_store`
  with the directional filter `source_repo_id IN workspace AND
  target_repo_id NOT IN workspace` (our spec depends on an external one),
  time-scoped by `created_at >= since OR stale_since >= since`, and
  repo-filter aware. Each item carries `source_workspace_slug` (extracted
  from the `@slug/...` target display), `spec_path` (the local affected
  spec), and `timestamp` preferring `stale_since` when set.
- **exceptions**: three real sources, each since-scoped — (1) gate failures:
  failed gate results for workspace MRs with `finished_at >= since` (inner
  filter prevents old failures leaking through recently-updated MRs);
  (2) spec assertion failures: `SpecAssertionFailure` notifications filtered
  to the workspace and repo scope; (3) MR reverts: MRs in `Reverted` status
  with `reverted_at`/`updated_at >= since`. Discriminators match the
  spec enum (`gate_failure`, `spec_assertion_failure`, `reverted`) and each
  type carries static `actions` labels.
- **Empty sections** serialize as `[]`, never null or omitted.

### Test evidence

Focused probe at assignment HEAD `078ec12a5` (branch
`pipeline/task-013/46b3400f491c4a66a2d12ede5b0f53e8-1`):
`SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib briefing_` —
**26 passed, 0 failed** (exit 0), including the 10 dedicated TASK-013 unit
tests (cross-workspace positive/negative, stale_since include/exclude,
gate failures incl. old-result exclusion, assertion failures, MR reverts,
empty-sections-as-arrays, since-filtering) and 4 MCP parity tests. Log
retained at `/tmp/stage/review-evidence/task-013-briefing-tests.log`.
