-- SQLite does not support DROP COLUMN for all versions, so we leave compute_target_id in place.
-- The compute_targets table is safe to drop.
DROP TABLE IF EXISTS compute_targets;
