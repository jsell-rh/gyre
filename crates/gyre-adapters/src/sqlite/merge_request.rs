use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{DiffStats, MergeRequest, MrStatus};
use gyre_ports::MergeRequestRepository;

use super::{open_conn, SqliteStorage};

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

fn row_to_mr(row: &rusqlite::Row<'_>) -> Result<MergeRequest> {
    let status_str: String = row.get(5)?;
    let reviewers_json: String = row.get(7)?;
    let reviewer_strs: Vec<String> = serde_json::from_str(&reviewers_json).unwrap_or_default();
    let diff_files: Option<i64> = row.get(10)?;
    let diff_ins: Option<i64> = row.get(11)?;
    let diff_del: Option<i64> = row.get(12)?;
    let diff_stats = match (diff_files, diff_ins, diff_del) {
        (Some(f), Some(i), Some(d)) => Some(DiffStats {
            files_changed: f as usize,
            insertions: i as usize,
            deletions: d as usize,
        }),
        _ => None,
    };
    let has_conflicts: Option<i64> = row.get(13)?;
    Ok(MergeRequest {
        id: Id::new(row.get::<_, String>(0)?),
        repository_id: Id::new(row.get::<_, String>(1)?),
        title: row.get(2)?,
        source_branch: row.get(3)?,
        target_branch: row.get(4)?,
        status: str_to_status(&status_str)?,
        author_agent_id: row.get::<_, Option<String>>(6)?.map(Id::new),
        reviewers: reviewer_strs.into_iter().map(Id::new).collect(),
        diff_stats,
        has_conflicts: has_conflicts.map(|v| v != 0),
        created_at: row.get::<_, i64>(8)? as u64,
        updated_at: row.get::<_, i64>(9)? as u64,
    })
}

#[async_trait]
impl MergeRequestRepository for SqliteStorage {
    async fn create(&self, mr: &MergeRequest) -> Result<()> {
        let path = self.db_path();
        let m = mr.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let reviewer_ids: Vec<&str> = m.reviewers.iter().map(|id| id.as_str()).collect();
            let reviewers_json = serde_json::to_string(&reviewer_ids)?;
            let conn = open_conn(&path)?;
            let diff_files = m.diff_stats.as_ref().map(|d| d.files_changed as i64);
            let diff_ins = m.diff_stats.as_ref().map(|d| d.insertions as i64);
            let diff_del = m.diff_stats.as_ref().map(|d| d.deletions as i64);
            let conflicts = m.has_conflicts.map(|v| if v { 1i64 } else { 0i64 });
            conn.execute(
                "INSERT INTO merge_requests (id, repository_id, title, source_branch, target_branch,
                                             status, author_agent_id, reviewers, created_at, updated_at,
                                             diff_files_changed, diff_insertions, diff_deletions, has_conflicts)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                rusqlite::params![
                    m.id.as_str(),
                    m.repository_id.as_str(),
                    m.title,
                    m.source_branch,
                    m.target_branch,
                    status_to_str(&m.status),
                    m.author_agent_id.as_ref().map(|id| id.as_str()),
                    reviewers_json,
                    m.created_at as i64,
                    m.updated_at as i64,
                    diff_files,
                    diff_ins,
                    diff_del,
                    conflicts,
                ],
            )
            .context("insert merge_request")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<MergeRequest>> {
        let path = self.db_path();
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<MergeRequest>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, repository_id, title, source_branch, target_branch,
                        status, author_agent_id, reviewers, created_at, updated_at,
                        diff_files_changed, diff_insertions, diff_deletions, has_conflicts
                 FROM merge_requests WHERE id = ?1",
            )?;
            let mut rows = stmt.query([id.as_str()])?;
            if let Some(row) = rows.next()? {
                Ok(Some(row_to_mr(row)?))
            } else {
                Ok(None)
            }
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<MergeRequest>> {
        let path = self.db_path();
        tokio::task::spawn_blocking(move || -> Result<Vec<MergeRequest>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, repository_id, title, source_branch, target_branch,
                        status, author_agent_id, reviewers, created_at, updated_at,
                        diff_files_changed, diff_insertions, diff_deletions, has_conflicts
                 FROM merge_requests ORDER BY created_at",
            )?;
            let rows = stmt.query_map([], |row| Ok(row_to_mr(row).unwrap()))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn list_by_status(&self, status: &MrStatus) -> Result<Vec<MergeRequest>> {
        let path = self.db_path();
        let status_str = status_to_str(status).to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<MergeRequest>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, repository_id, title, source_branch, target_branch,
                        status, author_agent_id, reviewers, created_at, updated_at,
                        diff_files_changed, diff_insertions, diff_deletions, has_conflicts
                 FROM merge_requests WHERE status = ?1 ORDER BY created_at",
            )?;
            let rows = stmt.query_map([&status_str], |row| Ok(row_to_mr(row).unwrap()))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn list_by_repo(&self, repository_id: &Id) -> Result<Vec<MergeRequest>> {
        let path = self.db_path();
        let repo_id = repository_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<MergeRequest>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, repository_id, title, source_branch, target_branch,
                        status, author_agent_id, reviewers, created_at, updated_at,
                        diff_files_changed, diff_insertions, diff_deletions, has_conflicts
                 FROM merge_requests WHERE repository_id = ?1 ORDER BY created_at",
            )?;
            let rows = stmt.query_map([repo_id.as_str()], |row| Ok(row_to_mr(row).unwrap()))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn update(&self, mr: &MergeRequest) -> Result<()> {
        let path = self.db_path();
        let m = mr.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let reviewer_ids: Vec<&str> = m.reviewers.iter().map(|id| id.as_str()).collect();
            let reviewers_json = serde_json::to_string(&reviewer_ids)?;
            let conn = open_conn(&path)?;
            let diff_files = m.diff_stats.as_ref().map(|d| d.files_changed as i64);
            let diff_ins = m.diff_stats.as_ref().map(|d| d.insertions as i64);
            let diff_del = m.diff_stats.as_ref().map(|d| d.deletions as i64);
            let conflicts = m.has_conflicts.map(|v| if v { 1i64 } else { 0i64 });
            conn.execute(
                "UPDATE merge_requests SET title=?1, source_branch=?2, target_branch=?3,
                          status=?4, author_agent_id=?5, reviewers=?6, updated_at=?7,
                          diff_files_changed=?8, diff_insertions=?9, diff_deletions=?10,
                          has_conflicts=?11
                 WHERE id=?12",
                rusqlite::params![
                    m.title,
                    m.source_branch,
                    m.target_branch,
                    status_to_str(&m.status),
                    m.author_agent_id.as_ref().map(|id| id.as_str()),
                    reviewers_json,
                    m.updated_at as i64,
                    diff_files,
                    diff_ins,
                    diff_del,
                    conflicts,
                    m.id.as_str(),
                ],
            )
            .context("update merge_request")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let path = self.db_path();
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute("DELETE FROM merge_requests WHERE id=?1", [id.as_str()])
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
        // Ignore error if project already exists
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
