DROP INDEX IF EXISTS idx_spec_approvals_approver;
DROP INDEX IF EXISTS idx_spec_approvals_path_sha;

-- Restore NOT NULL approved_at (pre-000056 shape).
CREATE TABLE spec_approvals_old (
    id TEXT NOT NULL PRIMARY KEY,
    spec_path TEXT NOT NULL,
    spec_sha TEXT NOT NULL,
    approver_id TEXT NOT NULL,
    signature TEXT,
    approved_at BIGINT NOT NULL,
    revoked_at BIGINT,
    revoked_by TEXT,
    revocation_reason TEXT,
    rejected_at BIGINT,
    rejected_reason TEXT,
    rejected_by TEXT
);

INSERT INTO spec_approvals_old (
    id, spec_path, spec_sha, approver_id, signature, approved_at,
    revoked_at, revoked_by, revocation_reason,
    rejected_at, rejected_reason, rejected_by
)
SELECT
    id, spec_path, spec_sha, approver_id, signature, COALESCE(approved_at, 0),
    revoked_at, revoked_by, revocation_reason,
    rejected_at, rejected_reason, rejected_by
FROM spec_approvals;

DROP TABLE spec_approvals;
ALTER TABLE spec_approvals_old RENAME TO spec_approvals;
