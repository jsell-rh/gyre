//! SQLite adapter for the `JobRunRepository` port (TASK-273,
//! admin-panel.md §Background Jobs: durable job-run history with logs).

use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_ports::{JobRunRecord, JobRunRepository};
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::job_runs;

#[derive(Queryable, Selectable)]
#[diesel(table_name = job_runs)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
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
impl JobRunRepository for SqliteStorage {
    async fn record(&self, run: &JobRunRecord) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let run = run.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            // Re-recording the same id replaces the row (the scheduler
            // records a run once at completion; a crash-restart re-record
            // must not duplicate it).
            diesel::replace_into(job_runs::table)
                .values(&to_row(&run))
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

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_ports::JobRunRepository as _;
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    fn run(
        id: &str,
        job: &str,
        started: u64,
        finished: Option<u64>,
        logs: Vec<&str>,
    ) -> JobRunRecord {
        JobRunRecord {
            id: id.to_string(),
            job_name: job.to_string(),
            started_at: started,
            finished_at: finished,
            status: match finished {
                Some(_) => "success".to_string(),
                None => "running".to_string(),
            },
            error: None,
            logs: logs.into_iter().map(|l| l.to_string()).collect(),
        }
    }

    #[tokio::test]
    async fn record_list_by_job_newest_first_with_logs_and_limit() {
        let (_tmp, s) = setup();
        s.record(&run(
            "r1",
            "repo_maintenance",
            100,
            Some(150),
            vec!["repo a: ok"],
        ))
        .await
        .unwrap();
        s.record(&run(
            "r2",
            "repo_maintenance",
            300,
            Some(350),
            vec!["repo b: ok", "repo c: FAILED"],
        ))
        .await
        .unwrap();
        s.record(&run("r3", "other_job", 200, Some(250), vec![]))
            .await
            .unwrap();

        let runs = s.list_by_job("repo_maintenance", 10).await.unwrap();
        assert_eq!(runs.len(), 2);
        // Newest first.
        assert_eq!(runs[0].id, "r2");
        assert_eq!(runs[1].id, "r1");
        // Per-run log lines round-trip in write order.
        assert_eq!(runs[0].logs, vec!["repo b: ok", "repo c: FAILED"]);
        assert_eq!(runs[1].logs, vec!["repo a: ok"]);

        // Limit is honored.
        let limited = s.list_by_job("repo_maintenance", 1).await.unwrap();
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].id, "r2");
    }

    #[tokio::test]
    async fn list_recent_across_jobs_newest_first() {
        let (_tmp, s) = setup();
        s.record(&run("r1", "job_a", 100, Some(110), vec![]))
            .await
            .unwrap();
        s.record(&run("r2", "job_b", 300, Some(310), vec![]))
            .await
            .unwrap();
        s.record(&run("r3", "job_a", 200, Some(210), vec![]))
            .await
            .unwrap();

        let runs = s.list_recent(10).await.unwrap();
        let ids: Vec<&str> = runs.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["r2", "r3", "r1"]);
    }

    #[tokio::test]
    async fn record_same_id_replaces_row() {
        let (_tmp, s) = setup();
        s.record(&run("r1", "job_a", 100, None, vec![]))
            .await
            .unwrap();
        // The scheduler re-records the run at completion (same id): the row
        // is replaced, not duplicated.
        s.record(&run("r1", "job_a", 100, Some(140), vec!["done"]))
            .await
            .unwrap();

        let runs = s.list_by_job("job_a", 10).await.unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].finished_at, Some(140));
        assert_eq!(runs[0].status, "success");
        assert_eq!(runs[0].logs, vec!["done"]);
    }

    #[tokio::test]
    async fn delete_older_than_removes_only_finished_rows() {
        let (_tmp, s) = setup();
        // Finished before the cutoff → deleted.
        s.record(&run("old-fin", "job_a", 100, Some(150), vec![]))
            .await
            .unwrap();
        // Started before the cutoff but NEVER finished (crashed cycle) →
        // kept: only finished rows are pruned.
        s.record(&run("old-run", "job_a", 100, None, vec![]))
            .await
            .unwrap();
        // Finished after the cutoff → kept.
        s.record(&run("new-fin", "job_a", 500, Some(550), vec![]))
            .await
            .unwrap();

        let deleted = s.delete_older_than(300).await.unwrap();
        assert_eq!(deleted, 1, "only the old finished row is deleted");

        let ids: Vec<String> = s
            .list_recent(10)
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert!(ids.contains(&"old-run".to_string()));
        assert!(ids.contains(&"new-fin".to_string()));
        assert!(!ids.contains(&"old-fin".to_string()));
    }

    #[tokio::test]
    async fn history_survives_storage_rebuild_restart_durability() {
        // The restart probe's adapter half: a NEW SqliteStorage opened on
        // the same database file still serves previously recorded runs
        // with status, derived duration, error and logs.
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        {
            let s = SqliteStorage::new(&path).unwrap();
            let mut failed = run(
                "r-fail",
                "repo_maintenance",
                100,
                Some(190),
                vec!["repo a: ok", "repo b: FAILED"],
            );
            failed.status = "failed".to_string();
            failed.error = Some("1 of 2 repositories failed".to_string());
            s.record(&failed).await.unwrap();
        }
        // "Server restart": rebuild the storage against the same file.
        let s2 = SqliteStorage::new(&path).unwrap();
        let runs = s2.list_by_job("repo_maintenance", 10).await.unwrap();
        assert_eq!(runs.len(), 1);
        let r = &runs[0];
        assert_eq!(r.status, "failed");
        assert_eq!(r.error.as_deref(), Some("1 of 2 repositories failed"));
        assert_eq!(r.duration_secs(), Some(90));
        assert_eq!(r.logs, vec!["repo a: ok", "repo b: FAILED"]);
    }
}
