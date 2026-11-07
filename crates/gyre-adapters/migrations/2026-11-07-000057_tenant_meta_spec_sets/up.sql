-- TASK-158: tenant-scope meta-spec changes (meta-spec-reconciliation.md §8).
--
-- 1. Tenant-level meta-spec sets: tenant-wide default bindings that every
--    workspace in the tenant inherits unless its own meta-spec set overrides
--    the pinned path. Same persistence model as meta_spec_sets (000018):
--    opaque JSON keyed by the scope entity.
CREATE TABLE tenant_meta_spec_sets (
    tenant_id  TEXT   NOT NULL PRIMARY KEY,
    json       TEXT   NOT NULL,
    updated_at BIGINT NOT NULL
);

-- 2. Widen meta_specs.scope to admit 'Tenant' (Tenant-scoped meta-specs carry
--    the tenant ID in scope_id, mirroring the Workspace design).
--
-- SQLite cannot ALTER a CHECK constraint, and PostgreSQL constraint
-- replacement (ALTER TABLE ... DROP CONSTRAINT) is not valid SQLite, so the
-- table is rebuilt with the widened CHECK — the same portable pattern as
-- 000054_audit_event_envelope. meta_spec_versions and meta_spec_bindings
-- reference meta_specs(id), so they are rebuilt too (children dropped before
-- the parent; the rebuilt children reference the rebuilt parent, and all
-- parent rows are copied before any child row is inserted so the foreign-key
-- check passes on both backends).
CREATE TABLE meta_specs_new (
    id TEXT NOT NULL PRIMARY KEY,
    kind TEXT NOT NULL CHECK(kind IN ('meta:persona', 'meta:principle', 'meta:standard', 'meta:process')),
    name TEXT NOT NULL,
    scope TEXT NOT NULL CHECK(scope IN ('Global', 'Tenant', 'Workspace')),
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

INSERT INTO meta_specs_new (id, kind, name, scope, scope_id, prompt, version, content_hash, required, approval_status, approved_by, approved_at, approved_content_hash, created_by, created_at, updated_at)
SELECT id, kind, name, scope, scope_id, prompt, version, content_hash, required, approval_status, approved_by, approved_at, approved_content_hash, created_by, created_at, updated_at FROM meta_specs;

CREATE TABLE meta_spec_versions_new (
    id TEXT NOT NULL PRIMARY KEY,
    meta_spec_id TEXT NOT NULL REFERENCES meta_specs_new(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    prompt TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(meta_spec_id, version)
);

INSERT INTO meta_spec_versions_new (id, meta_spec_id, version, prompt, content_hash, created_at)
SELECT id, meta_spec_id, version, prompt, content_hash, created_at FROM meta_spec_versions;

CREATE TABLE meta_spec_bindings_new (
    id TEXT NOT NULL PRIMARY KEY,
    spec_id TEXT NOT NULL,
    meta_spec_id TEXT NOT NULL REFERENCES meta_specs_new(id),
    pinned_version INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(spec_id, meta_spec_id)
);

INSERT INTO meta_spec_bindings_new (id, spec_id, meta_spec_id, pinned_version, created_at)
SELECT id, spec_id, meta_spec_id, pinned_version, created_at FROM meta_spec_bindings;

DROP TABLE meta_spec_bindings;
DROP TABLE meta_spec_versions;
DROP TABLE meta_specs;
ALTER TABLE meta_specs_new RENAME TO meta_specs;
ALTER TABLE meta_spec_versions_new RENAME TO meta_spec_versions;
ALTER TABLE meta_spec_bindings_new RENAME TO meta_spec_bindings;
