use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_domain::{BudgetConfig, BudgetEnforcementAction};
use gyre_ports::BudgetRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::budget_configs;

/// String encoding of `BudgetEnforcementAction` in the `budget_configs`
/// table — the serde snake_case variant names.
pub(crate) fn enforcement_action_to_db(a: &BudgetEnforcementAction) -> &'static str {
    match a {
        BudgetEnforcementAction::Warn => "warn",
        BudgetEnforcementAction::GracefulStop => "graceful_stop",
        BudgetEnforcementAction::HardKill => "hard_kill",
        BudgetEnforcementAction::QueuePause => "queue_pause",
    }
}

pub(crate) fn enforcement_action_from_db(s: &str) -> Option<BudgetEnforcementAction> {
    match s {
        "warn" => Some(BudgetEnforcementAction::Warn),
        "graceful_stop" => Some(BudgetEnforcementAction::GracefulStop),
        "hard_kill" => Some(BudgetEnforcementAction::HardKill),
        "queue_pause" => Some(BudgetEnforcementAction::QueuePause),
        _ => None,
    }
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = budget_configs)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct BudgetConfigRow {
    entity_key: String,
    max_tokens_per_day: Option<i64>,
    max_cost_per_day: Option<f64>,
    max_concurrent_agents: Option<i32>,
    max_agent_lifetime_secs: Option<i64>,
    enforcement_action: Option<String>,
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
            enforcement_action: self
                .enforcement_action
                .as_deref()
                .and_then(enforcement_action_from_db),
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
    enforcement_action: Option<&'a str>,
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
                enforcement_action: c.enforcement_action.as_ref().map(enforcement_action_to_db),
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
                    budget_configs::enforcement_action.eq(row.enforcement_action),
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

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_domain::BudgetEnforcementAction;
    use tempfile::NamedTempFile;

    fn tmp_storage() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let storage = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, storage)
    }

    #[tokio::test]
    async fn enforcement_action_round_trips_through_sqlite() {
        // task-191: the per-level enforcement override must survive a
        // set/get cycle (migration column + row mapping) — including the
        // None default and an overwrite back to None.
        let (_tmp, storage) = tmp_storage();
        let key = "workspace:ws-enf-sqlite";

        let config = BudgetConfig {
            max_tokens_per_day: Some(1000),
            enforcement_action: Some(BudgetEnforcementAction::HardKill),
            ..Default::default()
        };
        storage.set_config(key, &config).await.unwrap();
        let back = storage.get_config(key).await.unwrap().unwrap();
        assert_eq!(
            back.enforcement_action,
            Some(BudgetEnforcementAction::HardKill)
        );
        assert_eq!(back.max_tokens_per_day, Some(1000));

        // Overwrite with the default (None) — upsert must clear the column.
        let cleared = BudgetConfig {
            max_tokens_per_day: Some(2000),
            ..Default::default()
        };
        storage.set_config(key, &cleared).await.unwrap();
        let back = storage.get_config(key).await.unwrap().unwrap();
        assert_eq!(back.enforcement_action, None);
        assert_eq!(back.max_tokens_per_day, Some(2000));

        // Every variant survives the TEXT encoding.
        for action in [
            BudgetEnforcementAction::Warn,
            BudgetEnforcementAction::GracefulStop,
            BudgetEnforcementAction::HardKill,
            BudgetEnforcementAction::QueuePause,
        ] {
            let cfg = BudgetConfig {
                enforcement_action: Some(action),
                ..Default::default()
            };
            storage.set_config(key, &cfg).await.unwrap();
            let back = storage.get_config(key).await.unwrap().unwrap();
            assert_eq!(back.enforcement_action, Some(action));
        }
    }
}
