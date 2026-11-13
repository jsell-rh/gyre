-- Reverse of 000062: restore the member_ids JSON column (empty — membership
-- data added through team_memberships is not representable in it) and drop
-- the slug/lead columns, the workspace-slug unique index, and the
-- team_memberships table.
ALTER TABLE teams ADD COLUMN member_ids TEXT NOT NULL DEFAULT '[]';
DROP INDEX idx_team_memberships_user;
DROP TABLE team_memberships;
DROP INDEX idx_teams_workspace_slug;
ALTER TABLE teams DROP COLUMN lead;
ALTER TABLE teams DROP COLUMN slug;
