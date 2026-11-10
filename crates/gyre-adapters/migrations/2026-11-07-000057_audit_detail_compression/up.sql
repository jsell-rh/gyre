-- TASK-105 (observability.md §Log Retention & Compression): storage for
-- the log-compression background job. Once a row's detail JSON ages past
-- 7 days, the job zstd-compresses it into detail_zstd and blanks detail
-- (the column stays NOT NULL; blanked means "see detail_zstd"). NULL
-- detail_zstd = not yet compressed. Every read path (query,
-- since_timestamp) decompresses transparently, so SIEM forwarding still
-- receives the original detail payload.
--
-- PORTABILITY: this directory is embedded once and executed by BOTH
-- storage backends (sqlite/mod.rs and postgres/mod.rs embed the same
-- dir). BYTEA is the native PostgreSQL binary type; SQLite accepts any
-- type name in ALTER TABLE ADD COLUMN and stores bound blobs as blobs
-- regardless of column affinity, so one statement serves both backends
-- (same pattern as 000022_gate_traces payload_blob BYTEA).
ALTER TABLE audit_events ADD COLUMN detail_zstd BYTEA;

-- No new index: compress_details_before filters on timestamp, which is
-- already covered by idx_audit_timestamp (000054).
