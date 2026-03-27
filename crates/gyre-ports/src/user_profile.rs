use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{JudgmentEntry, JudgmentType, UserNotificationPreference, UserToken};

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
