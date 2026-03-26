-- M29.5A: Persist spec/gates/attestation/container-audit/ledger stores.

CREATE TABLE IF NOT EXISTS quality_gates (
    id TEXT PRIMARY KEY NOT NULL,
    repo_id TEXT NOT NULL,
    name TEXT NOT NULL,
    gate_type TEXT NOT NULL,
    command TEXT,
    required_approvals INTEGER,
    persona TEXT,
    required INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS gate_results (
    id TEXT PRIMARY KEY NOT NULL,
    gate_id TEXT NOT NULL,
    mr_id TEXT NOT NULL,
    status TEXT NOT NULL,
    output TEXT,
    started_at INTEGER,
    finished_at INTEGER
);

CREATE TABLE IF NOT EXISTS repo_push_gates (
    repo_id TEXT PRIMARY KEY NOT NULL,
    gate_names TEXT NOT NULL DEFAULT '[]'  -- JSON: Vec<String>
);

CREATE TABLE IF NOT EXISTS spec_policies (
    repo_id TEXT PRIMARY KEY NOT NULL,
    require_spec_ref INTEGER NOT NULL DEFAULT 0,
    require_approved_spec INTEGER NOT NULL DEFAULT 0,
    warn_stale_spec INTEGER NOT NULL DEFAULT 0,
    require_current_spec INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS attestation_bundles (
    mr_id TEXT PRIMARY KEY NOT NULL,
    attestation TEXT NOT NULL,  -- JSON: MergeAttestation
    signature TEXT NOT NULL,
    signing_key_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS container_audit_records (
    agent_id TEXT PRIMARY KEY NOT NULL,
    container_id TEXT NOT NULL,
    image TEXT NOT NULL,
    image_hash TEXT,
    runtime TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    stopped_at INTEGER,
    exit_code INTEGER
);

CREATE TABLE IF NOT EXISTS spec_ledger_entries (
    path TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    owner TEXT NOT NULL,
    kind TEXT,
    current_sha TEXT NOT NULL,
    approval_mode TEXT NOT NULL,
    approval_status TEXT NOT NULL,
    linked_tasks TEXT NOT NULL DEFAULT '[]',  -- JSON: Vec<String>
    linked_mrs TEXT NOT NULL DEFAULT '[]',    -- JSON: Vec<String>
    drift_status TEXT NOT NULL DEFAULT 'unknown',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS spec_approval_events (
    id TEXT PRIMARY KEY NOT NULL,
    spec_path TEXT NOT NULL,
    spec_sha TEXT NOT NULL,
    approver_type TEXT NOT NULL,
    approver_id TEXT NOT NULL,
    persona TEXT,
    approved_at INTEGER NOT NULL,
    revoked_at INTEGER,
    revoked_by TEXT,
    revocation_reason TEXT
);

CREATE INDEX IF NOT EXISTS idx_gate_results_mr_id ON gate_results (mr_id);
CREATE INDEX IF NOT EXISTS idx_quality_gates_repo_id ON quality_gates (repo_id);
CREATE INDEX IF NOT EXISTS idx_spec_approval_events_spec_path ON spec_approval_events (spec_path);
