use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{
    JudgmentEntry, JudgmentType, UserNotificationPreference, UserSession, UserToken,
};
use gyre_ports::{
    JudgmentLedgerRepository, SessionRepository, UserNotificationPreferenceRepository,
    UserTokenRepository,
};

use super::PgStorage;

// Stub implementations for PgStorage — full implementation deferred to future milestone.
// SQLite (SqliteStorage) is the primary backend for these new port traits.

#[async_trait]
impl UserNotificationPreferenceRepository for PgStorage {
    async fn list_for_user(&self, _user_id: &Id) -> Result<Vec<UserNotificationPreference>> {
        anyhow::bail!("UserNotificationPreferenceRepository not implemented for PgStorage")
    }

    async fn upsert(&self, _pref: &UserNotificationPreference) -> Result<()> {
        anyhow::bail!("UserNotificationPreferenceRepository not implemented for PgStorage")
    }

    async fn upsert_batch(&self, _prefs: &[UserNotificationPreference]) -> Result<()> {
        anyhow::bail!("UserNotificationPreferenceRepository not implemented for PgStorage")
    }
}

#[async_trait]
impl UserTokenRepository for PgStorage {
    async fn create(&self, _token: &UserToken) -> Result<()> {
        anyhow::bail!("UserTokenRepository not implemented for PgStorage")
    }

    async fn list_for_user(&self, _user_id: &Id) -> Result<Vec<UserToken>> {
        anyhow::bail!("UserTokenRepository not implemented for PgStorage")
    }

    async fn find_by_id(&self, _id: &Id) -> Result<Option<UserToken>> {
        anyhow::bail!("UserTokenRepository not implemented for PgStorage")
    }

    async fn find_by_hash(&self, _token_hash: &str) -> Result<Option<UserToken>> {
        anyhow::bail!("UserTokenRepository not implemented for PgStorage")
    }

    async fn touch(&self, _id: &Id, _last_used_at: u64) -> Result<()> {
        anyhow::bail!("UserTokenRepository not implemented for PgStorage")
    }

    async fn delete(&self, _id: &Id, _user_id: &Id) -> Result<()> {
        anyhow::bail!("UserTokenRepository not implemented for PgStorage")
    }
}

#[async_trait]
impl JudgmentLedgerRepository for PgStorage {
    async fn list_for_user(
        &self,
        _approver_id: &str,
        _workspace_id: Option<&Id>,
        _judgment_type: Option<JudgmentType>,
        _since: Option<u64>,
        _limit: u32,
        _offset: u32,
    ) -> Result<Vec<JudgmentEntry>> {
        anyhow::bail!("JudgmentLedgerRepository not implemented for PgStorage")
    }
}

#[async_trait]
impl SessionRepository for PgStorage {
    async fn create(&self, _session: &UserSession) -> Result<()> {
        anyhow::bail!("SessionRepository not implemented for PgStorage")
    }

    async fn list_for_user(&self, _user_id: &Id) -> Result<Vec<UserSession>> {
        anyhow::bail!("SessionRepository not implemented for PgStorage")
    }

    async fn find_by_id(&self, _id: &Id) -> Result<Option<UserSession>> {
        anyhow::bail!("SessionRepository not implemented for PgStorage")
    }

    async fn find_by_token_hash(&self, _token_hash: &str) -> Result<Option<UserSession>> {
        anyhow::bail!("SessionRepository not implemented for PgStorage")
    }

    async fn find_by_credential_and_device(
        &self,
        _user_id: &Id,
        _credential_hash: &str,
        _ip_address: &str,
        _user_agent: &str,
    ) -> Result<Option<UserSession>> {
        anyhow::bail!("SessionRepository not implemented for PgStorage")
    }

    async fn touch(&self, _id: &Id, _last_active_at: u64) -> Result<()> {
        anyhow::bail!("SessionRepository not implemented for PgStorage")
    }

    async fn revoke(&self, _id: &Id, _user_id: &Id) -> Result<()> {
        anyhow::bail!("SessionRepository not implemented for PgStorage")
    }

    async fn revoke_all_for_user(&self, _user_id: &Id) -> Result<()> {
        anyhow::bail!("SessionRepository not implemented for PgStorage")
    }

    async fn delete_expired_before(&self, _cutoff: u64) -> Result<u64> {
        anyhow::bail!("SessionRepository not implemented for PgStorage")
    }
}
