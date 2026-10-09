-- TASK-168: Gate ordering (repo-lifecycle.md §3 Gates — "drag to reorder,
-- gates execute in order").
--
-- Quality gates gain an explicit position within their repo. Existing gates
-- backfill to 0; display/execution order is (position, created_at) so the
-- pre-existing created_at ordering remains the tiebreaker for untouched rows.

ALTER TABLE quality_gates ADD COLUMN position INTEGER NOT NULL DEFAULT 0;
