-- TASK-105 downgrade: drops the compressed-detail column. Compressed
-- rows had their detail blanked, so downgrading after the job has run
-- loses those rows' detail payloads (the SIEM copy is the recovery path
-- per business-continuity.md).
ALTER TABLE audit_events DROP COLUMN detail_zstd;
