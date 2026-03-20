use anyhow::Result;
use async_trait::async_trait;
use gyre_ports::storage::StoragePort;
use std::sync::Arc;
use tracing::instrument;

pub mod activity;
pub mod agent;
pub mod agent_commit;
pub mod analytics;
pub mod audit;
pub mod merge_queue;
pub mod merge_request;
mod migrations;
pub mod project;
pub mod repository;
pub mod review;
pub mod task;
pub mod user;
pub mod worktree;

/// SQLite-backed storage adapter.
/// Implements all port traits for local/development deployments.
pub struct SqliteStorage {
    db_path: Arc<str>,
}

impl SqliteStorage {
    /// Open (or create) the SQLite database and run migrations.
    pub fn new(db_path: impl Into<String>) -> Result<Self> {
        let db_path: Arc<str> = db_path.into().into();
        let conn = open_conn(db_path.as_ref())?;
        migrations::run(&conn)?;
        Ok(Self { db_path })
    }

    pub(crate) fn db_path(&self) -> Arc<str> {
        Arc::clone(&self.db_path)
    }
}

/// Open a rusqlite connection with foreign keys enabled.
pub(crate) fn open_conn(path: &str) -> Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(path)?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    Ok(conn)
}

#[async_trait]
impl StoragePort for SqliteStorage {
    #[instrument(skip(self), err)]
    async fn health_check(&self) -> Result<()> {
        let path = self.db_path();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute_batch("SELECT 1;")?;
            Ok(())
        })
        .await??;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn tmp_storage() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let storage = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, storage)
    }

    #[tokio::test]
    async fn health_check_ok() {
        let (_tmp, storage) = tmp_storage();
        storage.health_check().await.unwrap();
    }

    #[test]
    fn migrations_create_tables() {
        let (_tmp, storage) = tmp_storage();
        let conn = rusqlite::Connection::open(storage.db_path.as_ref()).unwrap();
        let tables = [
            "_migrations",
            "projects",
            "repositories",
            "agents",
            "tasks",
            "merge_requests",
            "activity_events",
            "agent_commits",
            "agent_worktrees",
            "users",
            "api_keys",
            "analytics_events",
            "cost_entries",
            "audit_events",
            "siem_targets",
        ];
        for table in &tables {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "table '{}' missing", table);
        }
    }

    #[test]
    fn migration_is_idempotent() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap();
        // Run twice — should not fail.
        SqliteStorage::new(path).unwrap();
        SqliteStorage::new(path).unwrap();
    }
}
