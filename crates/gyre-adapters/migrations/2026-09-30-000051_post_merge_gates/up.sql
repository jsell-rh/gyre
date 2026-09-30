-- TASK-095: Post-merge validation gates (platform-model.md §6 Rollback & Recovery).
--
-- Quality gates gain a phase discriminator (pre_merge = blocking, per-MR, the
-- existing behavior; post_merge = validation against the new default-branch
-- HEAD after merge) and an optional per-gate command timeout.
--
-- Existing gates are all pre-merge, so the column backfills to 'pre_merge'.

ALTER TABLE quality_gates ADD COLUMN gate_phase TEXT NOT NULL DEFAULT 'pre_merge';
ALTER TABLE quality_gates ADD COLUMN timeout_secs BIGINT;
