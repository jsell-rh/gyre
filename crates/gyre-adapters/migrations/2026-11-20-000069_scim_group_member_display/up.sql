-- TASK-266 repair: persist the IdP-pushed RFC 7643 §2.4 `display`
-- sub-attribute of each SCIM group member reference.
--
-- The RFC 7644 Group schema's members carry {value, display}; the value
-- resolves to a Gyre user id, and `display` is the human-readable name the
-- IdP holds for that member. Storing it on the membership row lets group
-- reads round-trip exactly what the IdP pushed (the member user's Gyre
-- display name is only a fallback when the push carried no `display` —
-- deriving it unconditionally silently drops the IdP's value).
--
-- Nullable: pushes without a `display` (and every row written before this
-- migration) simply have none stored.
--
-- Portability: plain ALTER TABLE ADD COLUMN, runnable by both the SQLite
-- and PostgreSQL embedded-migration runners (same shape as 000062/000067).

ALTER TABLE scim_group_members ADD COLUMN display TEXT;
