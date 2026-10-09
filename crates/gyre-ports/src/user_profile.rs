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

/// Per-user delivery-channel preferences (user-management.md §Delivery Channels).
///
/// One row per user: the serialized `NotificationChannels` config. The in_app
/// flag is always true and enforced in the domain type, not by storage.
#[async_trait]
pub trait UserChannelPreferenceRepository: Send + Sync {
    /// Returns the user's channel config, or None when the user has never
    /// configured channels (callers apply `NotificationChannels::default()`).
    async fn find(&self, user_id: &Id) -> Result<Option<gyre_domain::NotificationChannels>>;
    async fn upsert(&self, user_id: &Id, channels: &gyre_domain::NotificationChannels) -> Result<()>;
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
