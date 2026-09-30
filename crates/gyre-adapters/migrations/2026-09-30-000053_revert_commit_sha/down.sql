-- Revert TASK-095 R2-3: restore the old column name.

ALTER TABLE merge_requests RENAME COLUMN revert_commit_sha TO revert_mr_id;
