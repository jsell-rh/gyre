-- Revert TASK-158: drop tenant-level meta-spec sets and restore the
-- two-value scope CHECK. Tenant-scoped rows cannot survive the narrowed
-- CHECK, so they are deleted first (ON DELETE CASCADE removes their
-- archived versions; bindings referencing them block the delete the same
-- way the repository layer does).

DROP TABLE IF EXISTS tenant_meta_spec_sets;

DELETE FROM meta_specs WHERE scope = 'Tenant';

CREATE TABLE meta_specs_old (
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
    approved_content_hash TEXT,
    created_by TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

INSERT INTO meta_specs_old (id, kind, name, scope, scope_id, prompt, version, content_hash, required, approval_status, approved_by, approved_at, approved_content_hash, created_by, created_at, updated_at)
SELECT id, kind, name, scope, scope_id, prompt, version, content_hash, required, approval_status, approved_by, approved_at, approved_content_hash, created_by, created_at, updated_at FROM meta_specs;

CREATE TABLE meta_spec_versions_old (
    id TEXT NOT NULL PRIMARY KEY,
    meta_spec_id TEXT NOT NULL REFERENCES meta_specs_old(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    prompt TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(meta_spec_id, version)
);

INSERT INTO meta_spec_versions_old (id, meta_spec_id, version, prompt, content_hash, created_at)
SELECT id, meta_spec_id, version, prompt, content_hash, created_at FROM meta_spec_versions;

CREATE TABLE meta_spec_bindings_old (
    id TEXT NOT NULL PRIMARY KEY,
    spec_id TEXT NOT NULL,
    meta_spec_id TEXT NOT NULL REFERENCES meta_specs_old(id),
    pinned_version INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(spec_id, meta_spec_id)
);

INSERT INTO meta_spec_bindings_old (id, spec_id, meta_spec_id, pinned_version, created_at)
SELECT id, spec_id, meta_spec_id, pinned_version, created_at FROM meta_spec_bindings;

DROP TABLE meta_spec_bindings;
DROP TABLE meta_spec_versions;
DROP TABLE meta_specs;
ALTER TABLE meta_specs_old RENAME TO meta_specs;
ALTER TABLE meta_spec_versions_old RENAME TO meta_spec_versions;
ALTER TABLE meta_spec_bindings_old RENAME TO meta_spec_bindings;
