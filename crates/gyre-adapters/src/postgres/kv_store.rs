use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_domain::BudgetUsage;
use gyre_ports::{BudgetUsageRepository, KvJsonStore};
use std::sync::Arc;

use super::PgStorage;
use crate::schema::{budget_usages, kv_store};

// ── KV store rows ─────────────────────────────────────────────────────────────

#[derive(Queryable, Selectable)]
#[diesel(table_name = kv_store)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct KvRow {
    #[allow(dead_code)]
    namespace: String,
    key: String,
    value_json: String,
    #[allow(dead_code)]
    updated_at: i64,
}

#[derive(Insertable)]
#[diesel(table_name = kv_store)]
struct NewKvRow<'a> {
    namespace: &'a str,
    key: &'a str,
    value_json: &'a str,
    updated_at: i64,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[async_trait]
impl KvJsonStore for PgStorage {
    async fn kv_set(&self, namespace: &str, key: &str, value: String) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let ns = namespace.to_string();
        let k = key.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let now = now_secs();
            let row = NewKvRow {
                namespace: &ns,
                key: &k,
                value_json: &value,
                updated_at: now,
            };
            diesel::insert_into(kv_store::table)
                .values(&row)
                .on_conflict((kv_store::namespace, kv_store::key))
                .do_update()
                .set((
                    kv_store::value_json.eq(&value),
                    kv_store::updated_at.eq(now),
                ))
                .execute(&mut *conn)
                .context("upsert kv_store")?;
            Ok(())
        })
        .await?
    }

    async fn kv_get(&self, namespace: &str, key: &str) -> Result<Option<String>> {
        let pool = Arc::clone(&self.pool);
        let ns = namespace.to_string();
        let k = key.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<String>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = kv_store::table
                .filter(kv_store::namespace.eq(&ns))
                .filter(kv_store::key.eq(&k))
                .first::<KvRow>(&mut *conn)
                .optional()
                .context("get kv_store")?;
            Ok(result.map(|r| r.value_json))
        })
        .await?
    }

    async fn kv_remove(&self, namespace: &str, key: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let ns = namespace.to_string();
        let k = key.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(
                kv_store::table
                    .filter(kv_store::namespace.eq(&ns))
                    .filter(kv_store::key.eq(&k)),
            )
            .execute(&mut *conn)
            .context("delete kv_store")?;
            Ok(())
        })
        .await?
    }

    async fn kv_list(&self, namespace: &str) -> Result<Vec<(String, String)>> {
        let pool = Arc::clone(&self.pool);
        let ns = namespace.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<(String, String)>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = kv_store::table
                .filter(kv_store::namespace.eq(&ns))
                .load::<KvRow>(&mut *conn)
                .context("list kv_store")?;
            Ok(rows.into_iter().map(|r| (r.key, r.value_json)).collect())
        })
        .await?
    }

    async fn kv_clear(&self, namespace: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let ns = namespace.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(kv_store::table.filter(kv_store::namespace.eq(&ns)))
                .execute(&mut *conn)
                .context("clear kv_store namespace")?;
            Ok(())
        })
        .await?
    }
}

// ── BudgetUsage rows ──────────────────────────────────────────────────────────

#[derive(Queryable, Selectable)]
#[diesel(table_name = budget_usages)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct BudgetUsageRow {
    entity_key: String,
    entity_type: String,
    entity_id: String,
    tokens_used_today: i64,
    cost_today: f64,
    active_agents: i32,
    period_start: i64,
    #[allow(dead_code)]
    updated_at: i64,
}

impl BudgetUsageRow {
    fn into_usage(self) -> BudgetUsage {
        BudgetUsage {
            entity_type: self.entity_type,
            entity_id: gyre_common::Id::new(self.entity_id),
            tokens_used_today: self.tokens_used_today as u64,
            cost_today: self.cost_today,
            active_agents: self.active_agents as u32,
            period_start: self.period_start as u64,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = budget_usages)]
struct NewBudgetUsageRow<'a> {
    entity_key: &'a str,
    entity_type: &'a str,
    entity_id: &'a str,
    tokens_used_today: i64,
    cost_today: f64,
    active_agents: i32,
    period_start: i64,
    updated_at: i64,
}

#[async_trait]
impl BudgetUsageRepository for PgStorage {
    async fn set_usage(&self, entity_key: &str, usage: &BudgetUsage) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let key = entity_key.to_string();
        let u = usage.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let now = now_secs();
            let entity_id = u.entity_id.to_string();
            let row = NewBudgetUsageRow {
                entity_key: &key,
                entity_type: &u.entity_type,
                entity_id: &entity_id,
                tokens_used_today: u.tokens_used_today as i64,
                cost_today: u.cost_today,
                active_agents: u.active_agents as i32,
                period_start: u.period_start as i64,
                updated_at: now,
            };
            diesel::insert_into(budget_usages::table)
                .values(&row)
                .on_conflict(budget_usages::entity_key)
                .do_update()
                .set((
                    budget_usages::entity_type.eq(&u.entity_type),
                    budget_usages::entity_id.eq(&entity_id),
                    budget_usages::tokens_used_today.eq(u.tokens_used_today as i64),
                    budget_usages::cost_today.eq(u.cost_today),
                    budget_usages::active_agents.eq(u.active_agents as i32),
                    budget_usages::period_start.eq(u.period_start as i64),
                    budget_usages::updated_at.eq(now),
                ))
                .execute(&mut *conn)
                .context("upsert budget_usage")?;
            Ok(())
        })
        .await?
    }

    async fn get_usage(&self, entity_key: &str) -> Result<Option<BudgetUsage>> {
        let pool = Arc::clone(&self.pool);
        let key = entity_key.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<BudgetUsage>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = budget_usages::table
                .find(&key)
                .first::<BudgetUsageRow>(&mut *conn)
                .optional()
                .context("get budget_usage")?;
            Ok(result.map(BudgetUsageRow::into_usage))
        })
        .await?
    }

    async fn delete_usage(&self, entity_key: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let key = entity_key.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(budget_usages::table.find(&key))
                .execute(&mut *conn)
                .context("delete budget_usage")?;
            Ok(())
        })
        .await?
    }

    async fn list_all_usage(&self) -> Result<Vec<(String, BudgetUsage)>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<(String, BudgetUsage)>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = budget_usages::table
                .load::<BudgetUsageRow>(&mut *conn)
                .context("list budget_usages")?;
            Ok(rows
                .into_iter()
                .map(|r| {
                    let key = r.entity_key.clone();
                    (key, r.into_usage())
                })
                .collect())
        })
        .await?
    }

    async fn increment_active(
        &self,
        entity_key: &str,
        entity_type: &str,
        entity_id: &str,
        now: u64,
    ) -> Result<BudgetUsage> {
        let pool = Arc::clone(&self.pool);
        let key = entity_key.to_string();
        let etype = entity_type.to_string();
        let eid = entity_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<BudgetUsage> {
            let mut conn = pool.get().context("get db connection")?;
            let ts = now_secs();
            // Insert-or-ignore, then atomic increment.
            let row = NewBudgetUsageRow {
                entity_key: &key,
                entity_type: &etype,
                entity_id: &eid,
                tokens_used_today: 0,
                cost_today: 0.0,
                active_agents: 0,
                period_start: now as i64,
                updated_at: ts,
            };
            diesel::insert_into(budget_usages::table)
                .values(&row)
                .on_conflict(budget_usages::entity_key)
                .do_nothing()
                .execute(&mut *conn)
                .context("ensure budget_usage row")?;
            diesel::update(budget_usages::table.find(&key))
                .set((
                    budget_usages::active_agents
                        .eq(budget_usages::active_agents + 1),
                    budget_usages::updated_at.eq(ts),
                ))
                .execute(&mut *conn)
                .context("increment active_agents")?;
            let updated = budget_usages::table
                .find(&key)
                .first::<BudgetUsageRow>(&mut *conn)
                .context("fetch updated budget_usage")?;
            Ok(updated.into_usage())
        })
        .await?
    }

    async fn decrement_active(&self, entity_key: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let key = entity_key.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let ts = now_secs();
            diesel::update(budget_usages::table.find(&key))
                .set((
                    budget_usages::active_agents.eq(
                        diesel::dsl::sql("GREATEST(0, active_agents - 1)"),
                    ),
                    budget_usages::updated_at.eq(ts),
                ))
                .execute(&mut *conn)
                .context("decrement active_agents")?;
            Ok(())
        })
        .await?
    }

    async fn add_tokens_cost(
        &self,
        entity_key: &str,
        entity_type: &str,
        entity_id: &str,
        now: u64,
        tokens: u64,
        cost_usd: f64,
    ) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let key = entity_key.to_string();
        let etype = entity_type.to_string();
        let eid = entity_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let ts = now_secs();
            let row = NewBudgetUsageRow {
                entity_key: &key,
                entity_type: &etype,
                entity_id: &eid,
                tokens_used_today: 0,
                cost_today: 0.0,
                active_agents: 0,
                period_start: now as i64,
                updated_at: ts,
            };
            diesel::insert_into(budget_usages::table)
                .values(&row)
                .on_conflict(budget_usages::entity_key)
                .do_nothing()
                .execute(&mut *conn)
                .context("ensure budget_usage row")?;
            diesel::update(budget_usages::table.find(&key))
                .set((
                    budget_usages::tokens_used_today
                        .eq(budget_usages::tokens_used_today + tokens as i64),
                    budget_usages::cost_today
                        .eq(budget_usages::cost_today + cost_usd),
                    budget_usages::updated_at.eq(ts),
                ))
                .execute(&mut *conn)
                .context("add tokens/cost")?;
            Ok(())
        })
        .await?
    }

    async fn reset_daily_counters(&self, now: u64) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let ts = now_secs();
            diesel::update(budget_usages::table)
                .set((
                    budget_usages::tokens_used_today.eq(0i64),
                    budget_usages::cost_today.eq(0.0f64),
                    budget_usages::period_start.eq(now as i64),
                    budget_usages::updated_at.eq(ts),
                ))
                .execute(&mut *conn)
                .context("reset daily counters")?;
            Ok(())
        })
        .await?
    }
}
