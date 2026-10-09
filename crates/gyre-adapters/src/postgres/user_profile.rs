use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{JudgmentEntry, JudgmentType, UserNotificationPreference, UserToken};
use gyre_ports::{
    JudgmentLedgerRepository, UserChannelPreferenceRepository,
    UserNotificationPreferenceRepository, UserTokenRepository,
};
use std::sync::Arc;

use super::PgStorage;
use crate::schema::user_channel_preferences;

// UserNotificationPreferenceRepository / UserTokenRepository /
// JudgmentLedgerRepository stubs predate task-112 (see git history); SQLite
// is the primary backend for those port traits.

// ─── User Channel Preferences (task-112, user-management.md §Delivery Channels) ──

#[derive(Queryable, Selectable)]
#[diesel(table_name = user_channel_preferences)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct ChannelPrefRow {
    #[allow(dead_code)]
    user_id: String,
    channels: String,
    #[allow(dead_code)]
    updated_at: i64,
}

#[derive(Insertable)]
#[diesel(table_name = user_channel_preferences)]
struct ChannelPrefRecord<'a> {
    user_id: &'a str,
    channels: &'a str,
    updated_at: i64,
}

#[async_trait]
impl UserChannelPreferenceRepository for PgStorage {
    async fn find(&self, user_id: &Id) -> Result<Option<gyre_domain::NotificationChannels>> {
        let pool = Arc::clone(&self.pool);
        let uid = user_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<gyre_domain::NotificationChannels>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = user_channel_preferences::table
                .find(&uid)
                .first::<ChannelPrefRow>(&mut *conn)
                .optional()
                .context("find user channel preferences")?;
            match row {
                Some(r) => serde_json::from_str(&r.channels)
                    .map(Some)
                    .map_err(|e| anyhow::anyhow!("corrupt channel preferences for {uid}: {e}")),
                None => Ok(None),
            }
        })
        .await?
    }

    async fn upsert(
        &self,
        user_id: &Id,
        channels: &gyre_domain::NotificationChannels,
    ) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let uid = user_id.as_str().to_string();
        let json = serde_json::to_string(channels).context("serialize channel preferences")?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let record = ChannelPrefRecord {
                user_id: &uid,
                channels: &json,
                updated_at: now,
            };
            diesel::insert_into(user_channel_preferences::table)
                .values(&record)
                .on_conflict(user_channel_preferences::user_id)
                .do_update()
                .set((
                    user_channel_preferences::channels.eq(&json),
                    user_channel_preferences::updated_at.eq(now),
                ))
                .execute(&mut *conn)
                .context("upsert user channel preferences")?;
            Ok(())
        })
        .await?
    }
}

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
