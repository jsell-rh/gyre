//! PostgreSQL adapter for the `CascadeTestResultRepository` port (task-280).
//!
//! Mirrors the SQLite adapter: same upsert contract (one row per cascade
//! task, `ON CONFLICT (cascade_task_id) DO UPDATE`), same portable schema
//! (migration 000070 runs on both backends).

use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::CascadeTestResult;
use gyre_ports::CascadeTestResultRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::cascade_test_results;

#[derive(Queryable, Selectable)]
#[diesel(table_name = cascade_test_results)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct CascadeTestResultRow {
    id: String,
    dependent_repo_id: String,
    merged_repo_id: String,
    merge_commit_sha: String,
    cascade_task_id: String,
    passed: bool,
    failure_details: Option<String>,
    recorded_at: i64,
}

impl CascadeTestResultRow {
    fn into_record(self) -> CascadeTestResult {
        CascadeTestResult {
            id: Id::new(self.id),
            dependent_repo_id: Id::new(self.dependent_repo_id),
            merged_repo_id: Id::new(self.merged_repo_id),
            merge_commit_sha: self.merge_commit_sha,
            cascade_task_id: Id::new(self.cascade_task_id),
            passed: self.passed,
            failure_details: self.failure_details,
            recorded_at: self.recorded_at.max(0) as u64,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = cascade_test_results)]
struct NewCascadeTestResultRow<'a> {
    id: &'a str,
    dependent_repo_id: &'a str,
    merged_repo_id: &'a str,
    merge_commit_sha: &'a str,
    cascade_task_id: &'a str,
    passed: bool,
    failure_details: Option<&'a str>,
    recorded_at: i64,
}

/// Upsert keyed on `cascade_task_id` (port contract: one result row per
/// cascade test task — re-reporting replaces the row, never duplicates).
/// Shared by the SQLite and PostgreSQL adapters so both enforce the
/// identical replace semantics via `ON CONFLICT (cascade_task_id)
/// DO UPDATE`.
#[async_trait]
impl CascadeTestResultRepository for PgStorage {
    async fn record(&self, result: &CascadeTestResult) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let r = result.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewCascadeTestResultRow {
                id: r.id.as_str(),
                dependent_repo_id: r.dependent_repo_id.as_str(),
                merged_repo_id: r.merged_repo_id.as_str(),
                merge_commit_sha: &r.merge_commit_sha,
                cascade_task_id: r.cascade_task_id.as_str(),
                passed: r.passed,
                failure_details: r.failure_details.as_deref(),
                recorded_at: r.recorded_at as i64,
            };
            diesel::insert_into(cascade_test_results::table)
                .values(&row)
                .on_conflict(cascade_test_results::cascade_task_id)
                .do_update()
                .set((
                    cascade_test_results::dependent_repo_id.eq(row.dependent_repo_id.to_string()),
                    cascade_test_results::merged_repo_id.eq(row.merged_repo_id.to_string()),
                    cascade_test_results::merge_commit_sha.eq(row.merge_commit_sha.to_string()),
                    cascade_test_results::passed.eq(row.passed),
                    cascade_test_results::failure_details
                        .eq(row.failure_details.map(|d| d.to_string())),
                    cascade_test_results::recorded_at.eq(row.recorded_at),
                ))
                .execute(&mut *conn)
                .context("record cascade test result")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_task(&self, task_id: &Id) -> Result<Option<CascadeTestResult>> {
        let pool = Arc::clone(&self.pool);
        let tid = task_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<CascadeTestResult>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = cascade_test_results::table
                .filter(cascade_test_results::cascade_task_id.eq(tid))
                .first::<CascadeTestResultRow>(&mut *conn)
                .optional()
                .context("find cascade test result by task")?;
            Ok(row.map(|r| r.into_record()))
        })
        .await?
    }

    async fn list_by_dependent_repo(&self, repo_id: &Id) -> Result<Vec<CascadeTestResult>> {
        let pool = Arc::clone(&self.pool);
        let rid = repo_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<CascadeTestResult>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = cascade_test_results::table
                .filter(cascade_test_results::dependent_repo_id.eq(rid))
                .order_by(cascade_test_results::recorded_at.desc())
                .load::<CascadeTestResultRow>(&mut *conn)
                .context("list cascade test results by dependent repo")?;
            Ok(rows.into_iter().map(|r| r.into_record()).collect())
        })
        .await?
    }

    async fn list_by_merged_repo(&self, repo_id: &Id) -> Result<Vec<CascadeTestResult>> {
        let pool = Arc::clone(&self.pool);
        let rid = repo_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<CascadeTestResult>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = cascade_test_results::table
                .filter(cascade_test_results::merged_repo_id.eq(rid))
                .order_by(cascade_test_results::recorded_at.desc())
                .load::<CascadeTestResultRow>(&mut *conn)
                .context("list cascade test results by merged repo")?;
            Ok(rows.into_iter().map(|r| r.into_record()).collect())
        })
        .await?
    }
}
