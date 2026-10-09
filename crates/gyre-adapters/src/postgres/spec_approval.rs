use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::spec_approval::ApprovalTransitionError;
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
    approved_at: Option<i64>,
    revoked_at: Option<i64>,
    revoked_by: Option<String>,
    revocation_reason: Option<String>,
    rejected_at: Option<i64>,
    rejected_reason: Option<String>,
    rejected_by: Option<String>,
}

impl SpecApprovalRow {
    fn into_approval(self) -> SpecApproval {
        SpecApproval {
            id: Id::new(self.id),
            spec_path: self.spec_path,
            spec_sha: self.spec_sha,
            approver_id: self.approver_id,
            signature: self.signature,
            approved_at: self.approved_at.map(|v| v as u64),
            revoked_at: self.revoked_at.map(|v| v as u64),
            revoked_by: self.revoked_by,
            revocation_reason: self.revocation_reason,
            rejected_at: self.rejected_at.map(|v| v as u64),
            rejected_reason: self.rejected_reason,
            rejected_by: self.rejected_by,
        }
    }
}
#[derive(Insertable, AsChangeset)]
#[diesel(table_name = spec_approvals)]
struct NewSpecApprovalRow<'a> {
    id: &'a str,
    spec_path: &'a str,
    spec_sha: &'a str,
    approver_id: &'a str,
    signature: Option<&'a str>,
    approved_at: Option<i64>,
    revoked_at: Option<i64>,
    revoked_by: Option<&'a str>,
    revocation_reason: Option<&'a str>,
    rejected_at: Option<i64>,
    rejected_reason: Option<&'a str>,
    rejected_by: Option<&'a str>,
}

impl PgStorage {
    /// Apply a domain transition (approve/revoke/reject) to a stored entry:
    /// load, validate via the domain's transition rules, persist the full row
    /// so mutual exclusivity (clearing other timestamp columns) is guaranteed.
    /// Returns Ok(None) when the entry does not exist.
    async fn transition(
        &self,
        id: &Id,
        apply: impl FnOnce(&mut SpecApproval) -> Result<(), ApprovalTransitionError> + Send + 'static,
    ) -> Result<Option<()>, ApprovalTransitionError> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<()>, ApprovalTransitionError> {
            let mut conn = pool
                .get()
                .map_err(|e| ApprovalTransitionError::Storage(e.to_string()))?;
            let row = spec_approvals::table
                .find(id.as_str())
                .first::<SpecApprovalRow>(&mut conn)
                .optional()
                .map_err(|e| ApprovalTransitionError::Storage(e.to_string()))?;
            let Some(row) = row else {
                return Ok(None);
            };
            let mut approval = row.into_approval();
            apply(&mut approval)?;
            upsert_row(&mut conn, &approval)
                .map_err(|e| ApprovalTransitionError::Storage(e.to_string()))?;
            Ok(Some(()))
        })
        .await
        .map_err(|e| ApprovalTransitionError::Storage(e.to_string()))?
    }
}
/// Persist the full row for an approval (used after in-domain transitions).
fn upsert_row(conn: &mut diesel::PgConnection, a: &SpecApproval) -> Result<()> {
    let row = NewSpecApprovalRow {
        id: a.id.as_str(),
        spec_path: &a.spec_path,
        spec_sha: &a.spec_sha,
        approver_id: &a.approver_id,
        signature: a.signature.as_deref(),
        approved_at: a.approved_at.map(|v| v as i64),
        revoked_at: a.revoked_at.map(|v| v as i64),
        revoked_by: a.revoked_by.as_deref(),
        revocation_reason: a.revocation_reason.as_deref(),
        rejected_at: a.rejected_at.map(|v| v as i64),
        rejected_reason: a.rejected_reason.as_deref(),
        rejected_by: a.rejected_by.as_deref(),
    };
    diesel::insert_into(spec_approvals::table)
        .values(&row)
        .on_conflict(spec_approvals::id)
        .do_update()
        .set(&row)
        .execute(conn)
        .context("upsert spec approval")?;
    Ok(())
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
                approved_at: a.approved_at.map(|v| v as i64),
                revoked_at: a.revoked_at.map(|v| v as i64),
                revoked_by: a.revoked_by.as_deref(),
                revocation_reason: a.revocation_reason.as_deref(),
                rejected_at: a.rejected_at.map(|v| v as i64),
                rejected_reason: a.rejected_reason.as_deref(),
                rejected_by: a.rejected_by.as_deref(),
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
                .order(spec_approvals::id.desc())
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
                .filter(spec_approvals::rejected_at.is_null())
                .filter(spec_approvals::approved_at.is_not_null())
                .order(spec_approvals::id.desc())
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
                .order(spec_approvals::id.desc())
                .load::<SpecApprovalRow>(&mut *conn)
                .context("list all spec approvals")?;
            Ok(rows
                .into_iter()
                .map(SpecApprovalRow::into_approval)
                .collect())
        })
        .await?
    }

    async fn approve(&self, id: &Id, now: u64) -> Result<Option<()>, ApprovalTransitionError> {
        self.transition(id, move |a| a.approve(now)).await
    }
    async fn revoke(
        &self,
        id: &Id,
        revoked_by: &str,
        reason: &str,
        now: u64,
    ) -> Result<Option<()>, ApprovalTransitionError> {
        let revoked_by = revoked_by.to_string();
        let reason = reason.to_string();
        self.transition(id, move |a| a.revoke(&revoked_by, &reason, now))
            .await
    }
    async fn reject(
        &self,
        id: &Id,
        rejected_by: &str,
        reason: &str,
        now: u64,
    ) -> Result<Option<()>, ApprovalTransitionError> {
        let rejected_by = rejected_by.to_string();
        let reason = reason.to_string();
        self.transition(id, move |a| a.reject(&rejected_by, &reason, now))
            .await
    }

    async fn revoke_all_for_path(
        &self,
        spec_path: &str,
        revoked_by: &str,
        reason: &str,
        now: u64,
    ) -> Result<u64> {
        let pool = Arc::clone(&self.pool);
        let path = spec_path.to_string();
        let revoked_by = revoked_by.to_string();
        let reason = reason.to_string();
        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            // Apply the same domain transition per row: only Approved rows
            // are revocable; mutual exclusivity clears approved_at/rejected_*.
            let rows = spec_approvals::table
                .filter(spec_approvals::spec_path.eq(&path))
                .load::<SpecApprovalRow>(&mut *conn)
                .context("load spec approvals for revoke-all")?;
            let mut revoked = 0u64;
            for row in rows {
                let mut approval = row.into_approval();
                // Stale-invalidation semantics: an approval is stale once the
                // spec content changes. Rows already revoked/rejected keep
                // their terminal state; only Approved rows transition.
                if approval.status() == gyre_domain::spec_approval::ApprovalStatus::Approved
                    && approval.revoke(&revoked_by, &reason, now).is_ok()
                {
                    upsert_row(&mut conn, &approval)?;
                    revoked += 1;
                }
            }
            Ok(revoked)
        })
        .await?
    }
}
