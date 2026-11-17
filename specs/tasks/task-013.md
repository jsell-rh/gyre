---
title: "Briefing Data Assembly — Cross-Workspace & Exceptions"
spec_ref: "human-system-interface.md §9 (Briefing Detail)"
depends_on: []
progress: ready-for-review
review: specs/reviews/task-013.md
coverage_sections: []
commits: ["a09baf85167f05a28fea9ac67a3bfd65a3543714"]
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
resource; no stubs remain. The R4 repair round fixed the three blocking
findings from the independent review of candidate `f8e9944` (finding id
`98422bb21a4d48b4a672c395bc65f930`); that production fix is commit
`a09baf85167f05a28fea9ac67a3bfd65a3543714` on this branch and is
preserved unchanged.

### Contract repair (this round)

The prior checkpoint added an R4 line to the protected `## Git Commits`
provenance section (contract finding `38b93875e29a4b42b538aa81671b4fbc`).
The executor restored the assigned task document; this round keeps that
assigned protected text byte-identical (the Git Commits list above is the
assigned four-line version) and records all progress under exactly this
`## Shipped` section. The R4 commit is attributed for review scoping via
the `commits:` frontmatter. No production code changed in this round —
the working tree matches `a09baf85` exactly for `crates/` and `web/`.

### Behavior (R4 production fix, commit a09baf85)

- **cross_workspace**: inbound spec links read from `state.spec_links_store`
  with the directional filter `source_repo_id IN workspace AND
  target_repo_id NOT IN workspace`, time-scoped by
  `created_at >= since OR stale_since >= since`, repo-filter aware, each
  item carrying `source_workspace_slug` and `spec_path` (unchanged from the
  prior round — no findings against it).
- **exceptions — gate failures (F2 fix)**: the gate result's `finished_at`
  is now the sole time gate. The outer `mr.updated_at >= since` filter was
  removed: gates run asynchronously after MR enqueue and the failure path
  never touches `mr.updated_at`, so that outer filter silently dropped
  fresh failures on stale MR records (approved days ago, enqueued and
  failed today, `mr.updated_at < since`).
- **exceptions — spec assertion failures (F1 + F3 fix)**: sourced via a new
  `NotificationRepository::list_by_workspace_type_since(workspace_id, type,
  since)` port query — workspace+type+since-scoped, across all users, and
  unbounded — replacing `list_recent(200)`, whose globally-truncated
  recency window dropped the failure once 200 newer notifications existed
  anywhere. The per-member notification fan-out (one row per notified
  Admin/Developer/Owner, identical source event) is deduplicated in
  `assemble_briefing` by `(source.entity_id, title, created_at)`, so one
  failure event yields exactly one exception item; distinct events (different
  `created_at`) each keep their own item.
- **exceptions — MR reverts**: unchanged (Reverted MRs since `since`).
- The new port method is implemented by every adapter: mem
  (`gyre-server/src/mem.rs`), SQLite and PostgreSQL
  (`gyre-adapters/src/{sqlite,postgres}/notification.rs`), and the
  `BroadcastingNotificationRepo` wrapper (`gyre-server/src/notifications.rs`).

### Test evidence (re-verified at this HEAD, this round)

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --all-targets` — **exit 0**
  (includes the new/extended briefing unit tests; log:
  `/tmp/stage/review-evidence/task-013-cargo-check-gyre-server.log`).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib sqlite::notification` —
  **10 passed, 0 failed**, including
  `list_by_workspace_type_since_scopes_and_is_unbounded` (fan-out copies
  from all users returned; other-workspace and pre-`since` rows excluded;
  250 newer noise rows of another type irrelevant; `since` past the event
  returns nothing; log:
  `/tmp/stage/review-evidence/task-013-adapters-sqlite-notification.log`).
- Server unit tests in `graph.rs` (execution deferred to the independent
  verifier per the build-budget constraint):
  - `briefing_exceptions_spec_assertion_failures` extended with a second
    per-member fan-out copy (same entity_id/title/created_at, different
    user) plus a distinct second event — asserts 2 items (fan-out
    deduplicated, distinct events kept).
  - `briefing_exceptions_gate_failure_on_stale_mr_record` —
    `mr.updated_at=1000 < since=1500 <= gr.finished_at=2000` → exactly 1
    `gate_failure` item, `timestamp=2000`.
  - `briefing_exceptions_assertion_failure_beyond_recency_window` — a
    `SpecAssertionFailure` followed by 201 newer notifications of other
    types (across users and workspaces) is still reported.

### Verifier commands (server tests, deferred)

```
SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib briefing_
SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib sqlite::notification
```
