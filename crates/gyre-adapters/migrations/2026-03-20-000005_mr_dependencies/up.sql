-- Add MR dependency graph fields
ALTER TABLE merge_requests ADD COLUMN depends_on TEXT NOT NULL DEFAULT '[]';
ALTER TABLE merge_requests ADD COLUMN atomic_group TEXT;
