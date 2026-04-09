# TASK-013: Briefing Data Assembly — Cross-Workspace & Exceptions

**Spec reference:** `human-system-interface.md` §9 (Briefing Detail)
**Depends on:** None (consumes existing notification, gate result, and spec link data)
**Progress:** `needs-revision`
**Review:** [`specs/reviews/task-013.md`](../reviews/task-013.md)

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

- `feat(briefing): implement cross_workspace and exceptions sections (TASK-013)`
- `fix(briefing): address R1 findings — filter direction, actions field, gate timestamp (TASK-013)`
