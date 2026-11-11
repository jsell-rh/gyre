use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::{Id, Notification, NotificationSource, NotificationType};
use gyre_ports::NotificationRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::notifications;

// ── Row types ─────────────────────────────────────────────────────────────────

#[derive(Queryable, Selectable)]
#[diesel(table_name = notifications)]
#[diesel(check_for_backend(diesel::pg::Pg))]
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
    entity_type: Option<String>,
    action_url: Option<String>,
    read: i32,
    read_at: Option<i64>,
    resolved_at: Option<i64>,
    dismissed_at: Option<i64>,
    created_at: i64,
    tenant_id: String,
}

impl NotificationRow {
    fn into_notification(self) -> Result<Notification> {
        let ntype = NotificationType::parse(&self.notification_type)
            .ok_or_else(|| anyhow!("unknown notification_type: {}", self.notification_type))?;
        let priority_rank = self.priority.clamp(1, 10) as u8;
        // Rows written before migration 000059 have no entity_type: their
        // source entity id survives in entity_ref, but its kind was never
        // recorded — map to the generic "system" source kind rather than
        // fabricating a specific one.
        let source = NotificationSource {
            entity_type: self.entity_type.unwrap_or_else(|| "system".to_string()),
            entity_id: self.entity_ref.unwrap_or_default(),
            workspace_id: Id::new(self.workspace_id),
            repo_id: self.repo_id.map(Id::new),
        };
        Ok(Notification {
            id: Id::new(self.id),
            workspace_id: source.workspace_id.clone(),
            user_id: Id::new(self.user_id),
            notification_type: ntype,
            priority: gyre_common::NotificationPriority::from_rank(priority_rank),
            priority_rank,
            title: self.title,
            body: self.body,
            source,
            action_url: self.action_url,
            read: self.read != 0,
            read_at: self.read_at,
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
    entity_type: Option<&'a str>,
    action_url: Option<&'a str>,
    read: i32,
    read_at: Option<i64>,
    resolved_at: Option<i64>,
    dismissed_at: Option<i64>,
    created_at: i64,
    tenant_id: &'a str,
}

// ── Repository impl ───────────────────────────────────────────────────────────

#[async_trait]
impl NotificationRepository for PgStorage {
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
                priority: n.priority_rank as i32,
                title: &n.title,
                body: n.body.as_deref(),
                entity_ref: Some(n.source.entity_id.as_str()),
                repo_id: n.source.repo_id.as_ref().map(|r| r.as_str()),
                entity_type: Some(n.source.entity_type.as_str()),
                action_url: n.action_url.as_deref(),
                read: n.read as i32,
                read_at: n.read_at,
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
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Notification>> {
            let mut conn = pool.get().context("get db connection")?;
            // Spec hierarchy-enforcement.md §3: every read filters by tenant_id.
            let result = notifications::table
                .filter(notifications::tenant_id.eq(&tenant))
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
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Notification>> {
            let mut conn = pool.get().context("get db connection")?;
            let mut query = notifications::table
                .filter(notifications::tenant_id.eq(&tenant))
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

    async fn mark_read(&self, id: &Id, user_id: &Id, read: bool) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        let uid = user_id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            // `read = true` stamps read_at; `read = false` clears both so an
            // unread notification has no stale read timestamp.
            let (read_val, read_at): (i32, Option<i64>) =
                if read { (1, Some(now)) } else { (0, None) };
            diesel::update(
                notifications::table
                    .filter(notifications::id.eq(id.as_str()))
                    .filter(notifications::user_id.eq(uid.as_str())),
            )
            .set((
                notifications::read.eq(read_val),
                notifications::read_at.eq(read_at),
            ))
            .execute(&mut *conn)
            .context("mark notification read")?;
            Ok(())
        })
        .await?
    }

    async fn count_unresolved(&self, user_id: &Id, workspace_id: Option<&Id>) -> Result<u64> {
        let pool = Arc::clone(&self.pool);
        let uid = user_id.clone();
        let ws_id = workspace_id.cloned();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            let mut query = notifications::table
                .filter(notifications::tenant_id.eq(&tenant))
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
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Notification>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows: Vec<NotificationRow> = notifications::table
                .filter(notifications::tenant_id.eq(&tenant))
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
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<bool> {
            let mut conn = pool.get().context("get db connection")?;
            let cutoff = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64
                - (days as i64 * 86400);
            let count = notifications::table
                .filter(notifications::tenant_id.eq(&tenant))
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

    async fn has_active_notification(
        &self,
        workspace_id: &Id,
        user_id: &Id,
        notification_type: &str,
        entity_ref: &str,
        since: i64,
    ) -> Result<bool> {
        let pool = Arc::clone(&self.pool);
        let ws_id = workspace_id.clone();
        let uid = user_id.clone();
        let ntype = notification_type.to_string();
        let eref = entity_ref.to_string();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<bool> {
            let mut conn = pool.get().context("get db connection")?;
            let count = notifications::table
                .filter(notifications::tenant_id.eq(&tenant))
                .filter(notifications::workspace_id.eq(ws_id.as_str()))
                .filter(notifications::user_id.eq(uid.as_str()))
                .filter(notifications::notification_type.eq(&ntype))
                .filter(notifications::entity_ref.eq(&eref))
                .filter(notifications::resolved_at.is_null())
                .filter(notifications::dismissed_at.is_null())
                .filter(notifications::created_at.ge(since))
                .count()
                .get_result::<i64>(&mut *conn)
                .context("has_active_notification")?;
            Ok(count > 0)
        })
        .await?
    }

    async fn delete_older_than(
        &self,
        read_cutoff_secs: u64,
        unread_cutoff_secs: u64,
    ) -> Result<u64> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            let read_cutoff = read_cutoff_secs as i64;
            let unread_cutoff = unread_cutoff_secs as i64;
            // A notification is "read" when the human acted on it (resolved),
            // explicitly dismissed it, or marked it read in the UI
            // (business-continuity.md §5 + user-management.md read state).
            let is_read = notifications::read
                .eq(1)
                .or(notifications::resolved_at.is_not_null())
                .or(notifications::dismissed_at.is_not_null());
            let read_old = is_read.and(notifications::created_at.lt(read_cutoff));
            let unread_old = notifications::read
                .eq(0)
                .and(notifications::resolved_at.is_null())
                .and(notifications::dismissed_at.is_null())
                .and(notifications::created_at.lt(unread_cutoff));
            let n = diesel::delete(notifications::table)
                .filter(read_old.or(unread_old))
                .execute(&mut *conn)
                .context("delete old notifications")?;
            Ok(n as u64)
        })
        .await?
    }
}
