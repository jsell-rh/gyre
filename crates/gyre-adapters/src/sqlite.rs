use anyhow::Result;
use async_trait::async_trait;
use gyre_ports::storage::StoragePort;

/// SQLite-backed storage adapter.
/// Implements the StoragePort for local/development deployments.
pub struct SqliteStorage {
    // db_path: String,  // will hold connection pool once rusqlite/sqlx added
}

impl SqliteStorage {
    pub fn new(_db_path: impl Into<String>) -> Self {
        Self {}
    }
}

#[async_trait]
impl StoragePort for SqliteStorage {
    async fn health_check(&self) -> Result<()> {
        // TODO: verify DB file accessible and schema version correct
        Ok(())
    }
}
