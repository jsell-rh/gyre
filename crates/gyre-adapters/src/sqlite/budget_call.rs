use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::BudgetCallRecord;
use gyre_ports::BudgetCallRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::budget_call_records;

#[derive(Queryable, Selectable)]
#[diesel(table_name = budget_call_records)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
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
impl BudgetCallRepository for SqliteStorage {
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
        tokio::task::spawn_blocking(move || -> Result<Vec<BudgetCallRecord>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = budget_call_records::table
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    fn record(id: &str, ws: &str, ts: u64) -> BudgetCallRecord {
        BudgetCallRecord {
            id: Id::new(id),
            tenant_id: Id::new("tenant-1"),
            workspace_id: Id::new(ws),
            repo_id: Some(Id::new("repo-1")),
            agent_id: None,
            task_id: None,
            usage_type: "llm_query".into(),
            input_tokens: 120,
            output_tokens: 240,
            cost_usd: 0.0042,
            model: "test-model".into(),
            timestamp: ts,
        }
    }

    #[tokio::test]
    async fn save_then_list_round_trips_every_field() {
        let (_tmp, s) = setup();
        s.save(&record("call-1", "ws-1", 1_000)).await.unwrap();

        let got = s.list_by_workspace("ws-1", 0, 10).await.unwrap();
        assert_eq!(got.len(), 1);
        let r = &got[0];
        assert_eq!(r.id.as_str(), "call-1");
        assert_eq!(r.tenant_id.as_str(), "tenant-1");
        assert_eq!(r.workspace_id.as_str(), "ws-1");
        assert_eq!(r.repo_id.as_ref().unwrap().as_str(), "repo-1");
        assert!(r.agent_id.is_none());
        assert_eq!(r.usage_type, "llm_query");
        assert_eq!(r.input_tokens, 120);
        assert_eq!(r.output_tokens, 240);
        assert!((r.cost_usd - 0.0042).abs() < f64::EPSILON);
        assert_eq!(r.model, "test-model");
        assert_eq!(r.timestamp, 1_000);
    }

    #[tokio::test]
    async fn list_filters_by_workspace_since_and_limit_newest_first() {
        let (_tmp, s) = setup();
        for (id, ts) in [("a", 10), ("b", 20), ("c", 30)] {
            s.save(&record(id, "ws-1", ts)).await.unwrap();
        }
        s.save(&record("other", "ws-2", 40)).await.unwrap();

        let all = s.list_by_workspace("ws-1", 0, 10).await.unwrap();
        assert_eq!(
            all.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["c", "b", "a"],
            "must be scoped to ws-1 and newest-first"
        );

        let since = s.list_by_workspace("ws-1", 20, 10).await.unwrap();
        assert_eq!(since.len(), 2);
        let limited = s.list_by_workspace("ws-1", 0, 1).await.unwrap();
        assert_eq!(limited.len(), 1);
    }

    #[tokio::test]
    async fn duplicate_id_is_rejected() {
        let (_tmp, s) = setup();
        s.save(&record("dup", "ws-1", 10)).await.unwrap();
        assert!(s.save(&record("dup", "ws-1", 20)).await.is_err());
    }
}
