-- SQLite does not support DROP COLUMN before 3.35.0; create new table without the columns.
CREATE TABLE spec_ledger_entries_backup AS
  SELECT path, title, owner, kind, current_sha, approval_mode, approval_status,
         linked_tasks, linked_mrs, drift_status, created_at, updated_at
  FROM spec_ledger_entries;

DROP TABLE spec_ledger_entries;

ALTER TABLE spec_ledger_entries_backup RENAME TO spec_ledger_entries;
