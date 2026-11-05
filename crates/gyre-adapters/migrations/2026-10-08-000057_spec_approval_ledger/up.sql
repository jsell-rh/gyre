-- Spec approval ledger (agent-gates.md §Spec Approval Ledger).
--
-- approved_at becomes nullable: NULL when status is Pending. Status is
-- derived from which timestamp column is non-null (never stored directly),
-- and only one of approved_at/revoked_at/rejected_at is non-null at a time.
--
-- SQLite cannot relax NOT NULL in place, so rebuild the table (same pattern
-- as 000013_remove_projects). PostgreSQL uses ALTER COLUMN DROP NOT NULL —
-- both dialects run this shared dir (scripts/check-migration-sql-portability.sh).

CREATE TABLE spec_approvals_new (
    id TEXT NOT NULL PRIMARY KEY,
    spec_path TEXT NOT NULL,
    spec_sha TEXT NOT NULL,
    approver_id TEXT NOT NULL,
    signature TEXT,
    approved_at BIGINT,
    revoked_at BIGINT,
    revoked_by TEXT,
    revocation_reason TEXT,
    rejected_at BIGINT,
    rejected_reason TEXT,
    rejected_by TEXT
);

INSERT INTO spec_approvals_new (
    id, spec_path, spec_sha, approver_id, signature, approved_at,
    revoked_at, revoked_by, revocation_reason,
    rejected_at, rejected_reason, rejected_by
)
SELECT
    id, spec_path, spec_sha, approver_id, signature, approved_at,
    revoked_at, revoked_by, revocation_reason,
    rejected_at, rejected_reason, rejected_by
FROM spec_approvals;

DROP TABLE spec_approvals;
ALTER TABLE spec_approvals_new RENAME TO spec_approvals;

-- Ledger lookups: approval status per (path, sha); audit queries per approver.
CREATE INDEX IF NOT EXISTS idx_spec_approvals_path_sha
    ON spec_approvals(spec_path, spec_sha);
CREATE INDEX IF NOT EXISTS idx_spec_approvals_approver
    ON spec_approvals(approver_id);
