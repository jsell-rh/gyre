# TASK-035: System Default Views Seeding

**Spec reference:** `explorer-implementation.md` §System Default Views  
**Depends on:** None (saved_views table and SavedViewRepository already exist)  
**Progress:** `not-started`

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

## Current State

- `saved_views` table has an `is_system` boolean column.
- `SavedViewRepository` trait and SQLite adapter are implemented.
- REST API for saved views CRUD exists (`GET/POST /api/v1/repos/:id/views`).
- No system default views are seeded — the saved views list is empty by default.

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

- [ ] 4 system default views are seeded after first graph extraction for a repo
- [ ] Views have correct query JSON matching the spec
- [ ] `is_system = true` on all seeded views
- [ ] Seeding is idempotent (no duplicates on re-extraction)
- [ ] Frontend shows system views in the saved views dropdown
- [ ] System views cannot be deleted by users
- [ ] Tests for seeding logic and query validity

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-ports/src/saved_view.rs` for the SavedView type and repository trait
3. Read `crates/gyre-common/src/view_query.rs` for ViewQuery struct construction
4. Read `crates/gyre-domain/src/extractor.rs` (or wherever graph extraction is triggered) to find the integration point
5. Read `web/src/lib/ExplorerChat.svelte` for the saved views dropdown
6. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

_(none yet)_
