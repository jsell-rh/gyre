use anyhow::Result;
use async_trait::async_trait;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel::PgConnection;
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
pub mod spawn_log;
pub mod task;
pub mod user;
pub mod worktree;

pub(crate) const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

pub(crate) type DbPool = Pool<ConnectionManager<PgConnection>>;

/// Diesel-backed PostgreSQL storage adapter.
/// Implements all port traits. All queries are scoped to `tenant_id`.
#[derive(Clone)]
pub struct PgStorage {
    pub(crate) pool: Arc<DbPool>,
    pub(crate) tenant_id: String,
}

impl PgStorage {
    /// Connect to a PostgreSQL database and run pending migrations.
    pub fn new(database_url: impl Into<String>) -> Result<Self> {
        Self::new_for_tenant(database_url, "default")
    }

    /// Connect scoped to a specific tenant.
    pub fn new_for_tenant(
        database_url: impl Into<String>,
        tenant_id: impl Into<String>,
    ) -> Result<Self> {
        let url: String = database_url.into();
        let manager = ConnectionManager::<PgConnection>::new(&url);
        let pool = Pool::builder().build(manager)?;

        // Run pending Diesel migrations.
        {
            let mut conn = pool.get()?;
            conn.run_pending_migrations(MIGRATIONS)
                .map_err(|e| anyhow::anyhow!("Diesel PG migration failed: {e}"))?;
        }

        Ok(Self {
            pool: Arc::new(pool),
            tenant_id: tenant_id.into(),
        })
    }
}

#[async_trait]
impl StoragePort for PgStorage {
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

/// Stub implementation of SpawnLogRepository for PgStorage.
/// Full Diesel-backed implementation deferred until M15.3.
#[async_trait]
impl gyre_ports::SpawnLogRepository for PgStorage {
    async fn append_spawn_step(
        &self,
        _agent_id: &str,
        _step: &str,
        _status: &str,
        _detail: Option<&str>,
        _occurred_at: u64,
    ) -> Result<()> {
        Ok(())
    }

    async fn get_spawn_log(&self, _agent_id: &str) -> Result<Vec<gyre_ports::SpawnLogEntry>> {
        Ok(vec![])
    }

    async fn revoke_token(
        &self,
        _token_hash: &str,
        _agent_id: &str,
        _revoked_at: u64,
    ) -> Result<()> {
        Ok(())
    }

    async fn is_token_revoked(&self, _token_hash: &str) -> Result<bool> {
        Ok(false)
    }
}
