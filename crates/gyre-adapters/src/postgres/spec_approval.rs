use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::SpecApproval;
use gyre_ports::SpecApprovalRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::spec_approvals;

#[derive(Queryable, Selectable)]
#[diesel(table_name = spec_approvals)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct SpecApprovalRow {
    id: String,
    spec_path: String,
    spec_sha: String,
    approver_id: String,
    signature: Option<String>,
    approved_at: i64,
    revoked_at: Option<i64>,
    revoked_by: Option<String>,
    revocation_reason: Option<String>,
}

impl SpecApprovalRow {
    fn into_approval(self) -> SpecApproval {
        SpecApproval {
            id: Id::new(self.id),
            spec_path: self.spec_path,
            spec_sha: self.spec_sha,
            approver_id: self.approver_id,
            signature: self.signature,
            approved_at: self.approved_at as u64,
            revoked_at: self.revoked_at.map(|v| v as u64),
            revoked_by: self.revoked_by,
            revocation_reason: self.revocation_reason,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = spec_approvals)]
struct NewSpecApprovalRow<'a> {
    id: &'a str,
    spec_path: &'a str,
    spec_sha: &'a str,
    approver_id: &'a str,
    signature: Option<&'a str>,
    approved_at: i64,
    revoked_at: Option<i64>,
    revoked_by: Option<&'a str>,
    revocation_reason: Option<&'a str>,
}

#[async_trait]
impl SpecApprovalRepository for PgStorage {
    async fn create(&self, approval: &SpecApproval) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let a = approval.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewSpecApprovalRow {
                id: a.id.as_str(),
                spec_path: &a.spec_path,
                spec_sha: &a.spec_sha,
                approver_id: &a.approver_id,
                signature: a.signature.as_deref(),
                approved_at: a.approved_at as i64,
                revoked_at: a.revoked_at.map(|v| v as i64),
                revoked_by: a.revoked_by.as_deref(),
                revocation_reason: a.revocation_reason.as_deref(),
            };
            diesel::insert_into(spec_approvals::table)
                .values(&row)
                .on_conflict(spec_approvals::id)
                .do_nothing()
                .execute(&mut *conn)
                .context("insert spec approval")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<SpecApproval>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<SpecApproval>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = spec_approvals::table
                .find(id.as_str())
                .first::<SpecApprovalRow>(&mut *conn)
                .optional()
                .context("find spec approval by id")?;
            Ok(result.map(SpecApprovalRow::into_approval))
        })
        .await?
    }

    async fn list_by_path(&self, spec_path: &str) -> Result<Vec<SpecApproval>> {
        let pool = Arc::clone(&self.pool);
        let path = spec_path.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<SpecApproval>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = spec_approvals::table
                .filter(spec_approvals::spec_path.eq(&path))
                .order(spec_approvals::approved_at.desc())
                .load::<SpecApprovalRow>(&mut *conn)
                .context("list spec approvals by path")?;
            Ok(rows
                .into_iter()
                .map(SpecApprovalRow::into_approval)
                .collect())
        })
        .await?
    }

    async fn list_active_by_path(&self, spec_path: &str) -> Result<Vec<SpecApproval>> {
        let pool = Arc::clone(&self.pool);
        let path = spec_path.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<SpecApproval>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = spec_approvals::table
                .filter(spec_approvals::spec_path.eq(&path))
                .filter(spec_approvals::revoked_at.is_null())
                .order(spec_approvals::approved_at.desc())
                .load::<SpecApprovalRow>(&mut *conn)
                .context("list active spec approvals by path")?;
            Ok(rows
                .into_iter()
                .map(SpecApprovalRow::into_approval)
                .collect())
        })
        .await?
    }

    async fn list_all(&self) -> Result<Vec<SpecApproval>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<SpecApproval>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = spec_approvals::table
                .order(spec_approvals::approved_at.desc())
                .load::<SpecApprovalRow>(&mut *conn)
                .context("list all spec approvals")?;
            Ok(rows
                .into_iter()
                .map(SpecApprovalRow::into_approval)
                .collect())
        })
        .await?
    }

    async fn revoke(&self, id: &Id, revoked_by: &str, reason: &str, now: u64) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        let revoked_by = revoked_by.to_string();
        let reason = reason.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(spec_approvals::table.find(id.as_str()))
                .set((
                    spec_approvals::revoked_at.eq(now as i64),
                    spec_approvals::revoked_by.eq(&revoked_by),
                    spec_approvals::revocation_reason.eq(&reason),
                ))
                .execute(&mut *conn)
                .context("revoke spec approval")?;
            Ok(())
        })
        .await?
    }

    async fn revoke_all_for_path(
        &self,
        spec_path: &str,
        revoked_by: &str,
        reason: &str,
        now: u64,
    ) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let path = spec_path.to_string();
        let revoked_by = revoked_by.to_string();
        let reason = reason.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(
                spec_approvals::table
                    .filter(spec_approvals::spec_path.eq(&path))
                    .filter(spec_approvals::revoked_at.is_null()),
            )
            .set((
                spec_approvals::revoked_at.eq(now as i64),
                spec_approvals::revoked_by.eq(&revoked_by),
                spec_approvals::revocation_reason.eq(&reason),
            ))
            .execute(&mut *conn)
            .context("revoke all spec approvals for path")?;
            Ok(())
        })
        .await?
    }
}
