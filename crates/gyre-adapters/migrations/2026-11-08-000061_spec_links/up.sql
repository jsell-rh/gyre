-- Task-198: persist the forge-maintained spec-link graph (spec-links.md
-- §Forge-Maintained Spec Graph). Exact 13-column schema from the spec.
--
-- target_sha is NOT NULL per spec; SpecLinkEntry.target_sha is Option<String>
-- for unresolved cross-workspace links — the adapters store the empty string
-- for that case (see sqlite/spec_links.rs / postgres/spec_links.rs).

CREATE TABLE IF NOT EXISTS spec_links (
    id              TEXT PRIMARY KEY,
    source_repo_id  TEXT NOT NULL DEFAULT '',
    source_path     TEXT NOT NULL,
    source_sha      TEXT NOT NULL DEFAULT '',
    link_type       TEXT NOT NULL,
    target_repo_id  TEXT,
    target_path     TEXT NOT NULL,
    target_sha      TEXT NOT NULL DEFAULT '',
    target_display  TEXT,
    reason          TEXT,
    status          TEXT NOT NULL DEFAULT 'active',
    created_at      INTEGER NOT NULL,
    stale_since     INTEGER
);

CREATE INDEX IF NOT EXISTS idx_spec_links_target_path ON spec_links (target_path);
CREATE INDEX IF NOT EXISTS idx_spec_links_source_repo_id ON spec_links (source_repo_id);
CREATE INDEX IF NOT EXISTS idx_spec_links_source_path ON spec_links (source_path);
