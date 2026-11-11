use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::{Id, Notification, NotificationSource, NotificationType};
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

    async fn update_body_by_entity_ref(
        &self,
        entity_ref: &str,
        notification_type: &str,
        body: &str,
    ) -> Result<u64> {
        let pool = Arc::clone(&self.pool);
        let entity_ref = entity_ref.to_string();
        let ntype = notification_type.to_string();
        let body = body.to_string();
        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            let updated = diesel::update(
                notifications::table
                    .filter(notifications::entity_ref.eq(entity_ref.as_str()))
                    .filter(notifications::notification_type.eq(ntype.as_str()))
                    .filter(notifications::resolved_at.is_null())
                    .filter(notifications::dismissed_at.is_null()),
            )
            .set(notifications::body.eq(body))
            .execute(&mut *conn)
            .context("update notification body by entity_ref")?;
            Ok(updated as u64)
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        // Scope the handle to the tenant the fixtures are created under —
        // reads are tenant-filtered (hierarchy-enforcement §3), so a
        // "default"-scoped handle would not see "tenant-1" rows.
        let s = SqliteStorage::new_for_tenant(tmp.path().to_str().unwrap(), "tenant-1").unwrap();
        (tmp, s)
    }

    fn make(id: &str, created_at: i64, read: bool) -> Notification {
        let mut n = Notification::new(
            Id::new(id),
            Id::new("ws-1"),
            Id::new("user-1"),
            NotificationType::GateFailure,
            format!("n {id}"),
            "tenant-1",
            created_at,
        );
        if read {
            n.resolved_at = Some(created_at + 10);
        }
        n
    }

    #[tokio::test]
    async fn delete_older_than_read_unread_split() {
        let (_tmp, s) = setup();
        let now = 1_800_000_000u64;
        let read_cutoff = now - 90 * 86_400;
        let unread_cutoff = now - 365 * 86_400;

        // old + read → deleted (past the 90d read cutoff).
        NotificationRepository::create(&s, &make("old-read", (read_cutoff - 1) as i64, true))
            .await
            .unwrap();
        // old + unread → only deleted past the 365d unread cutoff; this one
        // is past 90d but within 365d → kept.
        NotificationRepository::create(&s, &make("mid-unread", (read_cutoff - 1) as i64, false))
            .await
            .unwrap();
        // ancient + unread → deleted (past the 365d unread cutoff).
        NotificationRepository::create(
            &s,
            &make("ancient-unread", (unread_cutoff - 1) as i64, false),
        )
        .await
        .unwrap();
        // recent + read → kept (within the 90d read cutoff).
        NotificationRepository::create(&s, &make("new-read", (read_cutoff + 1) as i64, true))
            .await
            .unwrap();
        // dismissed counts as read too.
        let mut dismissed = make("old-dismissed", (read_cutoff - 1) as i64, false);
        dismissed.dismissed_at = Some(read_cutoff as i64 - 5);
        NotificationRepository::create(&s, &dismissed)
            .await
            .unwrap();
        // old + marked-read via the read flag (not resolved) → deleted as read.
        let mut marked = make("old-marked-read", (read_cutoff - 1) as i64, false);
        marked.read = true;
        marked.read_at = Some(read_cutoff as i64 - 5);
        NotificationRepository::create(&s, &marked).await.unwrap();

        let deleted = NotificationRepository::delete_older_than(&s, read_cutoff, unread_cutoff)
            .await
            .unwrap();
        assert_eq!(
            deleted, 4,
            "old-read, ancient-unread, old-dismissed, old-marked-read"
        );

        let remaining = NotificationRepository::list_recent(&s, 100).await.unwrap();
        let ids: Vec<&str> = remaining.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"mid-unread"));
        assert!(ids.contains(&"new-read"));
    }

    #[tokio::test]
    async fn update_body_by_entity_ref_updates_only_unresolved_matches() {
        // task-175 review F1: save_spec's append path refreshes the
        // SpecApprovalRequested card bodies in place. The update must touch
        // every unresolved card referencing the entity (the fan-out creates
        // one per recipient) and skip resolved/dismissed cards — their body
        // is the audit record of what was actually acted on — and skip other
        // entities/types entirely.
        let (_tmp, s) = setup();
        let mut base = make("card-1", 100, false);
        base.notification_type = NotificationType::SpecApprovalRequested;
        base.source.entity_type = "mr".to_string();
        base.source.entity_id = "mr-1".to_string();
        base.body = Some("{\"spec_sha\":\"old\"}".to_string());
        let mut card2 = base.clone();
        card2.id = Id::new("card-2"); // second recipient of the same MR
        let mut resolved = base.clone();
        resolved.id = Id::new("card-resolved");
        resolved.resolved_at = Some(110);
        let mut dismissed = base.clone();
        dismissed.id = Id::new("card-dismissed");
        dismissed.dismissed_at = Some(110);
        let mut other_mr = base.clone();
        other_mr.id = Id::new("card-other-mr");
        other_mr.source.entity_id = "mr-2".to_string();
        let mut other_type = base.clone();
        other_type.id = Id::new("card-other-type");
        other_type.notification_type = NotificationType::GateFailure;
        for n in [&base, &card2, &resolved, &dismissed, &other_mr, &other_type] {
            NotificationRepository::create(&s, n).await.unwrap();
        }

        let updated = NotificationRepository::update_body_by_entity_ref(
            &s,
            "mr-1",
            "SpecApprovalRequested",
            "{\"spec_sha\":\"new\"}",
        )
        .await
        .unwrap();
        assert_eq!(updated, 2, "card-1 + card-2 only");

        let all = NotificationRepository::list_recent(&s, 100).await.unwrap();
        let by_id = |id: &str| all.iter().find(|n| n.id.as_str() == id).unwrap();
        assert_eq!(
            by_id("card-1").body.as_deref(),
            Some("{\"spec_sha\":\"new\"}")
        );
        assert_eq!(
            by_id("card-2").body.as_deref(),
            Some("{\"spec_sha\":\"new\"}")
        );
        // Resolved/dismissed/other-entity/other-type bodies untouched.
        assert_eq!(
            by_id("card-resolved").body.as_deref(),
            Some("{\"spec_sha\":\"old\"}")
        );
        assert_eq!(
            by_id("card-dismissed").body.as_deref(),
            Some("{\"spec_sha\":\"old\"}")
        );
        assert_eq!(
            by_id("card-other-mr").body.as_deref(),
            Some("{\"spec_sha\":\"old\"}")
        );
        assert_eq!(
            by_id("card-other-type").body.as_deref(),
            Some("{\"spec_sha\":\"old\"}")
        );

        // Nonexistent entity updates nothing, succeeds.
        let updated = NotificationRepository::update_body_by_entity_ref(
            &s,
            "mr-nope",
            "SpecApprovalRequested",
            "x",
        )
        .await
        .unwrap();
        assert_eq!(updated, 0);
    }

    #[tokio::test]
    async fn create_persists_source_action_url_and_read_state() {
        let (_tmp, s) = setup();
        let mut n = Notification::new(
            Id::new("n-src"),
            Id::new("ws-1"),
            Id::new("user-1"),
            NotificationType::MrMerged,
            "MR merged",
            "tenant-1",
            1000,
        );
        n.source.entity_type = "mr".to_string();
        n.source.entity_id = "mr-9".to_string();
        n.source.repo_id = Some(Id::new("repo-1"));
        n.action_url = Some("/workspaces/ws/r/repo/mrs/mr-9".to_string());
        NotificationRepository::create(&s, &n).await.unwrap();

        let loaded = NotificationRepository::get(&s, &Id::new("n-src"), &Id::new("user-1"))
            .await
            .unwrap()
            .expect("notification must load");
        assert_eq!(loaded.source.entity_type, "mr");
        assert_eq!(loaded.source.entity_id, "mr-9");
        assert_eq!(
            loaded.source.repo_id.as_ref().map(|r| r.as_str()),
            Some("repo-1")
        );
        assert_eq!(
            loaded.action_url.as_deref(),
            Some("/workspaces/ws/r/repo/mrs/mr-9")
        );
        assert!(!loaded.read);
        assert!(loaded.read_at.is_none());
        assert_eq!(loaded.priority_rank, 9);
        assert_eq!(
            loaded.priority,
            gyre_common::NotificationPriority::Low,
            "MrMerged is informational per user-management.md"
        );
    }

    #[tokio::test]
    async fn mark_read_stamps_and_clears() {
        let (_tmp, s) = setup();
        NotificationRepository::create(&s, &make("n-read", 1000, false))
            .await
            .unwrap();

        NotificationRepository::mark_read(&s, &Id::new("n-read"), &Id::new("user-1"), true)
            .await
            .unwrap();
        let n = NotificationRepository::get(&s, &Id::new("n-read"), &Id::new("user-1"))
            .await
            .unwrap()
            .unwrap();
        assert!(n.read);
        assert!(n.read_at.is_some(), "read_at must be stamped");

        // Marking unread clears both — no stale read timestamp.
        NotificationRepository::mark_read(&s, &Id::new("n-read"), &Id::new("user-1"), false)
            .await
            .unwrap();
        let n = NotificationRepository::get(&s, &Id::new("n-read"), &Id::new("user-1"))
            .await
            .unwrap()
            .unwrap();
        assert!(!n.read);
        assert!(n.read_at.is_none());

        // Scoped to the owning user: another user's mark_read is a no-op.
        NotificationRepository::mark_read(&s, &Id::new("n-read"), &Id::new("user-2"), true)
            .await
            .unwrap();
        let n = NotificationRepository::get(&s, &Id::new("n-read"), &Id::new("user-1"))
            .await
            .unwrap()
            .unwrap();
        assert!(!n.read, "cross-user mark_read must not mutate");
    }

    #[tokio::test]
    async fn has_active_notification_matches_type_entity_and_since() {
        let (_tmp, s) = setup();
        let mut n = make("n-dedup", 1000, false);
        n.notification_type = NotificationType::AgentBudgetWarning;
        n.source.entity_id = "agent-1".to_string();
        NotificationRepository::create(&s, &n).await.unwrap();

        let ws = Id::new("ws-1");
        let user = Id::new("user-1");
        assert!(NotificationRepository::has_active_notification(
            &s,
            &ws,
            &user,
            "AgentBudgetWarning",
            "agent-1",
            500
        )
        .await
        .unwrap());
        // Wrong entity, wrong type, or before `since` → no match.
        assert!(!NotificationRepository::has_active_notification(
            &s,
            &ws,
            &user,
            "AgentBudgetWarning",
            "agent-2",
            500
        )
        .await
        .unwrap());
        assert!(!NotificationRepository::has_active_notification(
            &s,
            &ws,
            &user,
            "GatePassed",
            "agent-1",
            500
        )
        .await
        .unwrap());
        assert!(!NotificationRepository::has_active_notification(
            &s,
            &ws,
            &user,
            "AgentBudgetWarning",
            "agent-1",
            1001
        )
        .await
        .unwrap());

        // Resolving the notification removes it from the active set.
        NotificationRepository::resolve(&s, &Id::new("n-dedup"), &user, None)
            .await
            .unwrap();
        assert!(!NotificationRepository::has_active_notification(
            &s,
            &ws,
            &user,
            "AgentBudgetWarning",
            "agent-1",
            500
        )
        .await
        .unwrap());
    }
}
