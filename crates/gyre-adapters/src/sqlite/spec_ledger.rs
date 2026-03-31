use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_domain::{ApprovalStatus, SpecLedgerEntry};
use gyre_ports::SpecLedgerRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::spec_ledger_entries;

#[derive(Queryable, Selectable)]
#[diesel(table_name = spec_ledger_entries)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct SpecLedgerRow {
    path: String,
    title: String,
    owner: String,
    kind: Option<String>,
    current_sha: String,
    approval_mode: String,
    approval_status: String,
    linked_tasks: String,
    linked_mrs: String,
    drift_status: String,
    created_at: i64,
    updated_at: i64,
    repo_id: Option<String>,
    workspace_id: Option<String>,
}

impl SpecLedgerRow {
    fn into_entry(self) -> SpecLedgerEntry {
        let approval_status = match self.approval_status.as_str() {
            "approved" => ApprovalStatus::Approved,
            "deprecated" => ApprovalStatus::Deprecated,
            "revoked" => ApprovalStatus::Revoked,
            "rejected" => ApprovalStatus::Rejected,
            _ => ApprovalStatus::Pending,
        };
        SpecLedgerEntry {
            path: self.path,
            title: self.title,
            owner: self.owner,
            kind: self.kind,
            current_sha: self.current_sha,
            approval_mode: self.approval_mode,
            approval_status,
            linked_tasks: serde_json::from_str(&self.linked_tasks).unwrap_or_default(),
            linked_mrs: serde_json::from_str(&self.linked_mrs).unwrap_or_default(),
            drift_status: self.drift_status,
            created_at: self.created_at as u64,
            updated_at: self.updated_at as u64,
            repo_id: self.repo_id,
            workspace_id: self.workspace_id,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = spec_ledger_entries)]
struct NewSpecLedgerRow<'a> {
    path: &'a str,
    title: &'a str,
    owner: &'a str,
    kind: Option<&'a str>,
    current_sha: &'a str,
    approval_mode: &'a str,
    approval_status: &'a str,
    linked_tasks: &'a str,
    linked_mrs: &'a str,
    drift_status: &'a str,
    created_at: i64,
    updated_at: i64,
    repo_id: Option<&'a str>,
    workspace_id: Option<&'a str>,
}

fn approval_status_str(s: &ApprovalStatus) -> &'static str {
    match s {
        ApprovalStatus::Pending => "pending",
        ApprovalStatus::Approved => "approved",
        ApprovalStatus::Deprecated => "deprecated",
        ApprovalStatus::Revoked => "revoked",
        ApprovalStatus::Rejected => "rejected",
    }
}

#[async_trait]
impl SpecLedgerRepository for SqliteStorage {
    async fn find_by_path(&self, path: &str) -> Result<Option<SpecLedgerEntry>> {
        let pool = Arc::clone(&self.pool);
        let path = path.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<SpecLedgerEntry>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = spec_ledger_entries::table
                .find(&path)
                .first::<SpecLedgerRow>(&mut *conn)
                .optional()
                .context("find spec ledger entry by path")?;
            Ok(row.map(SpecLedgerRow::into_entry))
        })
        .await?
    }

    async fn list_all(&self) -> Result<Vec<SpecLedgerEntry>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<SpecLedgerEntry>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = spec_ledger_entries::table
                .order(spec_ledger_entries::path.asc())
                .load::<SpecLedgerRow>(&mut *conn)
                .context("list all spec ledger entries")?;
            Ok(rows.into_iter().map(SpecLedgerRow::into_entry).collect())
        })
        .await?
    }

    async fn save(&self, entry: &SpecLedgerEntry) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let e = entry.clone();
        let status_str = approval_status_str(&e.approval_status).to_string();
        let linked_tasks = serde_json::to_string(&e.linked_tasks).unwrap_or_else(|_| "[]".into());
        let linked_mrs = serde_json::to_string(&e.linked_mrs).unwrap_or_else(|_| "[]".into());
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewSpecLedgerRow {
                path: &e.path,
                title: &e.title,
                owner: &e.owner,
                kind: e.kind.as_deref(),
                current_sha: &e.current_sha,
                approval_mode: &e.approval_mode,
                approval_status: &status_str,
                linked_tasks: &linked_tasks,
                linked_mrs: &linked_mrs,
                drift_status: &e.drift_status,
                created_at: e.created_at as i64,
                updated_at: e.updated_at as i64,
                repo_id: e.repo_id.as_deref(),
                workspace_id: e.workspace_id.as_deref(),
            };
            diesel::insert_into(spec_ledger_entries::table)
                .values(&row)
                .on_conflict(spec_ledger_entries::path)
                .do_update()
                .set((
                    spec_ledger_entries::title.eq(row.title),
                    spec_ledger_entries::owner.eq(row.owner),
                    spec_ledger_entries::kind.eq(row.kind),
                    spec_ledger_entries::current_sha.eq(row.current_sha),
                    spec_ledger_entries::approval_mode.eq(row.approval_mode),
                    spec_ledger_entries::approval_status.eq(row.approval_status),
                    spec_ledger_entries::linked_tasks.eq(row.linked_tasks),
                    spec_ledger_entries::linked_mrs.eq(row.linked_mrs),
                    spec_ledger_entries::drift_status.eq(row.drift_status),
                    spec_ledger_entries::updated_at.eq(row.updated_at),
                    spec_ledger_entries::repo_id.eq(row.repo_id),
                    spec_ledger_entries::workspace_id.eq(row.workspace_id),
                ))
                .execute(&mut *conn)
                .context("upsert spec ledger entry")?;
            Ok(())
        })
        .await?
    }

    async fn delete_by_path(&self, path: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let path = path.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(spec_ledger_entries::table.find(&path))
                .execute(&mut *conn)
                .context("delete spec ledger entry")?;
            Ok(())
        })
        .await?
    }
}
