use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::BudgetCallRecord;
use gyre_ports::BudgetCallRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::budget_call_records;

#[derive(Queryable, Selectable)]
#[diesel(table_name = budget_call_records)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct BudgetCallRow {
    id: String,
    tenant_id: String,
    workspace_id: String,
    repo_id: Option<String>,
    agent_id: Option<String>,
    task_id: Option<String>,
    usage_type: String,
    input_tokens: i64,
    output_tokens: i64,
    cost_usd: f64,
    model: String,
    timestamp: i64,
}

impl From<BudgetCallRow> for BudgetCallRecord {
    fn from(r: BudgetCallRow) -> Self {
        BudgetCallRecord {
            id: Id::new(r.id),
            tenant_id: Id::new(r.tenant_id),
            workspace_id: Id::new(r.workspace_id),
            repo_id: r.repo_id.map(Id::new),
            agent_id: r.agent_id.map(Id::new),
            task_id: r.task_id.map(Id::new),
            usage_type: r.usage_type,
            input_tokens: r.input_tokens.max(0) as u64,
            output_tokens: r.output_tokens.max(0) as u64,
            cost_usd: r.cost_usd,
            model: r.model,
            timestamp: r.timestamp.max(0) as u64,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = budget_call_records)]
struct NewBudgetCallRow<'a> {
    id: &'a str,
    tenant_id: &'a str,
    workspace_id: &'a str,
    repo_id: Option<&'a str>,
    agent_id: Option<&'a str>,
    task_id: Option<&'a str>,
    usage_type: &'a str,
    input_tokens: i64,
    output_tokens: i64,
    cost_usd: f64,
    model: &'a str,
    timestamp: i64,
}

#[async_trait]
impl BudgetCallRepository for PgStorage {
    async fn save(&self, record: &BudgetCallRecord) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let rec = record.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewBudgetCallRow {
                id: rec.id.as_str(),
                tenant_id: rec.tenant_id.as_str(),
                workspace_id: rec.workspace_id.as_str(),
                repo_id: rec.repo_id.as_ref().map(|i| i.as_str()),
                agent_id: rec.agent_id.as_ref().map(|i| i.as_str()),
                task_id: rec.task_id.as_ref().map(|i| i.as_str()),
                usage_type: &rec.usage_type,
                input_tokens: rec.input_tokens as i64,
                output_tokens: rec.output_tokens as i64,
                cost_usd: rec.cost_usd,
                model: &rec.model,
                timestamp: rec.timestamp as i64,
            };
            diesel::insert_into(budget_call_records::table)
                .values(&row)
                .execute(&mut *conn)
                .context("insert budget call record")?;
            Ok(())
        })
        .await?
    }

    async fn list_by_workspace(
        &self,
        workspace_id: &str,
        since: u64,
        limit: i64,
    ) -> Result<Vec<BudgetCallRecord>> {
        let pool = Arc::clone(&self.pool);
        let ws = workspace_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<BudgetCallRecord>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = budget_call_records::table
                .filter(budget_call_records::workspace_id.eq(&ws))
                .filter(budget_call_records::timestamp.ge(since as i64))
                .order_by(budget_call_records::timestamp.desc())
                .then_order_by(budget_call_records::id.desc())
                .limit(limit)
                .load::<BudgetCallRow>(&mut *conn)
                .context("list budget call records")?;
            Ok(rows.into_iter().map(BudgetCallRecord::from).collect())
        })
        .await?
    }
}
