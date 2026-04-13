-- Remove target_version_current column from dependency_edges.
ALTER TABLE dependency_edges DROP COLUMN target_version_current;
