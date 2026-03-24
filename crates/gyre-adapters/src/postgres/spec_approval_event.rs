use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_domain::SpecApprovalEvent;
use gyre_ports::SpecApprovalEventRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::spec_approval_events;

#[derive(Queryable, Selectable)]
#[diesel(table_name = spec_approval_events)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct SpecApprovalEventRow {
    id: String,
    spec_path: String,
    spec_sha: String,
    approver_type: String,
    approver_id: String,
    persona: Option<String>,
    approved_at: i64,
    revoked_at: Option<i64>,
    revoked_by: Option<String>,
    revocation_reason: Option<String>,
}

impl SpecApprovalEventRow {
    fn into_event(self) -> SpecApprovalEvent {
        SpecApprovalEvent {
            id: self.id,
            spec_path: self.spec_path,
            spec_sha: self.spec_sha,
            approver_type: self.approver_type,
            approver_id: self.approver_id,
            persona: self.persona,
            approved_at: self.approved_at as u64,
            revoked_at: self.revoked_at.map(|v| v as u64),
            revoked_by: self.revoked_by,
            revocation_reason: self.revocation_reason,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = spec_approval_events)]
struct NewSpecApprovalEventRow<'a> {
    id: &'a str,
    spec_path: &'a str,
    spec_sha: &'a str,
    approver_type: &'a str,
    approver_id: &'a str,
    persona: Option<&'a str>,
    approved_at: i64,
    revoked_at: Option<i64>,
    revoked_by: Option<&'a str>,
    revocation_reason: Option<&'a str>,
}

#[async_trait]
impl SpecApprovalEventRepository for PgStorage {
    async fn record(&self, event: &SpecApprovalEvent) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let e = event.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewSpecApprovalEventRow {
                id: &e.id,
                spec_path: &e.spec_path,
                spec_sha: &e.spec_sha,
                approver_type: &e.approver_type,
                approver_id: &e.approver_id,
                persona: e.persona.as_deref(),
                approved_at: e.approved_at as i64,
                revoked_at: e.revoked_at.map(|v| v as i64),
                revoked_by: e.revoked_by.as_deref(),
                revocation_reason: e.revocation_reason.as_deref(),
            };
            diesel::insert_into(spec_approval_events::table)
                .values(&row)
                .on_conflict(spec_approval_events::id)
                .do_nothing()
                .execute(&mut *conn)
                .context("insert spec approval event")?;
            Ok(())
        })
        .await?
    }

    async fn list_by_path(&self, spec_path: &str) -> Result<Vec<SpecApprovalEvent>> {
        let pool = Arc::clone(&self.pool);
        let path = spec_path.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<SpecApprovalEvent>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = spec_approval_events::table
                .filter(spec_approval_events::spec_path.eq(&path))
                .order(spec_approval_events::approved_at.asc())
                .load::<SpecApprovalEventRow>(&mut *conn)
                .context("list spec approval events by path")?;
            Ok(rows
                .into_iter()
                .map(SpecApprovalEventRow::into_event)
                .collect())
        })
        .await?
    }

    async fn list_all(&self) -> Result<Vec<SpecApprovalEvent>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<SpecApprovalEvent>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = spec_approval_events::table
                .order(spec_approval_events::approved_at.asc())
                .load::<SpecApprovalEventRow>(&mut *conn)
                .context("list all spec approval events")?;
            Ok(rows
                .into_iter()
                .map(SpecApprovalEventRow::into_event)
                .collect())
        })
        .await?
    }

    async fn revoke_event(
        &self,
        id: &str,
        revoked_at: u64,
        revoked_by: &str,
        reason: &str,
    ) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.to_string();
        let revoked_by = revoked_by.to_string();
        let reason = reason.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(spec_approval_events::table.find(&id))
                .set((
                    spec_approval_events::revoked_at.eq(revoked_at as i64),
                    spec_approval_events::revoked_by.eq(&revoked_by),
                    spec_approval_events::revocation_reason.eq(&reason),
                ))
                .execute(&mut *conn)
                .context("revoke spec approval event")?;
            Ok(())
        })
        .await?
    }
}
