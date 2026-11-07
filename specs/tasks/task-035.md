---
title: "System Default Views Seeding"
spec_ref: "explorer-implementation.md §System Default Views"
depends_on: []
progress: ready-for-review
coverage_sections: []
commits: ["b0bb30824e81dd07705733f63ea7b8b3b152d91d"]
---

## Spec Excerpt

From `explorer-implementation.md` §System Default Views:

> Seeded on repo creation or first sync:
>
> ```json
> [
>   {
>     "name": "Architecture Overview",
>     "description": "Full codebase structure",
>     "query": { "scope": { "type": "all" }, "zoom": "fit" }
>   },
>   {
>     "name": "Test Coverage Gaps",
>     "description": "Functions not reachable from any test",
>     "query": {
>       "scope": { "type": "test_gaps" },
>       "emphasis": { "highlight": { "matched": { "color": "#ef4444", "label": "Untested" }}, "dim_unmatched": 0.3 },
>       "annotation": { "title": "Test coverage gaps" }
>     }
>   },
>   {
>     "name": "Hot Paths",
>     "description": "Most-called functions",
>     "query": {
>       "scope": { "type": "all" },
>       "emphasis": { "heat": { "metric": "incoming_calls", "palette": "blue-red" }},
>       "annotation": { "title": "Hot paths" }
>     }
>   },
>   {
>     "name": "Blast Radius (click)",
>     "description": "Click any node to see what it impacts",
>     "query": {
>       "scope": { "type": "focus", "node": "$clicked", "edges": ["calls", "implements", "field_of", "depends_on"], "direction": "incoming", "depth": 10 },
>       "emphasis": { "tiered_colors": ["#ef4444", "#f97316", "#eab308", "#94a3b8"], "dim_unmatched": 0.12 },
>       "edges": { "filter": ["calls", "implements", "field_of", "depends_on"] },
>       "zoom": "fit",
>       "annotation": { "title": "Blast radius: $name" }
>     }
>   }
> ]
> ```

## Shipped

**System default views are seeded, protected, and surfaced in the UI.** The backend was already in place from the explorer-canvas branch; this assignment verified it end-to-end against the acceptance criteria and closed the one remaining gap (frontend "System" badge).

### Backend (pre-existing, verified)

- `crates/gyre-server/src/api/saved_views.rs` — `system_default_views()` returns 7 views: the spec's exact 4 (Architecture Overview, Test Coverage Gaps, Hot Paths, Blast Radius (click)) with query JSON matching the spec shapes, plus High-Risk Untested Code, Spec Coverage, and Ungoverned Risk.
- Seeding is lazy on first `ListViews` access (repo-scoped `/repos/:id/views`, workspace-scoped `/workspaces/:id/explorer-views`, and the explorer WS path), name-reconciled and idempotent — existing system-view names are skipped, so re-listing never duplicates.
- All seeded views carry `is_system: true` and `created_by: "system"`.
- Delete endpoints reject system views with 403 "System default views cannot be deleted" (REST) and an error message on the WS path.

### Frontend (this assignment)

- `web/src/lib/ExplorerChat.svelte` — saved-views dropdown now renders a distinct **System** badge on system views (muted pill, uppercase) with a tooltip noting they cannot be deleted. Delete button was already hidden for system views; server-side 403 is the enforcement backstop.

### Test evidence

- Rust (`gyre-server` lib): `api::saved_views` 13 passed, `api::explorer_views` 16 passed (incl. `delete_builtin_view_forbidden`, `list_is_idempotent_for_builtins`), `explorer_ws::tests::test_system_default*` 5 passed — 34 total, 0 failed. All default queries pass `ViewQuery::validate` (`test_system_default_views_all_pass_validation`).
- Web (vitest): `ExplorerChat.test.js` 110 passed, including 2 new rendering tests — badge present on system views / absent on user views, delete button hidden for system views / shown for user views. Mutation-checked: removing the badge markup fails the badge test.
- Evidence retained at `/tmp/stage/review-evidence/task-035-verification.txt`.

## Implementation Plan

1. In `gyre-domain`, create a `default_views` module (or add to an existing module):
   - Define the 4 system default view queries as constants (JSON or ViewQuery structs)
   - Provide a function `seed_default_views(repo_id, workspace_id, tenant_id) -> Vec<SavedView>`

2. Integrate seeding into the graph extraction pipeline:
   - After the first successful graph extraction for a repo (when `list_nodes` returns results for the first time), call `seed_default_views`
   - Check `list_by_repo` first — if any `is_system=true` views exist, skip seeding (idempotent)
   - Use `SavedViewRepository::create` for each default view

3. Set `is_system = true` and `created_by = "system"` for all seeded views.

4. In the frontend (`ExplorerChat.svelte` or `ExplorerView.svelte`), display system views prominently:
   - Show them in the saved views dropdown with a distinct "System" badge
   - System views should NOT be deletable by users

5. Add test verifying:
   - Default views are seeded on first extraction
   - Seeding is idempotent (re-running does not duplicate)
   - View queries are valid (pass ViewQuery::validate)

## Acceptance Criteria

- [x] 4 system default views are seeded after first graph extraction for a repo
- [x] Views have correct query JSON matching the spec
- [x] `is_system = true` on all seeded views
- [x] Seeding is idempotent (no duplicates on re-extraction)
- [x] Frontend shows system views in the saved views dropdown
- [x] System views cannot be deleted by users
- [x] Tests for seeding logic and query validity

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-ports/src/saved_view.rs` for the SavedView type and repository trait
3. Read `crates/gyre-common/src/view_query.rs` for ViewQuery struct construction
4. Read `crates/gyre-domain/src/extractor.rs` (or wherever graph extraction is triggered) to find the integration point
5. Read `web/src/lib/ExplorerChat.svelte` for the saved views dropdown
6. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `b0bb30824e81dd07705733f63ea7b8b3b152d91d` feat(task-035): System badge for system views in explorer saved-views dropdown (this assignment)
- Backend seeding, delete protection, and validation tests predate task decomposition (explorer-canvas branch).
