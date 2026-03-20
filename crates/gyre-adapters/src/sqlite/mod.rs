use anyhow::Result;
use async_trait::async_trait;
use diesel::r2d2::{ConnectionManager, CustomizeConnection, Error as R2d2Error, Pool};
use diesel::SqliteConnection;
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
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
pub mod network_peer;
pub mod project;
pub mod repository;
pub mod review;
pub mod task;
pub mod user;
pub mod worktree;

pub(crate) const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

/// r2d2 connection customizer: enables WAL mode and foreign keys on every connection.
#[derive(Debug)]
struct SqliteCustomizer;

impl CustomizeConnection<SqliteConnection, R2d2Error> for SqliteCustomizer {
    fn on_acquire(&self, conn: &mut SqliteConnection) -> Result<(), R2d2Error> {
        use diesel::RunQueryDsl;
        diesel::sql_query("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .execute(conn)
            .map_err(R2d2Error::QueryError)?;
        Ok(())
    }
}

pub(crate) type DbPool = Pool<ConnectionManager<SqliteConnection>>;

/// Diesel-backed storage adapter.
/// Implements all port traits for local/development deployments.
/// All queries are scoped to `tenant_id` for multi-tenant isolation.
#[derive(Clone)]
pub struct SqliteStorage {
    /// Diesel r2d2 connection pool.
    pub(crate) pool: Arc<DbPool>,
    /// Tenant scope for all queries. Defaults to "default".
    pub(crate) tenant_id: String,
}

impl SqliteStorage {
    /// Open (or create) the SQLite database and run Diesel migrations.
    /// Uses the "default" tenant scope.
    pub fn new(db_path: impl Into<String>) -> Result<Self> {
        Self::new_for_tenant(db_path, "default")
    }

    /// Open (or create) the SQLite database scoped to a specific tenant.
    /// Security: "system" tenant should only be used by Admin-role callers.
    pub fn new_for_tenant(
        db_path: impl Into<String>,
        tenant_id: impl Into<String>,
    ) -> Result<Self> {
        let db_path: String = db_path.into();

        // Build r2d2 pool with WAL + foreign keys customizer.
        let manager = ConnectionManager::<SqliteConnection>::new(&db_path);
        let pool = Pool::builder()
            .connection_customizer(Box::new(SqliteCustomizer))
            .build(manager)?;

        // Run Diesel migrations.
        {
            let mut conn = pool.get()?;
            conn.run_pending_migrations(MIGRATIONS)
                .map_err(|e| anyhow::anyhow!("Diesel migration failed: {e}"))?;
        }

        Ok(Self {
            pool: Arc::new(pool),
            tenant_id: tenant_id.into(),
        })
    }
}

#[async_trait]
impl StoragePort for SqliteStorage {
    #[instrument(skip(self), err)]
    async fn health_check(&self) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<()> {
            use diesel::RunQueryDsl;
            let mut conn = pool.get()?;
            diesel::sql_query("SELECT 1")
                .execute(&mut *conn)
                .map_err(anyhow::Error::from)?;
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
        let mut conn = storage.pool.get().unwrap();
        let tables = [
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
            "network_peers",
        ];
        for table in &tables {
            use diesel::RunQueryDsl;
            // A table exists if we can SELECT from it without error.
            diesel::sql_query(format!("SELECT 1 FROM {table} LIMIT 0"))
                .execute(&mut *conn)
                .unwrap_or_else(|e| panic!("table '{table}' missing: {e}"));
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
