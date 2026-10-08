use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_domain::SpecAssertionResult;
use gyre_ports::SpecAssertionResultRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::spec_assertion_results;

#[derive(Queryable, Selectable)]
#[diesel(table_name = spec_assertion_results)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct SpecAssertionResultRow {
    id: String,
    repo_id: String,
    spec_path: String,
    line: i32,
    assertion_type: String,
    assertion_text: String,
    params_json: String,
    passed: bool,
    explanation: String,
    commit_sha: String,
    checked_at: i64,
}

impl SpecAssertionResultRow {
    fn into_record(self) -> SpecAssertionResult {
        SpecAssertionResult {
            id: self.id,
            repo_id: self.repo_id,
            spec_path: self.spec_path,
            line: self.line.max(0) as usize,
            assertion_type: self.assertion_type,
            assertion_text: self.assertion_text,
            params_json: self.params_json,
            passed: self.passed,
            explanation: self.explanation,
            commit_sha: self.commit_sha,
            checked_at: self.checked_at.max(0) as u64,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = spec_assertion_results)]
struct NewSpecAssertionResultRow<'a> {
    id: &'a str,
    repo_id: &'a str,
    spec_path: &'a str,
    line: i32,
    assertion_type: &'a str,
    assertion_text: &'a str,
    params_json: &'a str,
    passed: bool,
    explanation: &'a str,
    commit_sha: &'a str,
    checked_at: i64,
}

#[async_trait]
impl SpecAssertionResultRepository for PgStorage {
    async fn save_results(&self, results: &[SpecAssertionResult]) -> Result<()> {
        if results.is_empty() {
            return Ok(());
        }
        let pool = Arc::clone(&self.pool);
        let results = results.to_vec();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            conn.transaction::<_, anyhow::Error, _>(|conn| -> Result<()> {
                // Replace semantics: the stored set for (repo_id, spec_path)
                // must always reflect the latest check, including assertions
                // removed from the spec since the last push.
                let repo_id = &results[0].repo_id;
                let spec_path = &results[0].spec_path;
                diesel::delete(
                    spec_assertion_results::table
                        .filter(spec_assertion_results::repo_id.eq(repo_id))
                        .filter(spec_assertion_results::spec_path.eq(spec_path)),
                )
                .execute(conn)
                .context("delete stale spec assertion results")?;

                let rows: Vec<NewSpecAssertionResultRow<'_>> = results
                    .iter()
                    .map(|r| NewSpecAssertionResultRow {
                        id: &r.id,
                        repo_id: &r.repo_id,
                        spec_path: &r.spec_path,
                        line: r.line.min(i32::MAX as usize) as i32,
                        assertion_type: &r.assertion_type,
                        assertion_text: &r.assertion_text,
                        params_json: &r.params_json,
                        passed: r.passed,
                        explanation: &r.explanation,
                        commit_sha: &r.commit_sha,
                        checked_at: r.checked_at.min(i64::MAX as u64) as i64,
                    })
                    .collect();
                diesel::insert_into(spec_assertion_results::table)
                    .values(&rows)
                    .execute(conn)
                    .context("insert spec assertion results")?;
                Ok(())
            })
        })
        .await?
    }

    async fn list_by_spec(&self, repo_id: &str, spec_path: &str) -> Result<Vec<SpecAssertionResult>> {
        let pool = Arc::clone(&self.pool);
        let repo_id = repo_id.to_string();
        let spec_path = spec_path.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<SpecAssertionResult>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = spec_assertion_results::table
                .filter(spec_assertion_results::repo_id.eq(&repo_id))
                .filter(spec_assertion_results::spec_path.eq(&spec_path))
                .order(spec_assertion_results::line.asc())
                .load::<SpecAssertionResultRow>(&mut conn)
                .context("list spec assertion results by spec")?;
            Ok(rows.into_iter().map(SpecAssertionResultRow::into_record).collect())
        })
        .await?
    }

    async fn delete_by_repo(&self, repo_id: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let repo_id = repo_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(
                spec_assertion_results::table.filter(spec_assertion_results::repo_id.eq(&repo_id)),
            )
            .execute(&mut conn)
            .context("delete spec assertion results by repo")?;
            Ok(())
        })
        .await?
    }
}
