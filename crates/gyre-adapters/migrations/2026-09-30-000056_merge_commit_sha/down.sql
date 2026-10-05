-- Revert TASK-095 R3-F1: drop the recorded merge commit SHA.

ALTER TABLE merge_requests DROP COLUMN merge_commit_sha;
