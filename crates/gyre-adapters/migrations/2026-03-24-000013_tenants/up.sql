-- M34: Add tenants table (enterprise/org boundary).
-- Every workspace and user belongs to exactly one tenant.
CREATE TABLE IF NOT EXISTS tenants (
    id          TEXT    NOT NULL PRIMARY KEY,
    name        TEXT    NOT NULL,
    slug        TEXT    NOT NULL UNIQUE,
    oidc_issuer TEXT,
    budget      TEXT,       -- JSON BudgetConfig (optional)
    max_workspaces INTEGER,
    created_at  BIGINT  NOT NULL
);
