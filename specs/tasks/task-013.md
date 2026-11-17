---
title: "Briefing Data Assembly — Cross-Workspace & Exceptions"
spec_ref: "human-system-interface.md §9 (Briefing Detail)"
depends_on: []
progress: complete
review: specs/reviews/task-013.md
coverage_sections: []
commits: ["1aa8f84aeb1e014caf329651ba1da18818429a30", "a09baf85167f05a28fea9ac67a3bfd65a3543714"]
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

- `feat(briefing): implement cross_workspace and exceptions sections (TASK-013)`
- `fix(briefing): address R1 findings — filter direction, actions field, gate timestamp (TASK-013)`
- `fix(briefing): address R2 findings — exception type values, cross-workspace timestamp filter (TASK-013)`
- `fix(briefing): address R3 findings — source_workspace_slug field, stale_since test coverage (TASK-013)`

## Shipped

HSI §9 Briefing `cross_workspace` and `exceptions` sections are
implemented in production code in `crates/gyre-server/src/api/graph.rs`
(`assemble_briefing`), shared by the REST route and the MCP briefing
resource; no stubs remain.

### Behavior

- **cross_workspace**: inbound spec links read from `state.spec_links_store`
  with the directional filter `source_repo_id IN workspace AND
  target_repo_id NOT IN workspace`, time-scoped by
  `created_at >= since OR stale_since >= since`, repo-filter aware, each
  item carrying `source_workspace_slug` and `spec_path`.
- **exceptions — gate failures**: the gate result's `finished_at` is the
  sole time gate (no `mr.updated_at` outer filter — gates run
  asynchronously after MR enqueue and the failure path never touches
  `mr.updated_at`, so that outer filter silently dropped fresh failures on
  stale-MR records).
- **exceptions — spec assertion failures**: sourced via the
  `NotificationRepository::list_by_workspace_type_since(workspace_id, type,
  since)` port query — workspace+type+since-scoped and unbounded — replacing
  `list_recent(200)` whose globally-truncated recency window dropped the
  failure once 200 newer notifications existed anywhere. The per-member
  notification fan-out (identical source event) is deduplicated in
  `assemble_briefing` by `(source.entity_id, title, created_at)`, so one
  failure event yields exactly one exception item. Implemented by every
  adapter: mem (`gyre-server/src/mem.rs`), SQLite and PostgreSQL
  (`gyre-adapters/src/{sqlite,postgres}/notification.rs`), and the
  `BroadcastingNotificationRepo` wrapper (`gyre-server/src/notifications.rs`).
- **exceptions — MR reverts**: Reverted MRs since `since`.
- Empty sections return `[]`, never null or omitted; zero-activity
  briefings still return 200 with the full schema.

### This round (verification repair, commit 1aa8f84a)

Verification of candidate `b8597270` (finding id
`31b77ca69db04b31b9d6a0941119c7ce`) failed the changed-line clippy gate:
`mem.rs:3294 clippy::unnecessary_sort_by` in the mem adapter's
`list_by_workspace_type_since` (introduced by the R4 fix `a09baf85`).
Fixed by replacing the `sort_by` closure with the `sort_by_key` +
`std::cmp::Reverse` idiom already used throughout `mem.rs` — semantics
identical (stable descending sort by `created_at`), no behavior change.
The assigned `## Git Commits` provenance list above is kept byte-identical;
review scoping for the R4 and repair commits is via the `commits:`
frontmatter.

### Test evidence (this round, at HEAD 1aa8f84a)

- `SKIP_WEB_BUILD=1 cargo clippy -p gyre-server --lib` — exit 0;
  `unnecessary_sort_by` no longer fires at `mem.rs:3294`. The remaining
  clippy warnings in touched files (`mem.rs:2002/2019/2030/3273/5395`,
  `graph.rs:1374`) are byte-identical pre-existing baseline code outside
  this branch's changed hunks (log:
  `/tmp/stage/review-evidence/task-013-clippy-mem-sort.log`).
- `cargo fmt -p gyre-server -- --check` — exit 0.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib sqlite::notification`
  — 10 passed, 0 failed, including
  `list_by_workspace_type_since_scopes_and_is_unbounded` (log:
  `/tmp/stage/review-evidence/task-013-adapters-notification-tests.log`).

### Server tests for the independent verifier

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --all-targets`
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib api::graph` — the
  briefing suite covers: `briefing_cross_workspace_populated_when_linked_spec_changes`,
  `briefing_cross_workspace_excludes_same_workspace_links`,
  `briefing_cross_workspace_stale_since_includes_recently_stale_link`,
  `briefing_cross_workspace_stale_since_excludes_old_stale_link`,
  `briefing_exceptions_gate_failures`,
  `briefing_exceptions_gate_failures_excludes_old_gate_results`, and the
  assertion-failure/revert/empty-section cases in the same module
  (exercised through the mem adapter, including the repaired sort).
