-- TASK-099 F1: bind users to a tenant (platform-model.md §1: every user
-- belongs to exactly one tenant). Previously the admin user created by
-- POST /api/v1/users carried no tenant scope, so the auth extractor
-- fabricated tenant "default" for API-key auth — making the admin a global
-- superuser across tenants via the priority-900 policy.
--
-- Nullable because legacy rows (OIDC-provisioned users) have no binding
-- yet; the auth extractor fail-closes on unbound users rather than
-- fabricating a scope.

ALTER TABLE users ADD COLUMN tenant_id TEXT;

-- Revert TASK-099 F1: drop the user tenant binding.

ALTER TABLE users DROP COLUMN tenant_id;
