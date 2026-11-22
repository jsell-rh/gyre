-- task-122 (user-management.md §Team Management): teams gain a
-- workspace-unique URL-safe `slug` and a `lead` (the user responsible for
-- the team); membership moves from the `member_ids` JSON column to a
-- `team_memberships` table so each membership carries its own `added_at`.
--
-- All SQL below runs on BOTH SQLite and PostgreSQL (shared embedded
-- migrations dir — see scripts/check-migration-sql-portability.sh).

-- 1. New columns (default '' so existing rows pass the NOT NULL constraint;
--    adapters always write real values).
ALTER TABLE teams ADD COLUMN slug TEXT NOT NULL DEFAULT '';
ALTER TABLE teams ADD COLUMN lead TEXT NOT NULL DEFAULT '';

-- 2. Backfill slug: legacy rows get a stable, unique, URL-safe slug derived
--    from the team id (uuid hex). The member_ids JSON column was never
--    writable through any endpoint (no membership endpoints existed before
--    this migration), so there is no membership data to carry over — and
--    JSON parsing is not portable SQL in any case.
UPDATE teams SET slug = id WHERE slug = '';

-- 3. Backfill lead: the workspace Owner at migration time (every workspace
--    created through the API has one). The empty-string fallback only
--    applies to orphaned rows with no membership at all.
UPDATE teams SET lead = COALESCE(
    (SELECT m.user_id FROM workspace_memberships m
      WHERE m.workspace_id = teams.workspace_id AND m.role = 'Owner'
      ORDER BY m.created_at ASC, m.id ASC LIMIT 1),
    '')
WHERE lead = '';

-- 4. Slug is unique within a workspace.
CREATE UNIQUE INDEX idx_teams_workspace_slug ON teams (workspace_id, slug);

-- 5. Team membership (user-management.md §Team Management:
--    TeamMembership { team_id, user_id, added_at }).
CREATE TABLE team_memberships (
    team_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    added_at BIGINT NOT NULL,
    PRIMARY KEY (team_id, user_id)
);
CREATE INDEX idx_team_memberships_user ON team_memberships (user_id);

-- 6. The JSON id list is superseded by team_memberships.
ALTER TABLE teams DROP COLUMN member_ids;
