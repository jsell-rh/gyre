-- Add target_version_current column to dependency_edges for version drift tracking (TASK-021).
ALTER TABLE dependency_edges ADD COLUMN target_version_current TEXT;
