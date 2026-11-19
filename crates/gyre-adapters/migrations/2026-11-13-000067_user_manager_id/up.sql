-- TASK-265: SCIM enterprise extension (RFC 7643 §4.3) — the
-- `manager.value` attribute pushed by IdPs over SCIM
-- (identity-security.md §SCIM User Schema: the User resource carries
-- `urn:ietf:params:scim:schemas:extension:enterprise:2.0:User` with
-- `organization` (mapped to the existing users.tenant_id column) and
-- `manager.value` (this column)).
--
-- Nullable TEXT holding the manager's Gyre user id; NULL when the IdP has
-- not provisioned a manager. `organization` needs no column: it maps to
-- the tenant scope (users.tenant_id), which already exists.
--
-- Portability: plain ALTER TABLE ADD COLUMN with no dialect-only syntax,
-- runnable by both the SQLite and PostgreSQL embedded-migration runners.

ALTER TABLE users ADD COLUMN manager_id TEXT;
