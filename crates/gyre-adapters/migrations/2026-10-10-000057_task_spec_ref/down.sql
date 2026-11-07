-- Revert task-115 F1: drop the pinned spec_ref column. spec_path already
-- holds the bare path; nothing migrates back into it.
ALTER TABLE tasks DROP COLUMN spec_ref;
