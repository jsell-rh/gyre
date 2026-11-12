-- TASK-107 (identity-security.md §Layer 3): persist jj-squash commit
-- signatures. The in-memory HashMap store orphaned every record on restart
-- (review F9): the record is the platform's copy of the cryptographic proof
-- (the Rekor entry alone carries no attribution claims or the local-mode
-- signature), so it must survive a server restart.
--
-- Keyed by (repo_id, commit_sha): a record created for repo A must not be
-- retrievable through repo B's API path.
CREATE TABLE commit_signatures (
    repo_id TEXT NOT NULL,
    commit_sha TEXT NOT NULL,
    signer_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    spawned_by TEXT NOT NULL,
    algorithm TEXT NOT NULL,
    signature TEXT NOT NULL,
    signing_key_id TEXT NOT NULL,
    signed_at BIGINT NOT NULL,
    sigstore_mode TEXT NOT NULL,
    oidc_subject TEXT NOT NULL,
    oidc_issuer TEXT NOT NULL,
    certificate_pem TEXT,
    certificate_chain_pem TEXT,
    rekor_entry_id TEXT,
    tenant_id TEXT NOT NULL DEFAULT 'default',
    PRIMARY KEY (repo_id, commit_sha)
);
