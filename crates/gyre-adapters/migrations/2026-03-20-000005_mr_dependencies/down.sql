-- Remove MR dependency graph fields
-- SQLite does not support DROP COLUMN in older versions; for PostgreSQL:
ALTER TABLE merge_requests DROP COLUMN IF EXISTS depends_on;
ALTER TABLE merge_requests DROP COLUMN IF EXISTS atomic_group;
