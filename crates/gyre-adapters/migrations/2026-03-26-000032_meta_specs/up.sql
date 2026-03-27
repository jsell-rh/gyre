CREATE TABLE meta_specs (
    id TEXT NOT NULL PRIMARY KEY,
    kind TEXT NOT NULL CHECK(kind IN ('meta:persona', 'meta:principle', 'meta:standard', 'meta:process')),
    name TEXT NOT NULL,
    scope TEXT NOT NULL CHECK(scope IN ('Global', 'Workspace')),
    scope_id TEXT,
    prompt TEXT NOT NULL DEFAULT '',
    version INTEGER NOT NULL DEFAULT 1,
    content_hash TEXT NOT NULL DEFAULT '',
    required INTEGER NOT NULL DEFAULT 0,
    approval_status TEXT NOT NULL DEFAULT 'Pending' CHECK(approval_status IN ('Pending', 'Approved', 'Rejected')),
    approved_by TEXT,
    approved_at INTEGER,
    created_by TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE meta_spec_versions (
    id TEXT NOT NULL PRIMARY KEY,
    meta_spec_id TEXT NOT NULL REFERENCES meta_specs(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    prompt TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(meta_spec_id, version)
);

CREATE TABLE meta_spec_bindings (
    id TEXT NOT NULL PRIMARY KEY,
    spec_id TEXT NOT NULL,
    meta_spec_id TEXT NOT NULL REFERENCES meta_specs(id),
    pinned_version INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(spec_id, meta_spec_id)
);
