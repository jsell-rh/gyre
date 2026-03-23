use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_domain::BudgetConfig;
use gyre_ports::BudgetRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::budget_configs;

#[derive(Queryable, Selectable)]
#[diesel(table_name = budget_configs)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct BudgetConfigRow {
    entity_key: String,
    max_tokens_per_day: Option<i64>,
    max_cost_per_day: Option<f64>,
    max_concurrent_agents: Option<i32>,
    max_agent_lifetime_secs: Option<i64>,
    #[allow(dead_code)]
    updated_at: i64,
}

impl BudgetConfigRow {
    fn into_config(self) -> BudgetConfig {
        BudgetConfig {
            max_tokens_per_day: self.max_tokens_per_day.map(|v| v as u64),
            max_cost_per_day: self.max_cost_per_day,
            max_concurrent_agents: self.max_concurrent_agents.map(|v| v as u32),
            max_agent_lifetime_secs: self.max_agent_lifetime_secs.map(|v| v as u64),
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = budget_configs)]
struct NewBudgetConfigRow<'a> {
    entity_key: &'a str,
    max_tokens_per_day: Option<i64>,
    max_cost_per_day: Option<f64>,
    max_concurrent_agents: Option<i32>,
    max_agent_lifetime_secs: Option<i64>,
    updated_at: i64,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[async_trait]
impl BudgetRepository for SqliteStorage {
    async fn set_config(&self, entity_key: &str, config: &BudgetConfig) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let key = entity_key.to_string();
        let c = config.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let now = now_secs();
            let row = NewBudgetConfigRow {
                entity_key: &key,
                max_tokens_per_day: c.max_tokens_per_day.map(|v| v as i64),
                max_cost_per_day: c.max_cost_per_day,
                max_concurrent_agents: c.max_concurrent_agents.map(|v| v as i32),
                max_agent_lifetime_secs: c.max_agent_lifetime_secs.map(|v| v as i64),
                updated_at: now,
            };
            diesel::insert_into(budget_configs::table)
                .values(&row)
                .on_conflict(budget_configs::entity_key)
                .do_update()
                .set((
                    budget_configs::max_tokens_per_day.eq(row.max_tokens_per_day),
                    budget_configs::max_cost_per_day.eq(row.max_cost_per_day),
                    budget_configs::max_concurrent_agents.eq(row.max_concurrent_agents),
                    budget_configs::max_agent_lifetime_secs.eq(row.max_agent_lifetime_secs),
                    budget_configs::updated_at.eq(now),
                ))
                .execute(&mut *conn)
                .context("upsert budget config")?;
            Ok(())
        })
        .await?
    }

    async fn get_config(&self, entity_key: &str) -> Result<Option<BudgetConfig>> {
        let pool = Arc::clone(&self.pool);
        let key = entity_key.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<BudgetConfig>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = budget_configs::table
                .find(&key)
                .first::<BudgetConfigRow>(&mut *conn)
                .optional()
                .context("get budget config")?;
            Ok(result.map(BudgetConfigRow::into_config))
        })
        .await?
    }

    async fn delete_config(&self, entity_key: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let key = entity_key.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(budget_configs::table.find(&key))
                .execute(&mut *conn)
                .context("delete budget config")?;
            Ok(())
        })
        .await?
    }

    async fn list_all(&self) -> Result<Vec<(String, BudgetConfig)>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<(String, BudgetConfig)>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = budget_configs::table
                .load::<BudgetConfigRow>(&mut *conn)
                .context("list all budget configs")?;
            Ok(rows
                .into_iter()
                .map(|r| {
                    let key = r.entity_key.clone();
                    (key, r.into_config())
                })
                .collect())
        })
        .await?
    }
}
