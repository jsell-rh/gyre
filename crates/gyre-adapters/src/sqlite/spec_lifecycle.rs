use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_domain::SpecLifecycleConfig;
use gyre_ports::SpecLifecycleRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::spec_lifecycle_configs;

#[derive(Queryable, Selectable)]
#[diesel(table_name = spec_lifecycle_configs)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct SpecLifecycleRow {
    #[allow(dead_code)]
    repo_id: String,
    config: String,
}

#[derive(Insertable)]
#[diesel(table_name = spec_lifecycle_configs)]
struct NewSpecLifecycleRow<'a> {
    repo_id: &'a str,
    config: &'a str,
}

#[async_trait]
impl SpecLifecycleRepository for SqliteStorage {
    async fn get_for_repo(&self, repo_id: &str) -> Result<SpecLifecycleConfig> {
        let pool = Arc::clone(&self.pool);
        let repo_id = repo_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<SpecLifecycleConfig> {
            let mut conn = pool.get().context("get db connection")?;
            let row = spec_lifecycle_configs::table
                .find(&repo_id)
                .first::<SpecLifecycleRow>(&mut *conn)
                .optional()
                .context("find spec lifecycle config for repo")?;
            match row {
                None => Ok(SpecLifecycleConfig::default()),
                Some(row) => Ok(serde_json::from_str(&row.config)
                    .context("deserialize spec lifecycle config")?),
            }
        })
        .await?
    }

    async fn set_for_repo(&self, repo_id: &str, config: SpecLifecycleConfig) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let repo_id = repo_id.to_string();
        let config = serde_json::to_string(&config).context("serialize spec lifecycle config")?;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewSpecLifecycleRow {
                repo_id: &repo_id,
                config: &config,
            };
            diesel::insert_into(spec_lifecycle_configs::table)
                .values(&row)
                .on_conflict(spec_lifecycle_configs::repo_id)
                .do_update()
                .set(spec_lifecycle_configs::config.eq(&config))
                .execute(&mut *conn)
                .context("upsert spec lifecycle config")?;
            Ok(())
        })
        .await?
    }
}
