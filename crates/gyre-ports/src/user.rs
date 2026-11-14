use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::User;

#[async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
pub trait UserRepository: Send + Sync {
    /// Persist a user.
    ///
    /// Fails if a user with the same `id`, the same `external_id`, or the
    /// same `username` already exists (username is unique per
    /// user-management.md §Username vs Display Name).
    async fn create(&self, user: &User) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<User>>;
    async fn find_by_external_id(&self, external_id: &str) -> Result<Option<User>>;
    /// Find a user by their unique, immutable, URL-safe username.
    async fn find_by_username(&self, username: &str) -> Result<Option<User>>;
    /// Find users by email address, case-insensitive.
    ///
    /// Email is NOT unique — the same person may hold accounts in
    /// multiple tenants (user-management.md §Tenant-Level User
    /// Onboarding cross-tenant model) — so every match is returned and
    /// callers MUST scope the result by tenant before using it.
    async fn find_by_email(&self, email: &str) -> Result<Vec<User>>;
    async fn list(&self) -> Result<Vec<User>>;
    /// Persist updated profile fields. `username` and `external_id` are
    /// immutable after creation; implementations MUST reject an update that
    /// changes either (user-management.md §Username vs Display Name).
    async fn update(&self, user: &User) -> Result<()>;
    /// Re-point a user's `external_id` at an SSO subject — the single
    /// sanctioned `external_id` transition.
    ///
    /// This claims a local-mode invitation account on the invitee's first
    /// SSO login (user-management.md §Tenant-Level User Onboarding step 3:
    /// "SSO mode: redirected to Keycloak login, then account linked"), so
    /// the pre-assigned memberships and invited GlobalRole attach to the
    /// SSO identity instead of a duplicate account. Fails if the user does
    /// not exist or another user already owns `new_external_id`.
    async fn claim_external_id(&self, user_id: &Id, new_external_id: &str) -> Result<()>;
    async fn delete(&self, id: &Id) -> Result<()>;
}

/// API key → user_id mapping.
#[async_trait]
pub trait ApiKeyRepository: Send + Sync {
    async fn create(&self, key: &str, user_id: &Id, name: &str) -> Result<()>;
    async fn find_user_id(&self, key: &str) -> Result<Option<Id>>;
    async fn delete(&self, key: &str) -> Result<()>;
}
