-- Reverse the bindings_sha addition (SQLite cannot DROP COLUMN before
-- 3.35.0, so rebuild the table without it) and restore the retired
-- meta-spec-set tables.
CREATE TABLE spec_ledger_entries_backup AS
  SELECT path, title, owner, kind, current_sha, approval_mode, approval_status,
         linked_tasks, linked_mrs, drift_status, created_at, updated_at,
         repo_id, workspace_id
  FROM spec_ledger_entries;

DROP TABLE spec_ledger_entries;

ALTER TABLE spec_ledger_entries_backup RENAME TO spec_ledger_entries;

CREATE TABLE IF NOT EXISTS meta_spec_sets (
    workspace_id TEXT PRIMARY KEY NOT NULL,
    json TEXT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS tenant_meta_spec_sets (
    tenant_id TEXT PRIMARY KEY NOT NULL,
    json TEXT NOT NULL,
    updated_at BIGINT NOT NULL
);
