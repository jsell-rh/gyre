use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{JudgmentEntry, JudgmentType, UserNotificationPreference, UserSession, UserToken};

#[async_trait]
pub trait UserNotificationPreferenceRepository: Send + Sync {
    async fn list_for_user(&self, user_id: &Id) -> Result<Vec<UserNotificationPreference>>;
    async fn upsert(&self, pref: &UserNotificationPreference) -> Result<()>;
    async fn upsert_batch(&self, prefs: &[UserNotificationPreference]) -> Result<()>;
}

#[async_trait]
pub trait UserTokenRepository: Send + Sync {
    async fn create(&self, token: &UserToken) -> Result<()>;
    async fn list_for_user(&self, user_id: &Id) -> Result<Vec<UserToken>>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<UserToken>>;
    async fn find_by_hash(&self, token_hash: &str) -> Result<Option<UserToken>>;
    async fn touch(&self, id: &Id, last_used_at: u64) -> Result<()>;
    async fn delete(&self, id: &Id, user_id: &Id) -> Result<()>;
}

/// User session tracking (user-management.md §Session Management).
///
/// Sessions are created on successful API-key auth, tracked per request
/// (throttled `last_active_at` updates), and can be revoked individually or
/// in bulk ("sign out everywhere").
#[async_trait]
pub trait SessionRepository: Send + Sync {
    /// Create a session. Fails if a session with the same `id` already exists.
    async fn create(&self, session: &UserSession) -> Result<()>;
    /// All sessions for a user (active and revoked; callers filter by state).
    async fn list_for_user(&self, user_id: &Id) -> Result<Vec<UserSession>>;
    /// Find a session by id.
    async fn find_by_id(&self, id: &Id) -> Result<Option<UserSession>>;
    /// Find a session by token hash (used for per-request session resolution).
    async fn find_by_token_hash(&self, token_hash: &str) -> Result<Option<UserSession>>;
    /// Find the session a credential presents from a given device: same
    /// user, same credential hash, same client IP and User-Agent.
    ///
    /// API keys are long-lived, so one key presented from many devices must
    /// not collapse into one session row (revoking one device would sign out
    /// every device). Conversely, the same key re-presented from the same
    /// device IS the same session — the auth path uses this to avoid
    /// creating a row per request. Adapters match ALL of (user_id,
    /// credential_hash, ip_address, user_agent).
    async fn find_by_credential_and_device(
        &self,
        user_id: &Id,
        credential_hash: &str,
        ip_address: &str,
        user_agent: &str,
    ) -> Result<Option<UserSession>>;
    /// Update `last_active_at` (throttled by the caller to ≤ once per minute).
    async fn touch(&self, id: &Id, last_active_at: u64) -> Result<()>;
    /// Revoke a single session. No-op if already revoked or not found.
    async fn revoke(&self, id: &Id, user_id: &Id) -> Result<()>;
    /// Revoke every session for a user ("sign out everywhere").
    async fn revoke_all_for_user(&self, user_id: &Id) -> Result<()>;
    /// Delete sessions that expired before `cutoff` (retention cleanup).
    /// Returns the number of rows deleted.
    async fn delete_expired_before(&self, cutoff: u64) -> Result<u64>;
}

/// Read-only aggregated view of a user's judgment history.
#[async_trait]
pub trait JudgmentLedgerRepository: Send + Sync {
    async fn list_for_user(
        &self,
        approver_id: &str,
        workspace_id: Option<&Id>,
        judgment_type: Option<JudgmentType>,
        since: Option<u64>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<JudgmentEntry>>;
}
