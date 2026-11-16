-- Task-115 review F1: tasks gain a dedicated spec_ref column carrying the
-- pinned approved blob ("path@sha", agent-runtime.md §1 Phase 3 sub-task
-- shape). spec_path stays the bare path so list_by_spec_path exact-match
-- consumers (spec-rejection cancellation, spec progress, linked_tasks)
-- reach chain-created tasks.
ALTER TABLE tasks ADD COLUMN spec_ref TEXT;
