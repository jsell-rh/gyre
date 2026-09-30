-- TASK-095 R2-3: rename `revert_mr_id` to `revert_commit_sha`.
--
-- The recovery protocol stores the SHA of the revert commit that undid the
-- MR's changes on the default branch — no revert MR object is created. The
-- old column name falsely advertised an MR-id join key against
-- merge_requests.id; any consumer joining on it got a silent miss.

ALTER TABLE merge_requests RENAME COLUMN revert_mr_id TO revert_commit_sha;
