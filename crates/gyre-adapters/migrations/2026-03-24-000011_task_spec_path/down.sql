-- SQLite does not support DROP COLUMN before 3.35; recreate table without spec_path.
-- For PostgreSQL, a simple ALTER TABLE DROP COLUMN is used.
SELECT 1; -- no-op placeholder (handled per-backend if needed)
