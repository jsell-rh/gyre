-- Reverse migration 000035
-- SQLite does not support DROP COLUMN directly on older versions;
-- we recreate the table without the new columns.

DROP INDEX IF EXISTS idx_repos_workspace_name;

CREATE TABLE repositories_backup AS SELECT
    id, name, path, default_branch, created_at,
    is_mirror, mirror_url, mirror_interval_secs, last_mirror_sync,
    tenant_id, workspace_id
FROM repositories;

DROP TABLE repositories;

ALTER TABLE repositories_backup RENAME TO repositories;
