use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// One recorded run of a background job (admin-panel.md §Background Jobs:
/// "Full job history stored and visible in admin UI — status, duration,
/// failures, logs").
///
/// `logs` carries the per-run captured log lines (per-repo maintenance
/// outcomes, per-secret refresh outcomes, …) written through the
/// run-scoped sink job handlers receive. Storage is durable: the mem
/// adapter keeps it process-local, the SQL adapters persist it in the
/// `job_runs` table so a server restart does not orphan the history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRunRecord {
    /// Unique run id (UUID).
    pub id: String,
    /// Job name this run belongs to (registry key).
    pub job_name: String,
    /// Unix epoch seconds when the run started.
    pub started_at: u64,
    /// Unix epoch seconds when the run finished; `None` while running.
    pub finished_at: Option<u64>,
    /// "success" | "failed" | "running".
    pub status: String,
    /// Error message when the run failed.
    pub error: Option<String>,
    /// Per-run captured log lines, in write order.
    pub logs: Vec<String>,
}

impl JobRunRecord {
    /// Derived run duration in seconds (`None` while the run is unfinished).
    pub fn duration_secs(&self) -> Option<u64> {
        self.finished_at.map(|f| f.saturating_sub(self.started_at))
    }
}

/// Durable job-run history (admin-panel.md: "Full job history stored and
/// visible"). The in-memory registry history is a fronting cache at most —
/// this repository is the authoritative store.
#[async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
pub trait JobRunRepository: Send + Sync {
    /// Persist one run record. New rows are appended; re-recording the same
    /// id replaces the row (the scheduler records a run once at completion).
    async fn record(&self, run: &JobRunRecord) -> Result<()>;

    /// Most recent runs for one job, newest-first, at most `limit` rows.
    async fn list_by_job(&self, job_name: &str, limit: usize) -> Result<Vec<JobRunRecord>>;

    /// Most recent runs across all jobs, newest-first, at most `limit` rows.
    async fn list_recent(&self, limit: usize) -> Result<Vec<JobRunRecord>>;

    /// Delete runs that FINISHED before `cutoff_secs` (running/unfinished
    /// rows are never deleted). Returns the number of rows removed. Used by
    /// the self-managing `job_history_cleanup` job.
    async fn delete_older_than(&self, cutoff_secs: u64) -> Result<u64>;
}
