use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{DiffStats, MergeRequest, MrStatus};
use gyre_ports::MergeRequestRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::merge_requests;

fn status_to_str(s: &MrStatus) -> &'static str {
    match s {
        MrStatus::Open => "Open",
        MrStatus::Approved => "Approved",
        MrStatus::Merged => "Merged",
        MrStatus::Closed => "Closed",
        MrStatus::Reverted => "Reverted",
    }
}

fn str_to_status(s: &str) -> Result<MrStatus> {
    match s {
        "Open" => Ok(MrStatus::Open),
        "Approved" => Ok(MrStatus::Approved),
        "Merged" => Ok(MrStatus::Merged),
        "Closed" => Ok(MrStatus::Closed),
        "Reverted" => Ok(MrStatus::Reverted),
        other => Err(anyhow!("unknown MR status: {}", other)),
    }
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = merge_requests)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct MergeRequestRow {
    id: String,
    repository_id: String,
    title: String,
    source_branch: String,
    target_branch: String,
    status: String,
    author_agent_id: Option<String>,
    reviewers: String,
    created_at: i64,
    updated_at: i64,
    diff_files_changed: Option<i64>,
    diff_insertions: Option<i64>,
    diff_deletions: Option<i64>,
    has_conflicts: Option<i32>,
    #[allow(dead_code)]
    tenant_id: String,
    depends_on: String,
    atomic_group: Option<String>,
    workspace_id: String,
    reverted_at: Option<i64>,
    revert_mr_id: Option<String>,
    spec_ref: Option<String>,
}

impl MergeRequestRow {
    fn into_mr(self) -> Result<MergeRequest> {
        let reviewer_strs: Vec<String> = serde_json::from_str(&self.reviewers).unwrap_or_default();
        let depends_on_strs: Vec<String> =
            serde_json::from_str(&self.depends_on).unwrap_or_default();
        let diff_stats = match (
            self.diff_files_changed,
            self.diff_insertions,
            self.diff_deletions,
        ) {
            (Some(f), Some(i), Some(d)) => Some(DiffStats {
                files_changed: f as usize,
                insertions: i as usize,
                deletions: d as usize,
            }),
            _ => None,
        };
        Ok(MergeRequest {
            id: Id::new(self.id),
            repository_id: Id::new(self.repository_id),
            title: self.title,
            source_branch: self.source_branch,
            target_branch: self.target_branch,
            status: str_to_status(&self.status)?,
            author_agent_id: self.author_agent_id.map(Id::new),
            reviewers: reviewer_strs.into_iter().map(Id::new).collect(),
            diff_stats,
            has_conflicts: self.has_conflicts.map(|v| v != 0),
            spec_ref: self.spec_ref,
            depends_on: depends_on_strs.into_iter().map(Id::new).collect(),
            atomic_group: self.atomic_group,
            created_at: self.created_at as u64,
            updated_at: self.updated_at as u64,
            workspace_id: Id::new(self.workspace_id),
            reverted_at: self.reverted_at.map(|v| v as u64),
            revert_mr_id: self.revert_mr_id.map(Id::new),
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = merge_requests)]
struct NewMergeRequestRow<'a> {
    id: &'a str,
    repository_id: &'a str,
    title: &'a str,
    source_branch: &'a str,
    target_branch: &'a str,
    status: &'a str,
    author_agent_id: Option<&'a str>,
    reviewers: &'a str,
    created_at: i64,
    updated_at: i64,
    diff_files_changed: Option<i64>,
    diff_insertions: Option<i64>,
    diff_deletions: Option<i64>,
    has_conflicts: Option<i32>,
    tenant_id: &'a str,
    depends_on: &'a str,
    atomic_group: Option<&'a str>,
    workspace_id: &'a str,
    reverted_at: Option<i64>,
    revert_mr_id: Option<&'a str>,
    spec_ref: Option<&'a str>,
}

#[async_trait]
impl MergeRequestRepository for PgStorage {
    async fn create(&self, mr: &MergeRequest) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let m = mr.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let reviewer_strs: Vec<&str> = m.reviewers.iter().map(|id| id.as_str()).collect();
            let reviewers_json = serde_json::to_string(&reviewer_strs)?;
            let dep_strs: Vec<&str> = m.depends_on.iter().map(|id| id.as_str()).collect();
            let depends_on_json = serde_json::to_string(&dep_strs)?;
            let row = NewMergeRequestRow {
                id: m.id.as_str(),
                repository_id: m.repository_id.as_str(),
                title: &m.title,
                source_branch: &m.source_branch,
                target_branch: &m.target_branch,
                status: status_to_str(&m.status),
                author_agent_id: m.author_agent_id.as_ref().map(|id| id.as_str()),
                reviewers: &reviewers_json,
                created_at: m.created_at as i64,
                updated_at: m.updated_at as i64,
                diff_files_changed: m.diff_stats.as_ref().map(|d| d.files_changed as i64),
                diff_insertions: m.diff_stats.as_ref().map(|d| d.insertions as i64),
                diff_deletions: m.diff_stats.as_ref().map(|d| d.deletions as i64),
                has_conflicts: m.has_conflicts.map(|v| if v { 1i32 } else { 0 }),
                tenant_id: "default",
                depends_on: &depends_on_json,
                atomic_group: m.atomic_group.as_deref(),
                workspace_id: m.workspace_id.as_str(),
                reverted_at: m.reverted_at.map(|v| v as i64),
                revert_mr_id: m.revert_mr_id.as_ref().map(|id| id.as_str()),
                spec_ref: m.spec_ref.as_deref(),
            };
            diesel::insert_into(merge_requests::table)
                .values(&row)
                .on_conflict(merge_requests::id)
                .do_update()
                .set((
                    merge_requests::title.eq(row.title),
                    merge_requests::source_branch.eq(row.source_branch),
                    merge_requests::target_branch.eq(row.target_branch),
                    merge_requests::status.eq(row.status),
                    merge_requests::author_agent_id.eq(row.author_agent_id),
                    merge_requests::reviewers.eq(row.reviewers),
                    merge_requests::updated_at.eq(row.updated_at),
                    merge_requests::diff_files_changed.eq(row.diff_files_changed),
                    merge_requests::diff_insertions.eq(row.diff_insertions),
                    merge_requests::diff_deletions.eq(row.diff_deletions),
                    merge_requests::has_conflicts.eq(row.has_conflicts),
                    merge_requests::depends_on.eq(row.depends_on),
                    merge_requests::atomic_group.eq(row.atomic_group),
                    merge_requests::workspace_id.eq(row.workspace_id),
                    merge_requests::reverted_at.eq(row.reverted_at),
                    merge_requests::revert_mr_id.eq(row.revert_mr_id),
                    merge_requests::spec_ref.eq(row.spec_ref),
                ))
                .execute(&mut *conn)
                .context("insert merge_request")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<MergeRequest>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<MergeRequest>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = merge_requests::table
                .filter(merge_requests::tenant_id.eq(&tenant))
                .filter(merge_requests::id.eq(id.as_str()))
                .first::<MergeRequestRow>(&mut *conn)
                .optional()
                .context("find merge_request by id")?;
            result.map(MergeRequestRow::into_mr).transpose()
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<MergeRequest>> {
        let pool = Arc::clone(&self.pool);
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<MergeRequest>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = merge_requests::table
                .filter(merge_requests::tenant_id.eq(&tenant))
                .order(merge_requests::created_at.asc())
                .load::<MergeRequestRow>(&mut *conn)
                .context("list merge_requests")?;
            rows.into_iter().map(MergeRequestRow::into_mr).collect()
        })
        .await?
    }

    async fn list_by_status(&self, status: &MrStatus) -> Result<Vec<MergeRequest>> {
        let pool = Arc::clone(&self.pool);
        let status_str = status_to_str(status).to_string();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<MergeRequest>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = merge_requests::table
                .filter(merge_requests::tenant_id.eq(&tenant))
                .filter(merge_requests::status.eq(&status_str))
                .order(merge_requests::created_at.asc())
                .load::<MergeRequestRow>(&mut *conn)
                .context("list merge_requests by status")?;
            rows.into_iter().map(MergeRequestRow::into_mr).collect()
        })
        .await?
    }

    async fn list_by_repo(&self, repository_id: &Id) -> Result<Vec<MergeRequest>> {
        let pool = Arc::clone(&self.pool);
        let repo_id = repository_id.clone();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<MergeRequest>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = merge_requests::table
                .filter(merge_requests::tenant_id.eq(&tenant))
                .filter(merge_requests::repository_id.eq(repo_id.as_str()))
                .order(merge_requests::created_at.asc())
                .load::<MergeRequestRow>(&mut *conn)
                .context("list merge_requests by repo")?;
            rows.into_iter().map(MergeRequestRow::into_mr).collect()
        })
        .await?
    }

    async fn update(&self, mr: &MergeRequest) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let m = mr.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let reviewer_strs: Vec<&str> = m.reviewers.iter().map(|id| id.as_str()).collect();
            let reviewers_json = serde_json::to_string(&reviewer_strs)?;
            let dep_strs: Vec<&str> = m.depends_on.iter().map(|id| id.as_str()).collect();
            let depends_on_json = serde_json::to_string(&dep_strs)?;
            diesel::update(merge_requests::table.find(m.id.as_str()))
                .set((
                    merge_requests::title.eq(&m.title),
                    merge_requests::source_branch.eq(&m.source_branch),
                    merge_requests::target_branch.eq(&m.target_branch),
                    merge_requests::status.eq(status_to_str(&m.status)),
                    merge_requests::author_agent_id
                        .eq(m.author_agent_id.as_ref().map(|id| id.as_str())),
                    merge_requests::reviewers.eq(&reviewers_json),
                    merge_requests::updated_at.eq(m.updated_at as i64),
                    merge_requests::diff_files_changed
                        .eq(m.diff_stats.as_ref().map(|d| d.files_changed as i64)),
                    merge_requests::diff_insertions
                        .eq(m.diff_stats.as_ref().map(|d| d.insertions as i64)),
                    merge_requests::diff_deletions
                        .eq(m.diff_stats.as_ref().map(|d| d.deletions as i64)),
                    merge_requests::has_conflicts
                        .eq(m.has_conflicts.map(|v| if v { 1i32 } else { 0 })),
                    merge_requests::depends_on.eq(&depends_on_json),
                    merge_requests::atomic_group.eq(m.atomic_group.as_deref()),
                    merge_requests::reverted_at.eq(m.reverted_at.map(|v| v as i64)),
                    merge_requests::revert_mr_id.eq(m.revert_mr_id.as_ref().map(|id| id.as_str())),
                    merge_requests::spec_ref.eq(m.spec_ref.as_deref()),
                ))
                .execute(&mut *conn)
                .context("update merge_request")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(merge_requests::table.find(id.as_str()))
                .execute(&mut *conn)
                .context("delete merge_request")?;
            Ok(())
        })
        .await?
    }

    async fn list_dependents(&self, mr_id: &Id) -> Result<Vec<Id>> {
        // Load all MRs and filter in memory (JSON column scan).
        let all = self.list().await?;
        let target = mr_id.as_str().to_string();
        let dependents = all
            .into_iter()
            .filter(|mr| mr.depends_on.iter().any(|dep| dep.as_str() == target))
            .map(|mr| mr.id)
            .collect();
        Ok(dependents)
    }
    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<MergeRequest>> {
        let pool = Arc::clone(&self.pool);
        let workspace_id = workspace_id.clone();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<MergeRequest>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = merge_requests::table
                .filter(merge_requests::tenant_id.eq(&tenant))
                .filter(merge_requests::workspace_id.eq(workspace_id.as_str()))
                .order(merge_requests::created_at.asc())
                .load::<MergeRequestRow>(&mut *conn)
                .context("list merge_requests by workspace")?;
            rows.into_iter().map(MergeRequestRow::into_mr).collect()
        })
        .await?
    }
}
