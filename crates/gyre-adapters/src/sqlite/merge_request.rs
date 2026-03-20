use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{DiffStats, MergeRequest, MrStatus};
use gyre_ports::MergeRequestRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::merge_requests;

fn status_to_str(s: &MrStatus) -> &'static str {
    match s {
        MrStatus::Open => "Open",
        MrStatus::Approved => "Approved",
        MrStatus::Merged => "Merged",
        MrStatus::Closed => "Closed",
    }
}

fn str_to_status(s: &str) -> Result<MrStatus> {
    match s {
        "Open" => Ok(MrStatus::Open),
        "Approved" => Ok(MrStatus::Approved),
        "Merged" => Ok(MrStatus::Merged),
        "Closed" => Ok(MrStatus::Closed),
        other => Err(anyhow!("unknown MR status: {}", other)),
    }
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = merge_requests)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
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
}

impl MergeRequestRow {
    fn into_mr(self) -> Result<MergeRequest> {
        let reviewer_strs: Vec<String> = serde_json::from_str(&self.reviewers).unwrap_or_default();
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
            created_at: self.created_at as u64,
            updated_at: self.updated_at as u64,
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
}

#[async_trait]
impl MergeRequestRepository for SqliteStorage {
    async fn create(&self, mr: &MergeRequest) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let m = mr.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let reviewer_strs: Vec<&str> = m.reviewers.iter().map(|id| id.as_str()).collect();
            let reviewers_json = serde_json::to_string(&reviewer_strs)?;
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
        tokio::task::spawn_blocking(move || -> Result<Option<MergeRequest>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = merge_requests::table
                .find(id.as_str())
                .first::<MergeRequestRow>(&mut *conn)
                .optional()
                .context("find merge_request by id")?;
            result.map(MergeRequestRow::into_mr).transpose()
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<MergeRequest>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<MergeRequest>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = merge_requests::table
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
        tokio::task::spawn_blocking(move || -> Result<Vec<MergeRequest>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = merge_requests::table
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
        tokio::task::spawn_blocking(move || -> Result<Vec<MergeRequest>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = merge_requests::table
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqliteStorage;
    use gyre_domain::{Project, Repository};
    use gyre_ports::{ProjectRepository, RepoRepository};
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    async fn create_repo(s: &SqliteStorage, project_id: &str, repo_id: &str) {
        let p = Project::new(Id::new(project_id), format!("proj-{}", project_id), 1000);
        let _ = ProjectRepository::create(s, &p).await;
        let r = Repository::new(
            Id::new(repo_id),
            Id::new(project_id),
            format!("repo-{}", repo_id),
            format!("/repos/{}", repo_id),
            1000,
        );
        RepoRepository::create(s, &r).await.unwrap();
    }

    fn make_mr(id: &str, repo_id: &str) -> MergeRequest {
        MergeRequest::new(
            Id::new(id),
            Id::new(repo_id),
            format!("MR {}", id),
            "feat/x",
            "main",
            1000,
        )
    }

    #[tokio::test]
    async fn create_and_find() {
        let (_tmp, s) = setup();
        create_repo(&s, "p1", "r1").await;
        let mr = make_mr("mr1", "r1");
        MergeRequestRepository::create(&s, &mr).await.unwrap();
        let found = MergeRequestRepository::find_by_id(&s, &mr.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.title, "MR mr1");
        assert_eq!(found.status, MrStatus::Open);
    }

    #[tokio::test]
    async fn find_missing_returns_none() {
        let (_tmp, s) = setup();
        let result = MergeRequestRepository::find_by_id(&s, &Id::new("nope"))
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn list_merge_requests() {
        let (_tmp, s) = setup();
        create_repo(&s, "p1", "r1").await;
        MergeRequestRepository::create(&s, &make_mr("mr1", "r1"))
            .await
            .unwrap();
        MergeRequestRepository::create(&s, &make_mr("mr2", "r1"))
            .await
            .unwrap();
        assert_eq!(MergeRequestRepository::list(&s).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn list_by_status() {
        let (_tmp, s) = setup();
        create_repo(&s, "p1", "r1").await;
        let mut mr1 = make_mr("mr1", "r1");
        let mr2 = make_mr("mr2", "r1");
        MergeRequestRepository::create(&s, &mr1).await.unwrap();
        MergeRequestRepository::create(&s, &mr2).await.unwrap();
        mr1.status = MrStatus::Approved;
        mr1.updated_at = 2000;
        MergeRequestRepository::update(&s, &mr1).await.unwrap();

        let approved = MergeRequestRepository::list_by_status(&s, &MrStatus::Approved)
            .await
            .unwrap();
        assert_eq!(approved.len(), 1);
        let open = MergeRequestRepository::list_by_status(&s, &MrStatus::Open)
            .await
            .unwrap();
        assert_eq!(open.len(), 1);
    }

    #[tokio::test]
    async fn list_by_repo() {
        let (_tmp, s) = setup();
        create_repo(&s, "p1", "r1").await;
        create_repo(&s, "p1", "r2").await;
        MergeRequestRepository::create(&s, &make_mr("mr1", "r1"))
            .await
            .unwrap();
        MergeRequestRepository::create(&s, &make_mr("mr2", "r1"))
            .await
            .unwrap();
        MergeRequestRepository::create(&s, &make_mr("mr3", "r2"))
            .await
            .unwrap();

        let r1_mrs = MergeRequestRepository::list_by_repo(&s, &Id::new("r1"))
            .await
            .unwrap();
        assert_eq!(r1_mrs.len(), 2);
        let r2_mrs = MergeRequestRepository::list_by_repo(&s, &Id::new("r2"))
            .await
            .unwrap();
        assert_eq!(r2_mrs.len(), 1);
    }

    #[tokio::test]
    async fn update_merge_request() {
        let (_tmp, s) = setup();
        create_repo(&s, "p1", "r1").await;
        let mut mr = make_mr("mr1", "r1");
        MergeRequestRepository::create(&s, &mr).await.unwrap();
        mr.status = MrStatus::Approved;
        mr.reviewers = vec![Id::new("agent-1"), Id::new("agent-2")];
        mr.updated_at = 9999;
        MergeRequestRepository::update(&s, &mr).await.unwrap();

        let found = MergeRequestRepository::find_by_id(&s, &mr.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.status, MrStatus::Approved);
        assert_eq!(found.reviewers.len(), 2);
        assert_eq!(found.updated_at, 9999);
    }

    #[tokio::test]
    async fn delete_merge_request() {
        let (_tmp, s) = setup();
        create_repo(&s, "p1", "r1").await;
        let mr = make_mr("mr1", "r1");
        MergeRequestRepository::create(&s, &mr).await.unwrap();
        MergeRequestRepository::delete(&s, &mr.id).await.unwrap();
        assert!(MergeRequestRepository::find_by_id(&s, &mr.id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn reviewers_roundtrip() {
        let (_tmp, s) = setup();
        create_repo(&s, "p1", "r1").await;
        let mut mr = make_mr("mr1", "r1");
        mr.reviewers = vec![Id::new("a1"), Id::new("a2"), Id::new("a3")];
        MergeRequestRepository::create(&s, &mr).await.unwrap();
        let found = MergeRequestRepository::find_by_id(&s, &mr.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.reviewers, mr.reviewers);
    }
}
