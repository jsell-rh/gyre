use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::{Id, Notification, NotificationType};
use gyre_ports::NotificationRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::notifications;

// ── Row types ─────────────────────────────────────────────────────────────────

#[derive(Queryable, Selectable)]
#[diesel(table_name = notifications)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct NotificationRow {
    id: String,
    workspace_id: String,
    user_id: String,
    notification_type: String,
    priority: i32,
    title: String,
    body: Option<String>,
    entity_ref: Option<String>,
    repo_id: Option<String>,
    resolved_at: Option<i64>,
    dismissed_at: Option<i64>,
    created_at: i64,
    tenant_id: String,
}

impl NotificationRow {
    fn into_notification(self) -> Result<Notification> {
        let ntype = NotificationType::parse(&self.notification_type)
            .ok_or_else(|| anyhow!("unknown notification_type: {}", self.notification_type))?;
        Ok(Notification {
            id: Id::new(self.id),
            workspace_id: Id::new(self.workspace_id),
            user_id: Id::new(self.user_id),
            notification_type: ntype,
            priority: self.priority.clamp(1, 10) as u8,
            title: self.title,
            body: self.body,
            entity_ref: self.entity_ref,
            repo_id: self.repo_id,
            resolved_at: self.resolved_at,
            dismissed_at: self.dismissed_at,
            created_at: self.created_at,
            tenant_id: self.tenant_id,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = notifications)]
struct NewNotificationRow<'a> {
    id: &'a str,
    workspace_id: &'a str,
    user_id: &'a str,
    notification_type: &'a str,
    priority: i32,
    title: &'a str,
    body: Option<&'a str>,
    entity_ref: Option<&'a str>,
    repo_id: Option<&'a str>,
    resolved_at: Option<i64>,
    dismissed_at: Option<i64>,
    created_at: i64,
    tenant_id: &'a str,
}

// ── Repository impl ───────────────────────────────────────────────────────────

#[async_trait]
impl NotificationRepository for SqliteStorage {
    async fn create(&self, notification: &Notification) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let n = notification.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewNotificationRow {
                id: n.id.as_str(),
                workspace_id: n.workspace_id.as_str(),
                user_id: n.user_id.as_str(),
                notification_type: n.notification_type.as_str(),
                priority: n.priority as i32,
                title: &n.title,
                body: n.body.as_deref(),
                entity_ref: n.entity_ref.as_deref(),
                repo_id: n.repo_id.as_deref(),
                resolved_at: n.resolved_at,
                dismissed_at: n.dismissed_at,
                created_at: n.created_at,
                tenant_id: &n.tenant_id,
            };
            diesel::insert_into(notifications::table)
                .values(&row)
                .on_conflict(notifications::id)
                .do_nothing()
                .execute(&mut *conn)
                .context("insert notification")?;
            Ok(())
        })
        .await?
    }

    async fn get(&self, id: &Id, user_id: &Id) -> Result<Option<Notification>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        let uid = user_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Notification>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = notifications::table
                .filter(notifications::id.eq(id.as_str()))
                .filter(notifications::user_id.eq(uid.as_str()))
                .first::<NotificationRow>(&mut *conn)
                .optional()
                .context("get notification")?;
            result.map(NotificationRow::into_notification).transpose()
        })
        .await?
    }

    async fn list_for_user(
        &self,
        user_id: &Id,
        workspace_id: Option<&Id>,
        min_priority: Option<u8>,
        max_priority: Option<u8>,
        notification_type: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Notification>> {
        let pool = Arc::clone(&self.pool);
        let uid = user_id.clone();
        let ws_id = workspace_id.cloned();
        let ntype = notification_type.map(|s| s.to_string());
        tokio::task::spawn_blocking(move || -> Result<Vec<Notification>> {
            let mut conn = pool.get().context("get db connection")?;
            let mut query = notifications::table
                .filter(notifications::user_id.eq(uid.as_str()))
                .order(notifications::priority.asc())
                .then_order_by(notifications::created_at.desc())
                .into_boxed();
            if let Some(ref ws) = ws_id {
                query = query.filter(notifications::workspace_id.eq(ws.as_str()));
            }
            if let Some(min_p) = min_priority {
                query = query.filter(notifications::priority.ge(min_p as i32));
            }
            if let Some(max_p) = max_priority {
                query = query.filter(notifications::priority.le(max_p as i32));
            }
            if let Some(ref nt) = ntype {
                query = query.filter(notifications::notification_type.eq(nt));
            }
            let rows = query
                .limit(limit as i64)
                .offset(offset as i64)
                .load::<NotificationRow>(&mut *conn)
                .context("list notifications")?;
            rows.into_iter()
                .map(NotificationRow::into_notification)
                .collect()
        })
        .await?
    }

    async fn dismiss(&self, id: &Id, user_id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        let uid = user_id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            diesel::update(
                notifications::table
                    .filter(notifications::id.eq(id.as_str()))
                    .filter(notifications::user_id.eq(uid.as_str())),
            )
            .set(notifications::dismissed_at.eq(now))
            .execute(&mut *conn)
            .context("dismiss notification")?;
            Ok(())
        })
        .await?
    }

    async fn resolve(&self, id: &Id, user_id: &Id, _action_taken: Option<&str>) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        let uid = user_id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            diesel::update(
                notifications::table
                    .filter(notifications::id.eq(id.as_str()))
                    .filter(notifications::user_id.eq(uid.as_str())),
            )
            .set(notifications::resolved_at.eq(now))
            .execute(&mut *conn)
            .context("resolve notification")?;
            Ok(())
        })
        .await?
    }

    async fn count_unresolved(&self, user_id: &Id, workspace_id: Option<&Id>) -> Result<u64> {
        let pool = Arc::clone(&self.pool);
        let uid = user_id.clone();
        let ws_id = workspace_id.cloned();
        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            let mut query = notifications::table
                .filter(notifications::user_id.eq(uid.as_str()))
                .filter(notifications::resolved_at.is_null())
                .filter(notifications::dismissed_at.is_null())
                .into_boxed();
            if let Some(ref ws) = ws_id {
                query = query.filter(notifications::workspace_id.eq(ws.as_str()));
            }
            let count = query
                .count()
                .get_result::<i64>(&mut *conn)
                .context("count unresolved notifications")?;
            Ok(count as u64)
        })
        .await?
    }

    async fn list_recent(&self, limit: usize) -> Result<Vec<Notification>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<Notification>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows: Vec<NotificationRow> = notifications::table
                .order(notifications::created_at.desc())
                .limit(limit as i64)
                .select(NotificationRow::as_select())
                .load(&mut *conn)
                .context("list_recent")?;
            rows.into_iter().map(|r| r.into_notification()).collect()
        })
        .await?
    }

    async fn has_recent_dismissal(
        &self,
        workspace_id: &Id,
        user_id: &Id,
        notification_type: &str,
        days: u32,
    ) -> Result<bool> {
        let pool = Arc::clone(&self.pool);
        let ws_id = workspace_id.clone();
        let uid = user_id.clone();
        let ntype = notification_type.to_string();
        tokio::task::spawn_blocking(move || -> Result<bool> {
            let mut conn = pool.get().context("get db connection")?;
            let cutoff = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64
                - (days as i64 * 86400);
            let count = notifications::table
                .filter(notifications::workspace_id.eq(ws_id.as_str()))
                .filter(notifications::user_id.eq(uid.as_str()))
                .filter(notifications::notification_type.eq(&ntype))
                .filter(notifications::dismissed_at.ge(cutoff))
                .count()
                .get_result::<i64>(&mut *conn)
                .context("has_recent_dismissal")?;
            Ok(count > 0)
        })
        .await?
    }
}
