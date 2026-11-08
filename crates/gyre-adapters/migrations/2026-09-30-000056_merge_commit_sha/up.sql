-- TASK-095 R3-F1: record the MR's own merge commit SHA at merge time.
--
-- The manual revert endpoint (POST /repos/:id/revert/:mr_id) previously
-- resolved "the merge commit" as the current target-branch HEAD — wrong
-- after any subsequent merge or revert. The MR row now carries the SHA of
-- the merge commit that landed it, so a specific MR can be reverted no
-- matter what landed afterwards.

ALTER TABLE merge_requests ADD COLUMN merge_commit_sha TEXT;
