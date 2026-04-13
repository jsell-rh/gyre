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
pub mod attestation;
pub mod audit;
pub mod authorization_provenance;
pub mod budget;
pub mod compute_target;
pub mod container_audit;
pub mod conversation;
pub mod dependency;
pub mod kv_store;
pub mod llm_config;
pub mod merge_queue;
pub mod merge_request;
pub mod message;
pub mod meta_spec;
pub mod meta_spec_set;
pub mod network_peer;
pub mod notification;
pub mod policy;
pub mod prompt_template;
pub mod push_gate;
pub mod quality_gate;
pub mod repository;
pub mod review;
pub mod spawn_log;
pub mod spec_approval;
pub mod spec_approval_event;
pub mod spec_ledger;
pub mod spec_policy;
pub mod task;
pub mod team;
pub mod tenant;
pub mod trace;
pub mod user;
pub mod user_profile;
pub mod user_workspace_state;
pub mod workspace;
pub mod workspace_membership;
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

// SpawnLogRepository for PgStorage is implemented in spawn_log.rs (merged via PR #136).
