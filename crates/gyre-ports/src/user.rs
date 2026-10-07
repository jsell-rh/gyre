use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::User;

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn create(&self, user: &User) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<User>>;
    async fn find_by_external_id(&self, external_id: &str) -> Result<Option<User>>;
    async fn list(&self) -> Result<Vec<User>>;
    async fn update(&self, user: &User) -> Result<()>;
    /// Records the OIDC provider that just authenticated this user: the verified `iss` claim and
    /// the authentication time (HSI §12 "Auth provider info"). Server-written — no handler ever
    /// routes a client value here.
    ///
    /// The write is skipped unless `min_interval_secs` has elapsed since the stored
    /// `last_login_at` (or it is unset). OIDC tokens are validated on *every* request, so an
    /// unconditional write would make `users` the hottest table in the server to record a
    /// timestamp that is read a few times a day. Implementations MUST evaluate the interval and
    /// apply the write in one statement — a read-modify-write would let concurrent requests for
    /// the same user race.
    async fn record_login(
        &self,
        user_id: &Id,
        oidc_issuer: &str,
        at: u64,
        min_interval_secs: u64,
    ) -> Result<()>;
    async fn delete(&self, id: &Id) -> Result<()>;
}

/// API key → user_id mapping.
#[async_trait]
pub trait ApiKeyRepository: Send + Sync {
    async fn create(&self, key: &str, user_id: &Id, name: &str) -> Result<()>;
    async fn find_user_id(&self, key: &str) -> Result<Option<Id>>;
    async fn delete(&self, key: &str) -> Result<()>;
}
