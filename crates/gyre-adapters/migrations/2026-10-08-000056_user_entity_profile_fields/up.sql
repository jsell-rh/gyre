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
ALTER TABLE users ADD COLUMN global_role TEXT NOT NULL DEFAULT 'Member';

-- Backfill username from the existing display name (`name` / `display_name`,
-- which the pre-migration domain type aliased to the same value): lowercase,
-- then map the separator and whitespace characters the sanitizer treats as
-- '-' so freshly backfilled rows match what new SSO provisioning derives.
-- Remaining non-URL-safe characters can survive the REPLACE pass (LOWER and
-- REPLACE only handle ASCII); rows whose handle is still not URL-safe after
-- this backfill are renamed to "u-<row id>" by the sanitize pass below —
-- the handle contract is never violated for legacy rows, at the cost of a
-- degraded (but unique, valid) handle; new users go through
-- User::sanitize_username in Rust.
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
-- The "contains a character outside [a-z0-9_-]" predicate deletes every
-- ALLOWED character and tests whether a remainder is left. Expressing
-- that deletion as one nested REPLACE(...) chain (39 levels deep)
-- overflows SQLite's SQL parser — "parser stack overflow", reproduced
-- against both the bundled libsqlite3-sys 0.28 and system SQLite 3.50 —
-- and SqliteStorage::new runs this file on every construction, so the
-- default deployment could not boot. Instead the deletion runs as 38
-- sequential depth-1 UPDATE statements over a scratch column, and the
-- final predicate reads the scratch remainder. GLOB/regexp would be
-- shorter but are single-dialect (SQLite-only / PG-only); REPLACE,
-- substr, length, and IN are standard SQL on both backends. DROP COLUMN
-- is available on SQLite >= 3.35 (already relied on by 000036's
-- down.sql) and on PostgreSQL.
ALTER TABLE users ADD COLUMN tmp_username_scratch TEXT;
UPDATE users SET tmp_username_scratch = username;
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'a', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'b', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'c', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'd', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'e', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'f', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'g', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'h', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'i', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'j', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'k', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'l', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'm', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'n', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'o', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'p', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'q', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'r', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 's', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 't', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'u', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'v', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'w', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'x', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'y', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, 'z', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, '0', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, '1', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, '2', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, '3', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, '4', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, '5', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, '6', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, '7', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, '8', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, '9', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, '-', '');
UPDATE users SET tmp_username_scratch = REPLACE(tmp_username_scratch, '_', '');
UPDATE users
SET username = 'u-' || id
WHERE username IS NULL
   OR username = ''
   OR tmp_username_scratch <> ''
   OR length(username) > 64
   OR substr(username, 1, 1) IN ('-','_')
   OR substr(username, length(username), 1) IN ('-','_')
   OR REPLACE(username, '--', '') <> username
   OR REPLACE(username, '__', '') <> username;
ALTER TABLE users DROP COLUMN tmp_username_scratch;

-- Second, de-duplicate: within each username group, ordered by
-- (created_at, id), the first row keeps the base name and later rows get
-- a numeric suffix (-2, -3, ...). UPDATE ... FROM is portable across SQLite
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

-- Pass 1's "-<n>" suffix can itself collide with a distinct pre-existing
-- handle: two rows named "jsell" dedup to jsell/jsell-2, and a third row
-- already literally named "jsell-2" would abort the migration at CREATE
-- UNIQUE INDEX below. Pass 2 re-ranks the post-pass-1 table; any row still
-- sharing a handle is renamed to "<handle>-<row id>", unique by primary
-- key, so the unique index cannot fail on dedup collisions. Deterministic
-- rule throughout: within a colliding group the earliest-created row wins
-- the shorter handle. A residual collision after pass 2 would require a
-- legacy display name that sanitizes to exactly "<handle>-<uuid of another
-- row>" — not constructible without foreknowledge of generated row ids.
WITH ranked AS (
    SELECT id,
           username AS base,
           ROW_NUMBER() OVER (PARTITION BY username ORDER BY created_at, id) AS rn
    FROM users
)
UPDATE users
SET username = ranked.base || CASE WHEN ranked.rn = 1 THEN '' ELSE '-' || ranked.id END
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

-- Defaults per spec: timezone UTC, locale en-US (task plan step 3), and
-- global_role Member for rows predating the column (admins are granted via
-- the bootstrap path, which re-stamps the role on update).
UPDATE users SET timezone = 'UTC' WHERE timezone IS NULL OR timezone = '';
UPDATE users SET locale = 'en-US' WHERE locale IS NULL OR locale = '';
UPDATE users SET global_role = 'Member' WHERE global_role IS NULL OR global_role = '';

-- Default preferences JSON for rows created before the column existed.
-- Matches gyre-domain UserPreferences::default() exactly.
UPDATE users
SET preferences = '{"default_workspace_id":null,"theme":"System","notification_channels":{"in_app":true,"email_enabled":false,"email_digest":"Off"},"ui_density":"Comfortable","code_font_size":14,"diff_view":"SideBySide","activity_feed_scope":"MyActivity"}'
WHERE preferences IS NULL;
