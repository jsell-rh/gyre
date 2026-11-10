-- Revert TASK-156 repair: drop the approval content-hash stamp.

ALTER TABLE meta_specs DROP COLUMN approved_content_hash;
