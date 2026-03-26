use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{Notification, NotificationPriority, NotificationType};
use gyre_ports::NotificationRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::notifications;

fn type_to_str(t: &NotificationType) -> &'static str {
    match t {
        NotificationType::SpecApprovalRequested => "SpecApprovalRequested",
        NotificationType::PersonaApprovalRequested => "PersonaApprovalRequested",
        NotificationType::AgentEscalation => "AgentEscalation",
        NotificationType::AgentBudgetWarning => "AgentBudgetWarning",
        NotificationType::AgentBudgetExhausted => "AgentBudgetExhausted",
        NotificationType::AgentFailed => "AgentFailed",
        NotificationType::GateFailure => "GateFailure",
        NotificationType::GatePassed => "GatePassed",
        NotificationType::MrMerged => "MrMerged",
        NotificationType::MrNeedsReview => "MrNeedsReview",
        NotificationType::MrReverted => "MrReverted",
        NotificationType::BreakingChangeDetected => "BreakingChangeDetected",
        NotificationType::SpecDriftDetected => "SpecDriftDetected",
        NotificationType::InvitationReceived => "InvitationReceived",
        NotificationType::MembershipChanged => "MembershipChanged",
        NotificationType::SystemAlert => "SystemAlert",
        NotificationType::CrossWorkspaceSpecChanged => "CrossWorkspaceSpecChanged",
    }
}

fn str_to_type(s: &str) -> Result<NotificationType> {
    match s {
        "SpecApprovalRequested" => Ok(NotificationType::SpecApprovalRequested),
        "PersonaApprovalRequested" => Ok(NotificationType::PersonaApprovalRequested),
        "AgentEscalation" => Ok(NotificationType::AgentEscalation),
        "AgentBudgetWarning" => Ok(NotificationType::AgentBudgetWarning),
        "AgentBudgetExhausted" => Ok(NotificationType::AgentBudgetExhausted),
        "AgentFailed" => Ok(NotificationType::AgentFailed),
        "GateFailure" => Ok(NotificationType::GateFailure),
        "GatePassed" => Ok(NotificationType::GatePassed),
        "MrMerged" => Ok(NotificationType::MrMerged),
        "MrNeedsReview" => Ok(NotificationType::MrNeedsReview),
        "MrReverted" => Ok(NotificationType::MrReverted),
        "BreakingChangeDetected" => Ok(NotificationType::BreakingChangeDetected),
        "SpecDriftDetected" => Ok(NotificationType::SpecDriftDetected),
        "InvitationReceived" => Ok(NotificationType::InvitationReceived),
        "MembershipChanged" => Ok(NotificationType::MembershipChanged),
        "SystemAlert" => Ok(NotificationType::SystemAlert),
        "CrossWorkspaceSpecChanged" => Ok(NotificationType::CrossWorkspaceSpecChanged),
        other => Err(anyhow!("unknown notification type: {}", other)),
    }
}

fn priority_to_str(p: &NotificationPriority) -> &'static str {
    match p {
        NotificationPriority::Low => "Low",
        NotificationPriority::Medium => "Medium",
        NotificationPriority::High => "High",
        NotificationPriority::Urgent => "Urgent",
    }
}

fn str_to_priority(s: &str) -> Result<NotificationPriority> {
    match s {
        "Low" => Ok(NotificationPriority::Low),
        "Medium" => Ok(NotificationPriority::Medium),
        "High" => Ok(NotificationPriority::High),
        "Urgent" => Ok(NotificationPriority::Urgent),
        other => Err(anyhow!("unknown notification priority: {}", other)),
    }
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = notifications)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct NotificationRow {
    id: String,
    user_id: String,
    notification_type: String,
    title: String,
    body: String,
    entity_type: Option<String>,
    entity_id: Option<String>,
    priority: String,
    action_url: Option<String>,
    read: i32,
    read_at: Option<i64>,
    created_at: i64,
}

impl NotificationRow {
    fn into_notification(self) -> Result<Notification> {
        Ok(Notification {
            id: Id::new(self.id),
            user_id: Id::new(self.user_id),
            notification_type: str_to_type(&self.notification_type)?,
            title: self.title,
            body: self.body,
            entity_type: self.entity_type,
            entity_id: self.entity_id,
            priority: str_to_priority(&self.priority)?,
            action_url: self.action_url,
            read: self.read != 0,
            read_at: self.read_at.map(|v| v as u64),
            created_at: self.created_at as u64,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = notifications)]
struct NewNotificationRow<'a> {
    id: &'a str,
    user_id: &'a str,
    notification_type: &'a str,
    title: &'a str,
    body: &'a str,
    entity_type: Option<&'a str>,
    entity_id: Option<&'a str>,
    priority: &'a str,
    action_url: Option<&'a str>,
    read: i32,
    read_at: Option<i64>,
    created_at: i64,
}

#[async_trait]
impl NotificationRepository for PgStorage {
    async fn create(&self, notification: &Notification) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let n = notification.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewNotificationRow {
                id: n.id.as_str(),
                user_id: n.user_id.as_str(),
                notification_type: type_to_str(&n.notification_type),
                title: &n.title,
                body: &n.body,
                entity_type: n.entity_type.as_deref(),
                entity_id: n.entity_id.as_deref(),
                priority: priority_to_str(&n.priority),
                action_url: n.action_url.as_deref(),
                read: n.read as i32,
                read_at: n.read_at.map(|v| v as i64),
                created_at: n.created_at as i64,
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

    async fn find_by_id(&self, id: &Id) -> Result<Option<Notification>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Notification>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = notifications::table
                .find(id.as_str())
                .first::<NotificationRow>(&mut *conn)
                .optional()
                .context("find notification by id")?;
            result.map(NotificationRow::into_notification).transpose()
        })
        .await?
    }

    async fn list_by_user(&self, user_id: &Id, unread_only: bool) -> Result<Vec<Notification>> {
        let pool = Arc::clone(&self.pool);
        let uid = user_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Notification>> {
            let mut conn = pool.get().context("get db connection")?;
            let mut query = notifications::table
                .filter(notifications::user_id.eq(uid.as_str()))
                .order(notifications::created_at.desc())
                .into_boxed();
            if unread_only {
                query = query.filter(notifications::read.eq(0_i32));
            }
            let rows = query
                .load::<NotificationRow>(&mut *conn)
                .context("list notifications by user")?;
            rows.into_iter()
                .map(NotificationRow::into_notification)
                .collect()
        })
        .await?
    }

    async fn count_unread(&self, user_id: &Id) -> Result<u64> {
        let pool = Arc::clone(&self.pool);
        let uid = user_id.clone();
        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            let count = notifications::table
                .filter(notifications::user_id.eq(uid.as_str()))
                .filter(notifications::read.eq(0_i32))
                .count()
                .get_result::<i64>(&mut *conn)
                .context("count unread notifications")?;
            Ok(count as u64)
        })
        .await?
    }

    async fn mark_read(&self, id: &Id, now: u64) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(notifications::table.find(id.as_str()))
                .set((
                    notifications::read.eq(1_i32),
                    notifications::read_at.eq(now as i64),
                ))
                .execute(&mut *conn)
                .context("mark notification read")?;
            Ok(())
        })
        .await?
    }

    async fn mark_all_read(&self, user_id: &Id, now: u64) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let uid = user_id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(
                notifications::table
                    .filter(notifications::user_id.eq(uid.as_str()))
                    .filter(notifications::read.eq(0_i32)),
            )
            .set((
                notifications::read.eq(1_i32),
                notifications::read_at.eq(now as i64),
            ))
            .execute(&mut *conn)
            .context("mark all notifications read")?;
            Ok(())
        })
        .await?
    }
}
