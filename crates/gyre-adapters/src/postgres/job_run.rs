//! SQLite adapter for the `JobRunRepository` port (TASK-273,
//! admin-panel.md §Background Jobs: durable job-run history with logs).

use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_ports::{JobRunRecord, JobRunRepository};
use std::sync::Arc;

use super::PgStorage;
use crate::schema::job_runs;

#[derive(Queryable, Selectable)]
#[diesel(table_name = job_runs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct JobRunRow {
    id: String,
    job_name: String,
    started_at: i64,
    finished_at: Option<i64>,
    status: String,
    error: Option<String>,
    /// JSON array of log lines.
    logs: String,
}

impl JobRunRow {
    fn into_record(self) -> Result<JobRunRecord> {
        let logs = serde_json::from_str(&self.logs)
            .with_context(|| format!("job run {} logs are corrupt", self.id))?;
        Ok(JobRunRecord {
            id: self.id,
            job_name: self.job_name,
            started_at: self.started_at as u64,
            finished_at: self.finished_at.map(|t| t as u64),
            status: self.status,
            error: self.error,
            logs,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = job_runs)]
struct NewJobRunRow<'a> {
    id: &'a str,
    job_name: &'a str,
    started_at: i64,
    finished_at: Option<i64>,
    status: &'a str,
    error: Option<&'a str>,
    logs: String,
}

fn to_row(run: &JobRunRecord) -> NewJobRunRow<'_> {
    // Logs are stored as a JSON array of strings; serialization of a
    // Vec<String> cannot fail.
    let logs = serde_json::to_string(&run.logs).unwrap_or_else(|_| "[]".to_string());
    NewJobRunRow {
        id: &run.id,
        job_name: &run.job_name,
        started_at: run.started_at as i64,
        finished_at: run.finished_at.map(|t| t as i64),
        status: &run.status,
        error: run.error.as_deref(),
        logs,
    }
}

#[async_trait]
impl JobRunRepository for PgStorage {
    async fn record(&self, run: &JobRunRecord) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let run = run.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            // Re-recording the same id replaces the row (the scheduler
            // records a run once at completion; a crash-restart re-record
            // must not duplicate it). PostgreSQL has no REPLACE INTO — the
            // portable form is ON CONFLICT DO UPDATE.
            diesel::insert_into(job_runs::table)
                .values(&to_row(&run))
                .on_conflict(job_runs::id)
                .do_update()
                .set((
                    job_runs::job_name.eq(run.job_name.clone()),
                    job_runs::started_at.eq(run.started_at as i64),
                    job_runs::finished_at.eq(run.finished_at.map(|t| t as i64)),
                    job_runs::status.eq(run.status.clone()),
                    job_runs::error.eq(run.error.clone()),
                    job_runs::logs
                        .eq(serde_json::to_string(&run.logs).unwrap_or_else(|_| "[]".to_string())),
                ))
                .execute(&mut conn)
                .context("insert job_run")?;
            Ok(())
        })
        .await?
    }

    async fn list_by_job(&self, job_name: &str, limit: usize) -> Result<Vec<JobRunRecord>> {
        let pool = Arc::clone(&self.pool);
        let job_name = job_name.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<JobRunRecord>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = job_runs::table
                .filter(job_runs::job_name.eq(job_name))
                .order(job_runs::started_at.desc())
                .limit(limit as i64)
                .select(JobRunRow::as_select())
                .load(&mut conn)
                .context("list job_runs by job")?;
            rows.into_iter().map(JobRunRow::into_record).collect()
        })
        .await?
    }

    async fn list_recent(&self, limit: usize) -> Result<Vec<JobRunRecord>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<JobRunRecord>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = job_runs::table
                .order(job_runs::started_at.desc())
                .limit(limit as i64)
                .select(JobRunRow::as_select())
                .load(&mut conn)
                .context("list recent job_runs")?;
            rows.into_iter().map(JobRunRow::into_record).collect()
        })
        .await?
    }

    async fn delete_older_than(&self, cutoff_secs: u64) -> Result<u64> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            // Only FINISHED runs are pruned: an unfinished row belongs to a
            // live run (or a crashed one whose status is still "running" —
            // it stays visible for diagnosis until it finishes or ages out
            // from its started_at via the retention window on finished rows
            // written by a later run).
            let n = diesel::delete(job_runs::table)
                .filter(job_runs::finished_at.lt(cutoff_secs as i64))
                .execute(&mut conn)
                .context("delete old job_runs")?;
            Ok(n as u64)
        })
        .await?
    }
}
