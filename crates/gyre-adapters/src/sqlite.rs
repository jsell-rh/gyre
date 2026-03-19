use anyhow::Result;
use async_trait::async_trait;
use gyre_ports::storage::StoragePort;
use tracing::instrument;

/// SQLite-backed storage adapter.
/// Implements the StoragePort for local/development deployments.
pub struct SqliteStorage {
    db_path: String,
}

impl SqliteStorage {
    /// Open (or create) the SQLite database and run initial migrations.
    pub fn new(db_path: impl Into<String>) -> Result<Self> {
        let db_path = db_path.into();
        let conn = rusqlite::Connection::open(&db_path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (
                id         INTEGER PRIMARY KEY,
                name       TEXT NOT NULL,
                applied_at TEXT NOT NULL
            );",
        )?;
        Ok(Self { db_path })
    }
}

#[async_trait]
impl StoragePort for SqliteStorage {
    #[instrument(skip(self), err)]
    async fn health_check(&self) -> Result<()> {
        let path = self.db_path.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = rusqlite::Connection::open(&path)?;
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

    #[tokio::test]
    async fn health_check_ok() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        storage.health_check().await.unwrap();
    }

    #[test]
    fn new_creates_migrations_table() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        // Re-open and verify the table exists.
        let conn = rusqlite::Connection::open(&storage.db_path).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='_migrations'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
