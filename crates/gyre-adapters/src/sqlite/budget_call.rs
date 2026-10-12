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
    prompt_template_sha: Option<String>,
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
            prompt_template_sha: r.prompt_template_sha,
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
    prompt_template_sha: Option<&'a str>,
    timestamp: i64,
}

#[async_trait]
impl BudgetCallRepository for SqliteStorage {
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
                prompt_template_sha: rec.prompt_template_sha.as_deref(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_storage() -> SqliteStorage {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "gyre-budget-call-test-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let storage = SqliteStorage::new(path.to_str().unwrap()).expect("create test storage");
        // Keep the file for the storage lifetime; remove when dropped is not
        // implemented by SqliteStorage, so unlink up front (SQLite keeps the
        // open handle usable on Unix).
        let _ = std::fs::remove_file(&path);
        storage
    }

    fn record(ws: &str, usage_type: &str, ts: u64) -> BudgetCallRecord {
        BudgetCallRecord {
            id: Id::new(format!("bcr-{ws}-{ts}")),
            tenant_id: Id::new("tenant-1"),
            workspace_id: Id::new(ws),
            repo_id: Some(Id::new("repo-1")),
            agent_id: None,
            task_id: Some(Id::new("task-1")),
            usage_type: usage_type.to_string(),
            input_tokens: 100,
            output_tokens: 40,
            cost_usd: 0.002,
            model: "test-model".to_string(),
            prompt_template_sha: Some("a".repeat(40)),
            timestamp: ts,
        }
    }

    #[tokio::test]
    async fn save_persists_and_list_reads_back() {
        let st = test_storage();
        st.save(&record("ws-1", "llm_query", 1000)).await.unwrap();
        st.save(&record("ws-1", "agent_run", 2000)).await.unwrap();
        st.save(&record("ws-2", "llm_query", 1500)).await.unwrap();

        let all = st.list_by_workspace("ws-1", 0, 10).await.unwrap();
        assert_eq!(all.len(), 2);
        // Newest first.
        assert_eq!(all[0].input_tokens, 100);
        assert_eq!(all[0].output_tokens, 40);
        assert_eq!(all[0].repo_id.as_ref().map(|i| i.as_str()), Some("repo-1"));
        // Prompt-template provenance round-trips (ui-layout.md §2).
        assert_eq!(all[0].prompt_template_sha.as_deref(), Some(&"a".repeat(40)[..]));
        assert_eq!(all[1].usage_type, "llm_query");

        // Since filter excludes older rows.
        let recent = st.list_by_workspace("ws-1", 1500, 10).await.unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].usage_type, "agent_run");

        // Limit applies.
        let limited = st.list_by_workspace("ws-1", 0, 1).await.unwrap();
        assert_eq!(limited.len(), 1);

        // Other workspace unaffected.
        assert_eq!(st.list_by_workspace("ws-2", 0, 10).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn save_rejects_duplicate_id() {
        let st = test_storage();
        st.save(&record("ws-dup", "llm_query", 1)).await.unwrap();
        assert!(st.save(&record("ws-dup", "llm_query", 1)).await.is_err());
    }
}
