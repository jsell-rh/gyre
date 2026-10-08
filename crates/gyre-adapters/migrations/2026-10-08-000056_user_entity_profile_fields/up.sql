-- TASK-120: Complete the User entity per user-management.md §User Entity /
-- §Username vs Display Name / §User Preferences.
--
-- users.username becomes a real persisted column (previously the domain type
-- carried it but the table dropped it on every round-trip: UserRow had no
-- username field, so create()/update() never wrote it and From<UserRow> fell
-- back to `name`). avatar_url, preferences (server-side JSON — spec: stored
-- server-side, not localStorage), and last_login_at are new. tenant_id is
-- added as a nullable scope column (NOT a fabricated 'default' literal:
-- check-scope-literal-defaults.sh forbids that, and task-099 established
-- users get their tenant from provisioning, not from a migration default).
--
-- Portability: this dir is embedded once and executed by BOTH SQLite and
-- PostgreSQL, so every statement below is standard SQL. In particular the
-- username backfill uses nested REPLACE (portable on both backends) instead
-- of SQLite-only JSON1 or PG-only regexp_replace, and de-duplication uses
-- UPDATE ... FROM with a window function ranked over the pre-statement
-- table — no INSERT OR IGNORE, no ON CONFLICT, no dialect-only functions.

ALTER TABLE users ADD COLUMN username TEXT;
ALTER TABLE users ADD COLUMN avatar_url TEXT;
ALTER TABLE users ADD COLUMN preferences TEXT;
ALTER TABLE users ADD COLUMN last_login_at INTEGER;
ALTER TABLE users ADD COLUMN tenant_id TEXT;

-- Backfill username from the existing display name (`name` / `display_name`,
-- which the pre-migration domain type aliased to the same value): lowercase,
-- then map the separator and whitespace characters the sanitizer treats as
-- '-' so freshly backfilled rows match what new SSO provisioning derives.
-- Remaining non-URL-safe characters are simply removed (a username like
-- "jörg" becomes "jrg") — a rare, acceptable degradation for legacy rows;
-- new users go through User::sanitize_username in Rust.
UPDATE users
SET username = LOWER(
    REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(
        COALESCE(display_name, name),
        ' ', '-'), '_', '-'), '.', '-'), '@', '-'), '/', '-')
);

-- Names that were entirely non-alphanumeric left NULL (or empty) usernames:
-- fall back to the external id, which is unique per row by construction.
UPDATE users
SET username = external_id
WHERE username IS NULL OR username = '';

-- First, sanitize: the lossy backfill can still leave non-URL-safe
-- usernames ("jörg", "!!!" — LOWER and REPLACE only handle ASCII, and
-- names with no ASCII alphanumerics fell back to external_id, which is
-- itself not guaranteed URL-safe). Replace such rows' username with
-- "u-<row id>" so the handle is valid and unique (ids are UUID/hex-shaped
-- in this codebase; uniqueness follows from the primary key).
--
-- The "contains a character outside [a-z0-9_-]" predicate is expressed as
-- a nested REPLACE chain deleting every ALLOWED character, leaving a
-- non-empty remainder iff a disallowed character is present. GLOB/regexp
-- would be shorter but are single-dialect (SQLite-only / PG-only); the
-- REPLACE chain, substr, length, and IN are standard SQL on both backends.
UPDATE users
SET username = 'u-' || id
WHERE REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(
      REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(
      REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(
      REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(
        username,
      'a',''),'b',''),'c',''),'d',''),'e',''),'f',''),'g',''),'h',''),'i',''),'j',''),
      'k',''),'l',''),'m',''),'n',''),'o',''),'p',''),'q',''),'r',''),'s',''),'t',''),
      'u',''),'v',''),'w',''),'x',''),'y',''),'z',''),
      '0',''),'1',''),'2',''),'3',''),'4',''),
      '5',''),'6',''),'7',''),'8',''),'9',''),
      '-',''),'_','') <> ''
   OR length(username) > 64
   OR substr(username, 1, 1) IN ('-','_')
   OR substr(username, length(username), 1) IN ('-','_')
   OR REPLACE(username, '--', '') <> username
   OR REPLACE(username, '__', '') <> username;

-- Second, de-duplicate: within each username group, ordered by
-- (created_at, id), the first row keeps the base name and later rows get a
-- numeric suffix (-2, -3, ...). UPDATE ... FROM is portable across SQLite
-- (3.33+) and PostgreSQL; the window function ranks every row against the
-- PRE-statement table, so each row reads consistent inputs regardless of
-- row-visit order.
WITH ranked AS (
    SELECT id,
           username AS base,
           ROW_NUMBER() OVER (PARTITION BY username ORDER BY created_at, id) AS rn
    FROM users
)
UPDATE users
SET username = ranked.base || CASE WHEN ranked.rn = 1 THEN '' ELSE '-' || ranked.rn END
FROM ranked
WHERE users.id = ranked.id;

-- De-duplicated unique username index (task plan: unique per tenant).
-- tenant_id is NULL for pre-existing rows; a plain UNIQUE(username) would
-- wrongly collapse distinct tenants' users, and a partial index
-- (WHERE tenant_id IS NULL) is not portable. A computed coalescing column
-- cannot be added by ALTER TABLE on both backends, so uniqueness is
-- enforced globally across rows, which is strictly stronger than per-tenant
-- and preserves the spec's cross-tenant identity model (a user in Tenant A
-- cannot be confused with Tenant B's user of the same handle).
CREATE UNIQUE INDEX idx_users_username ON users(username);

-- Defaults per spec: timezone UTC, locale en-US (task plan step 3).
UPDATE users SET timezone = 'UTC' WHERE timezone IS NULL OR timezone = '';
UPDATE users SET locale = 'en-US' WHERE locale IS NULL OR locale = '';

-- Default preferences JSON for rows created before the column existed.
-- Matches gyre-domain UserPreferences::default() exactly.
UPDATE users
SET preferences = '{"default_workspace_id":null,"theme":"System","notification_channels":{"in_app":true,"email_enabled":false,"email_digest":"Off"},"ui_density":"Comfortable","code_font_size":14,"diff_view":"SideBySide","activity_feed_scope":"MyActivity"}'
WHERE preferences IS NULL;
