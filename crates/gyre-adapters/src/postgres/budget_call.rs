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
            input_tokens: r.input_tokens as u64,
            output_tokens: r.output_tokens as u64,
            cost_usd: r.cost_usd,
            model: r.model,
            timestamp: r.timestamp as u64,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = budget_call_records)]
struct BudgetCallNew<'a> {
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
        let r = record.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = BudgetCallNew {
                id: r.id.as_str(),
                tenant_id: r.tenant_id.as_str(),
                workspace_id: r.workspace_id.as_str(),
                repo_id: r.repo_id.as_ref().map(|id| id.as_str()),
                agent_id: r.agent_id.as_ref().map(|id| id.as_str()),
                task_id: r.task_id.as_ref().map(|id| id.as_str()),
                usage_type: &r.usage_type,
                input_tokens: r.input_tokens as i64,
                output_tokens: r.output_tokens as i64,
                cost_usd: r.cost_usd,
                model: &r.model,
                timestamp: r.timestamp as i64,
            };
            diesel::insert_into(budget_call_records::table)
                .values(&row)
                .execute(&mut *conn)
                .context("insert budget_call_record")?;
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
        let tenant_id = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<BudgetCallRecord>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = budget_call_records::table
                .filter(budget_call_records::tenant_id.eq(&tenant_id))
                .filter(budget_call_records::workspace_id.eq(&ws))
                .filter(budget_call_records::timestamp.ge(since as i64))
                .order(budget_call_records::timestamp.desc())
                .limit(limit)
                .load::<BudgetCallRow>(&mut *conn)
                .context("list budget_call_records by workspace")?;
            Ok(rows.into_iter().map(BudgetCallRecord::from).collect())
        })
        .await?
    }
}
