use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{PreviewEnvironment, PreviewEnvironmentStatus};
use gyre_ports::PreviewEnvironmentRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::preview_environments;

#[derive(Queryable, Selectable)]
#[diesel(table_name = preview_environments)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct PreviewEnvironmentRow {
    id: String,
    tenant_id: String,
    workspace_id: String,
    repo_id: String,
    mr_id: String,
    requested_by: String,
    commit_sha: String,
    branch: String,
    status: String,
    mesh_ip: Option<String>,
    peer_id: Option<String>,
    agent_id: String,
    compute_target: String,
    image: Option<String>,
    data_dir: String,
    port: i32,
    process_handle: Option<String>,
    created_at: i64,
    expires_at: i64,
    ended_at: Option<i64>,
    end_reason: Option<String>,
}

fn parse_status(s: &str) -> Result<PreviewEnvironmentStatus> {
    match s {
        "provisioning" => Ok(PreviewEnvironmentStatus::Provisioning),
        "active" => Ok(PreviewEnvironmentStatus::Active),
        "expired" => Ok(PreviewEnvironmentStatus::Expired),
        "failed" => Ok(PreviewEnvironmentStatus::Failed),
        other => Err(anyhow::anyhow!(
            "unknown preview environment status: {other:?}"
        )),
    }
}

impl PreviewEnvironmentRow {
    fn into_env(self) -> Result<PreviewEnvironment> {
        Ok(PreviewEnvironment {
            id: Id::new(self.id),
            tenant_id: Id::new(self.tenant_id),
            workspace_id: Id::new(self.workspace_id),
            repo_id: Id::new(self.repo_id),
            mr_id: Id::new(self.mr_id),
            requested_by: Id::new(self.requested_by),
            commit_sha: self.commit_sha,
            branch: self.branch,
            status: parse_status(&self.status)?,
            mesh_ip: self.mesh_ip,
            peer_id: self.peer_id.map(Id::new),
            agent_id: Id::new(self.agent_id),
            compute_target: self.compute_target,
            image: self.image,
            data_dir: self.data_dir,
            port: u16::try_from(self.port).context("preview environment port")?,
            process_handle: self.process_handle,
            created_at: self.created_at as u64,
            expires_at: self.expires_at as u64,
            ended_at: self.ended_at.map(|v| v as u64),
            end_reason: self.end_reason,
        })
    }
}

#[derive(Insertable, AsChangeset)]
#[diesel(table_name = preview_environments)]
struct PreviewEnvironmentRecord<'a> {
    id: &'a str,
    tenant_id: &'a str,
    workspace_id: &'a str,
    repo_id: &'a str,
    mr_id: &'a str,
    requested_by: &'a str,
    commit_sha: &'a str,
    branch: &'a str,
    status: &'a str,
    mesh_ip: Option<&'a str>,
    peer_id: Option<&'a str>,
    agent_id: &'a str,
    compute_target: &'a str,
    image: Option<&'a str>,
    data_dir: &'a str,
    port: i32,
    process_handle: Option<&'a str>,
    created_at: i64,
    expires_at: i64,
    ended_at: Option<i64>,
    end_reason: Option<&'a str>,
}

impl<'a> PreviewEnvironmentRecord<'a> {
    fn from_env(env: &'a PreviewEnvironment) -> Self {
        Self {
            id: env.id.as_str(),
            tenant_id: env.tenant_id.as_str(),
            workspace_id: env.workspace_id.as_str(),
            repo_id: env.repo_id.as_str(),
            mr_id: env.mr_id.as_str(),
            requested_by: env.requested_by.as_str(),
            commit_sha: &env.commit_sha,
            branch: &env.branch,
            status: env.status.as_str(),
            mesh_ip: env.mesh_ip.as_deref(),
            peer_id: env.peer_id.as_ref().map(|id| id.as_str()),
            agent_id: env.agent_id.as_str(),
            compute_target: &env.compute_target,
            image: env.image.as_deref(),
            data_dir: &env.data_dir,
            port: env.port as i32,
            process_handle: env.process_handle.as_deref(),
            created_at: env.created_at as i64,
            expires_at: env.expires_at as i64,
            ended_at: env.ended_at.map(|v| v as i64),
            end_reason: env.end_reason.as_deref(),
        }
    }
}

#[async_trait]
impl PreviewEnvironmentRepository for SqliteStorage {
    async fn create(&self, env: &PreviewEnvironment) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let env = env.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let record = PreviewEnvironmentRecord::from_env(&env);
            diesel::insert_into(preview_environments::table)
                .values(&record)
                .execute(&mut *conn)
                .context("insert preview_environment")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<PreviewEnvironment>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<PreviewEnvironment>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = preview_environments::table
                .find(id.as_str())
                .first::<PreviewEnvironmentRow>(&mut *conn)
                .optional()
                .context("find preview_environment by id")?;
            row.map(|r| r.into_env()).transpose()
        })
        .await?
    }

    async fn find_by_mr(&self, mr_id: &Id) -> Result<Vec<PreviewEnvironment>> {
        let pool = Arc::clone(&self.pool);
        let mr_id = mr_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<PreviewEnvironment>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = preview_environments::table
                .filter(preview_environments::mr_id.eq(mr_id.as_str()))
                .order(preview_environments::created_at.desc())
                .load::<PreviewEnvironmentRow>(&mut *conn)
                .context("find preview_environments by mr")?;
            rows.into_iter().map(|r| r.into_env()).collect()
        })
        .await?
    }

    async fn find_by_workspace(&self, workspace_id: &Id) -> Result<Vec<PreviewEnvironment>> {
        let pool = Arc::clone(&self.pool);
        let ws_id = workspace_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<PreviewEnvironment>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = preview_environments::table
                .filter(preview_environments::workspace_id.eq(ws_id.as_str()))
                .order(preview_environments::created_at.desc())
                .load::<PreviewEnvironmentRow>(&mut *conn)
                .context("find preview_environments by workspace")?;
            rows.into_iter().map(|r| r.into_env()).collect()
        })
        .await?
    }

    async fn find_live_expired_before(&self, now: u64) -> Result<Vec<PreviewEnvironment>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<PreviewEnvironment>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = preview_environments::table
                .filter(preview_environments::expires_at.le(now as i64))
                .filter(
                    preview_environments::status
                        .eq("provisioning")
                        .or(preview_environments::status.eq("active")),
                )
                .order(preview_environments::expires_at.asc())
                .load::<PreviewEnvironmentRow>(&mut *conn)
                .context("find expired live preview_environments")?;
            rows.into_iter().map(|r| r.into_env()).collect()
        })
        .await?
    }

    async fn update(&self, env: &PreviewEnvironment) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let env = env.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let record = PreviewEnvironmentRecord::from_env(&env);
            let updated = diesel::update(preview_environments::table.find(env.id.as_str()))
                .set(&record)
                .execute(&mut *conn)
                .context("update preview_environment")?;
            // Port contract: update fails when the row does not exist — a
            // status transition on a deleted environment must not silently
            // no-op.
            if updated == 0 {
                anyhow::bail!("preview environment {} not found", env.id);
            }
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(preview_environments::table.find(id.as_str()))
                .execute(&mut *conn)
                .context("delete preview_environment")?;
            Ok(())
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
        let s = SqliteStorage::new_for_tenant(tmp.path().to_str().unwrap(), "tenant-1").unwrap();
        (tmp, s)
    }

    fn env(id: &str, mr: &str, created_at: u64, expires_at: u64) -> PreviewEnvironment {
        PreviewEnvironment::new(
            Id::new(id),
            Id::new("tenant-1"),
            Id::new("ws-1"),
            Id::new("repo-1"),
            Id::new(mr),
            Id::new("user-1"),
            "0123456789abcdef0123456789abcdef01234567",
            "main",
            Id::new("agent-1"),
            "container",
            "/tmp/gyre-preview-1",
            18421,
            created_at,
            expires_at,
        )
    }

    #[tokio::test]
    async fn create_and_find_by_id_round_trips_all_fields() {
        let (_tmp, s) = setup();
        let mut e = env("pe-1", "mr-1", 1000, 4600);
        e.status = PreviewEnvironmentStatus::Active;
        e.mesh_ip = Some("10.100.0.5".to_string());
        e.peer_id = Some(Id::new("peer-1"));
        e.image = Some("gyre-agent:latest".to_string());
        s.create(&e).await.unwrap();

        let loaded = s.find_by_id(&Id::new("pe-1")).await.unwrap().unwrap();
        assert_eq!(loaded.status, PreviewEnvironmentStatus::Active);
        assert_eq!(loaded.mesh_ip.as_deref(), Some("10.100.0.5"));
        assert_eq!(loaded.peer_id.as_ref().map(|p| p.as_str()), Some("peer-1"));
        assert_eq!(loaded.image.as_deref(), Some("gyre-agent:latest"));
        assert_eq!(loaded.commit_sha, e.commit_sha);
        assert_eq!(loaded.port, 18421);
        assert_eq!(loaded.expires_at, 4600);
    }

    #[tokio::test]
    async fn create_duplicate_id_fails() {
        let (_tmp, s) = setup();
        s.create(&env("pe-1", "mr-1", 1000, 4600)).await.unwrap();
        // Same id again: the insert must fail, not overwrite (port contract).
        assert!(s.create(&env("pe-1", "mr-2", 2000, 5600)).await.is_err());
        // The original row is intact.
        let loaded = s.find_by_id(&Id::new("pe-1")).await.unwrap().unwrap();
        assert_eq!(loaded.mr_id.as_str(), "mr-1");
    }

    #[tokio::test]
    async fn update_missing_row_fails() {
        let (_tmp, s) = setup();
        let e = env("pe-nope", "mr-1", 1000, 4600);
        assert!(s.update(&e).await.is_err());
    }

    #[tokio::test]
    async fn find_by_mr_orders_newest_first() {
        let (_tmp, s) = setup();
        s.create(&env("pe-old", "mr-1", 1000, 4600)).await.unwrap();
        s.create(&env("pe-new", "mr-1", 2000, 5600)).await.unwrap();
        s.create(&env("pe-other", "mr-2", 3000, 6600))
            .await
            .unwrap();

        let rows = s.find_by_mr(&Id::new("mr-1")).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id.as_str(), "pe-new");
        assert_eq!(rows[1].id.as_str(), "pe-old");
    }

    #[tokio::test]
    async fn find_live_expired_before_returns_only_live_past_deadline() {
        let (_tmp, s) = setup();
        let mut live = env("pe-live", "mr-1", 1000, 1500);
        live.status = PreviewEnvironmentStatus::Active;
        let mut not_yet = env("pe-future", "mr-1", 1000, 9900);
        not_yet.status = PreviewEnvironmentStatus::Active;
        let mut already_ended = env("pe-ended", "mr-1", 1000, 1200);
        already_ended.status = PreviewEnvironmentStatus::Expired;

        s.create(&live).await.unwrap();
        s.create(&not_yet).await.unwrap();
        s.create(&already_ended).await.unwrap();

        let rows = s.find_live_expired_before(2000).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id.as_str(), "pe-live");
    }

    #[tokio::test]
    async fn durability_across_reopen() {
        // The durable-state invariant: a row written before a "restart"
        // (fresh storage handle over the same database file) is still
        // queryable after it.
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("preview.sqlite3");

        {
            let s = SqliteStorage::new(db_path.to_str().unwrap()).unwrap();
            s.create(&env("pe-1", "mr-1", 1000, 4600)).await.unwrap();
        }
        {
            let s = SqliteStorage::new(db_path.to_str().unwrap()).unwrap();
            let loaded = s.find_by_id(&Id::new("pe-1")).await.unwrap();
            assert!(loaded.is_some(), "preview row must survive a restart");
        }
    }
}
