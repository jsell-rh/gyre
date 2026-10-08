use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::User;

#[async_trait]
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
    async fn list(&self) -> Result<Vec<User>>;
    /// Persist updated profile fields. `username` and `external_id` are
    /// immutable after creation; implementations MUST reject an update that
    /// changes either (user-management.md §Username vs Display Name).
    async fn update(&self, user: &User) -> Result<()>;
    async fn delete(&self, id: &Id) -> Result<()>;
}

/// API key → user_id mapping.
#[async_trait]
pub trait ApiKeyRepository: Send + Sync {
    async fn create(&self, key: &str, user_id: &Id, name: &str) -> Result<()>;
    async fn find_user_id(&self, key: &str) -> Result<Option<Id>>;
    async fn delete(&self, key: &str) -> Result<()>;
}
