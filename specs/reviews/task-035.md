# Review — task-035 (System Default Views Seeding)

Spec: `specs/system/explorer-implementation.md` §System Default Views (lines 295+).
Candidate: `d0bf060c498450b4772f9a43b4c7bcc18e08a049` (base `233664cf81cbb1189b2aa00f76e80c8445745955`); assigned commit `b0bb30824e81dd07705733f63ea7b8b3b152d91d` (frontend System badge).
Verdict: **approved**.

## What the candidate changes

The backend (seeding, protection, unique index) is pre-existing in the assigned base; the candidate's production delta is the last open plan item — the frontend "System" badge — plus two rendering tests and the task record (frontmatter + `## Shipped`; contract body unchanged, verified by diff). Since the acceptance criteria cover the whole feature, the backend was independently verified on the candidate rather than trusted from the base.

## Evidence (fresh runs, `/tmp/stage/bin/cargo` / `/tmp/stage/bin/npx`; details in `/tmp/stage/review-evidence/task-035-review.md`)

- `cargo test -p gyre-server --lib api::saved_views` → **13 passed, 0 failed** (default-view count/names/JSON/scope, `test_system_default_views_all_pass_validation`, per-view query-shape tests).
- `cargo test -p gyre-server --lib api::explorer_views` → **16 passed, 0 failed**, including `delete_builtin_view_forbidden` (seeds via a real LIST, then DELETE on the builtin asserts 403) and `list_is_idempotent_for_builtins` (two real LIST calls, equal counts).
- `cargo test -p gyre-server --lib explorer_ws::tests` → **43 passed, 0 failed** (5 `test_system_default*`: validity, unique names, scope types, ViewQuery deserialization, interactive clicked view).
- `npx vitest run src/__tests__/ExplorerChat.test.js` → **110 passed, 0 failed**, including the 2 new rendering tests from `b0bb30824`.
- **Mutation check** (restored immediately; tree verified clean afterward): removing the badge block and the `{#if !view.is_system}` delete-button guard makes both new tests fail — they are real guards, not self-confirming. Post-restore re-run passes.
- **Query-JSON fidelity** (programmatic comparison of `system_default_views()` against the spec's exact 4 views): every spec-specified field present with equal values for all 4 views. Implementation adds nested `annotation.description` strings and 3 extra views (7 total, asserted by `test_system_default_views_count`) — additive, not contradictory.

## Verified against acceptance criteria

- **Seeding**: lazy on first views-list across all three surfaces (repo-scoped `/repos/:id/views`, workspace-scoped `/workspaces/:id/explorer-views`, explorer WS). Idempotent by name-reconciliation **plus** a DB backstop: partial unique index `idx_saved_views_no_dup_system (tenant_id, workspace_id, repo_id, name, is_system) WHERE is_system = 1` (migration `000046`); the WS path treats a unique-violation as a concurrent-seed success. Re-listing never duplicates.
- **`is_system` / `created_by`**: all seeded views carry `is_system: true`, `created_by: "system"` on every seeding path.
- **Delete protection**: REST 403 "System default views cannot be deleted" (`saved_views.rs:459-465`, `explorer_views.rs:618`), WS error "Cannot delete system views" (`explorer_ws.rs:1295`), and the frontend hides the delete button plus an early return in `deleteView` — server-side 403 remains the enforcement backstop.
- **Frontend**: system views render in the saved-views dropdown with a distinct "System" badge (tooltip "System default view — cannot be deleted"), mutation-verified tests.
- **Scope handling**: `resolve_workspace_id` returns `""` on lookup failure (no fabricated `"default"` scope); the repo must resolve for the listing to have returned rows, so seeded views are correctly scoped.

## Notes (non-blocking, recorded for completeness)

- Spec says "Seeded on repo creation or first sync"; the implementation seeds lazily on first views-list access. The task's own Current State documents and blesses this design, and the observable guarantee holds — default views are always present whenever anything reads them, with no duplication. Recording the interpretation, not flagging it.
- The task body's pre-existing "Current State" says 6 default views; the actual (and tested) count is 7. The protected task text is unchanged and the `## Shipped` section states 7 correctly — stale count in protected text only.
- The added `annotation.description` fields inside two default queries go beyond the spec's JSON (which specifies only `title`). They are additive display strings that pass `ViewQuery::validate`; every spec-specified field is present with the spec's value.
