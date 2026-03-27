-- SQLite does not support DROP COLUMN in older versions.
-- Nullify the added columns to reverse their effect.
UPDATE spec_approvals SET rejected_at = NULL, rejected_reason = NULL, rejected_by = NULL;
UPDATE tasks SET cancelled_at = NULL, cancelled_reason = NULL;
UPDATE merge_requests SET reverted_at = NULL, revert_mr_id = NULL;
