-- TASK-209: HSI §12 "Auth provider info (OIDC issuer, last login — read-only)".
--
-- Both columns are server-written during OIDC/JWT authentication
-- (auth::find_or_create_user -> UserRepository::record_login). No handler accepts
-- a client-supplied value for either, and PUT /api/v1/users/me has no field for
-- them, so they cannot be self-reported.
--
-- `last_login_at` already existed on the domain `User` struct but was never
-- persisted, so every read decoded it as None — a field that looked implemented
-- and always reported "never".
--
-- Both are nullable by necessity: a principal that only ever authenticated with
-- an API key or the global operator token has no OIDC issuer, and a user created
-- before this migration has no recorded login until their next OIDC sign-in.

ALTER TABLE users ADD COLUMN oidc_issuer TEXT;
ALTER TABLE users ADD COLUMN last_login_at BIGINT;
