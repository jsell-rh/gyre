//! Job framework for gyre-server.
//!
//! Provides a registry for background jobs, a scheduler that runs them on fixed intervals,
//! and a run-history store for the last N executions per job.
//!
//! TASK-273 (admin-panel.md §Background Jobs): run history is DURABLE —
//! every recorded run is persisted through the `JobRunRepository` port
//! (SQLite/PostgreSQL adapters; mem in pure-mem mode) with per-run captured
//! log lines, and the in-memory ring is only a fronting cache. Handlers
//! receive a run-scoped [`JobRunLog`] sink to write per-item outcomes
//! (per-repo maintenance results, per-secret refresh outcomes, …) that land
//! in both the durable record and the admin Jobs tab.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::AppState;

const MAX_HISTORY_PER_JOB: usize = 50;

/// Default retention for the self-managing `job_history_cleanup` job
/// (days), overridable via `GYRE_JOB_HISTORY_RETENTION_DAYS`.
pub const JOB_HISTORY_RETENTION_DAYS_DEFAULT: u64 = 30;

// ── Data structures ───────────────────────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize)]
pub struct JobDefinition {
    pub name: String,
    pub description: String,
    pub interval_secs: u64,
    pub enabled: bool,
    /// When set, the scheduler runs this job once per day at this UTC hour
    /// (wall-clock) instead of on a fixed `interval_secs` interval from
    /// process start. Used by the nightly retention job (02:00 UTC,
    /// business-continuity.md §5).
    #[serde(default)]
    pub run_at_utc_hour: Option<u8>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct JobRun {
    pub id: String,
    pub job_name: String,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    /// Per-run captured log lines (run-scoped sink), in write order
    /// (admin-panel.md: job history includes logs).
    #[serde(default)]
    pub logs: Vec<String>,
    pub status: String,
    pub error: Option<String>,
}

impl JobRun {
    /// Derived run duration in seconds (`None` while the run is unfinished).
    pub fn duration_secs(&self) -> Option<u64> {
        self.finished_at.map(|f| f.saturating_sub(self.started_at))
    }

    fn from_record(record: gyre_ports::JobRunRecord) -> Self {
        Self {
            id: record.id,
            job_name: record.job_name,
            started_at: record.started_at,
            finished_at: record.finished_at,
            logs: record.logs,
            status: record.status,
            error: record.error,
        }
    }

    fn to_record(&self) -> gyre_ports::JobRunRecord {
        gyre_ports::JobRunRecord {
            id: self.id.clone(),
            job_name: self.job_name.clone(),
            started_at: self.started_at,
            finished_at: self.finished_at,
            status: self.status.clone(),
            error: self.error.clone(),
            logs: self.logs.clone(),
        }
    }
}

/// Run-scoped log sink handed to job handlers (admin-panel.md: job history
/// includes logs). Lines written here are attached to the run's record and
/// persisted with it. Cloneable and cheap to pass by value.
#[derive(Clone, Default)]
pub struct JobRunLog {
    lines: Arc<Mutex<Vec<String>>>,
}

impl JobRunLog {
    /// A fresh empty sink (used by the scheduler and tests).
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one log line to the run's record.
    pub async fn log(&self, line: impl std::fmt::Display) {
        self.lines.lock().await.push(line.to_string());
    }

    async fn take(&self) -> Vec<String> {
        self.lines.lock().await.clone()
    }
}

/// Liveness verdict for a background job (business-continuity.md §2:
/// Kubernetes liveness probes must fail while a job loop is dead).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobLiveness {
    /// Last run started within the liveness window.
    Ok,
    /// Scheduled, but its first run is not yet due since process start.
    Pending,
    /// Most recent run failed, even if it finished recently.
    Failed,
    /// Scheduled, but its last run is older than the liveness window.
    Stale,
    /// Not registered, or no scheduler loop is running it.
    NotScheduled,
}

impl JobLiveness {
    pub fn as_str(self) -> &'static str {
        match self {
            JobLiveness::Ok => "ok",
            JobLiveness::Pending => "pending",
            JobLiveness::Failed => "failed",
            JobLiveness::Stale => "stale",
            JobLiveness::NotScheduled => "not_scheduled",
        }
    }

    /// A job passes its liveness check while it is running or still awaiting
    /// its first due time since process start.
    pub fn is_live(self) -> bool {
        matches!(self, JobLiveness::Ok | JobLiveness::Pending)
    }
}

// ── Registry ──────────────────────────────────────────────────────────────────

type JobHandler = Arc<
    dyn Fn(Arc<AppState>, JobRunLog) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>
        + Send
        + Sync,
>;

struct RegisteredJob {
    def: JobDefinition,
    handler: JobHandler,
}

/// Central job registry — shared between the scheduler and API handlers.
#[derive(Clone, Default)]
pub struct JobRegistry {
    inner: Arc<Mutex<RegistryInner>>,
    /// Durable job-run store (TASK-273): when set (via
    /// [`JobRegistry::set_durable_store`], called from
    /// `start_job_registry`), every recorded run is persisted here and the
    /// in-memory ring is only a fronting cache.
    durable: Arc<Mutex<Option<Arc<dyn gyre_ports::JobRunRepository>>>>,
}

#[derive(Default)]
struct RegistryInner {
    jobs: HashMap<String, RegisteredJob>,
    /// Recent run history per job name.
    history: HashMap<String, VecDeque<JobRun>>,
    /// Jobs with a running scheduler loop (set by `spawn_job` / module loops).
    scheduled: HashSet<String>,
}

impl JobRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach the durable job-run store. Every run recorded afterwards is
    /// persisted (best-effort: a persistence failure is logged, never fails
    /// the job itself). Idempotent.
    pub async fn set_durable_store(&self, store: Arc<dyn gyre_ports::JobRunRepository>) {
        *self.durable.lock().await = Some(store);
    }

    /// Register a job. Idempotent; re-registration overwrites the definition.
    pub async fn register<F, Fut>(&self, def: JobDefinition, handler: F)
    where
        F: Fn(Arc<AppState>, JobRunLog) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let handler: JobHandler = Arc::new(move |state, log| Box::pin(handler(state, log)));
        let mut inner = self.inner.lock().await;
        inner
            .jobs
            .insert(def.name.clone(), RegisteredJob { def, handler });
    }

    /// Return definitions of all registered jobs.
    pub async fn list_jobs(&self) -> Vec<JobDefinition> {
        let inner = self.inner.lock().await;
        inner.jobs.values().map(|j| j.def.clone()).collect()
    }

    /// Return history for a specific job.
    pub async fn history(&self, job_name: &str) -> Vec<JobRun> {
        let inner = self.inner.lock().await;
        inner
            .history
            .get(job_name)
            .map(|runs| runs.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// DURABLE history for a specific job (newest-first, at most `limit`
    /// rows), read through the attached `JobRunRepository`. Falls back to
    /// the in-memory cache when no durable store is attached (pure-mem
    /// mode before `start_job_registry` ran).
    pub async fn durable_history(&self, job_name: &str, limit: usize) -> Vec<JobRun> {
        let store = self.durable.lock().await.clone();
        match store {
            Some(store) => store
                .list_by_job(job_name, limit)
                .await
                .map(|runs| runs.into_iter().map(JobRun::from_record).collect())
                .unwrap_or_default(),
            None => self.history(job_name).await,
        }
    }

    /// Return the most recent run across all jobs.
    pub async fn all_history(&self) -> Vec<JobRun> {
        let inner = self.inner.lock().await;
        let mut all: Vec<JobRun> = inner
            .history
            .values()
            .flat_map(|runs| runs.iter().cloned())
            .collect();
        all.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        all
    }

    /// Run a job by name immediately, recording history. Returns error if job not found.
    pub async fn trigger(&self, job_name: &str, state: Arc<AppState>) -> anyhow::Result<()> {
        let handler = {
            let inner = self.inner.lock().await;
            inner
                .jobs
                .get(job_name)
                .map(|j| j.handler.clone())
                .ok_or_else(|| anyhow::anyhow!("job '{}' not found", job_name))?
        };

        let run_id = uuid::Uuid::new_v4().to_string();
        let started_at = now_secs();
        let log_sink = JobRunLog::new();

        let result = handler(state, log_sink.clone()).await;

        let finished_at = now_secs();
        let (status, error) = match &result {
            Ok(()) => ("success".to_string(), None),
            Err(e) => ("failed".to_string(), Some(e.to_string())),
        };

        let run = JobRun {
            id: run_id,
            job_name: job_name.to_string(),
            started_at,
            finished_at: Some(finished_at),
            status,
            error,
            logs: log_sink.take().await,
        };

        self.record_run(job_name, run).await;
        result
    }

    pub async fn record_run(&self, job_name: &str, run: JobRun) {
        // Durable persistence first (authoritative store; TASK-273):
        // best-effort — a persistence failure is logged but never fails
        // the job that produced the run.
        if let Some(store) = self.durable.lock().await.as_ref() {
            if let Err(e) = store.record(&run.to_record()).await {
                error!(
                    job = %job_name,
                    "persisting job run to the durable store failed: {e}"
                );
            }
        }
        let mut inner = self.inner.lock().await;
        let history = inner.history.entry(job_name.to_string()).or_default();
        if history.len() == MAX_HISTORY_PER_JOB {
            history.pop_front();
        }
        history.push_back(run);
    }

    /// Record the outcome of one scheduler cycle for loops that live outside
    /// `spawn_job` (merge processor, stale agent detector).
    pub async fn record_cycle(&self, job_name: &str, started_at: u64, result: &anyhow::Result<()>) {
        let (status, error) = match result {
            Ok(()) => ("success", None),
            Err(e) => ("failed", Some(e.to_string())),
        };
        let run = JobRun {
            id: uuid::Uuid::new_v4().to_string(),
            job_name: job_name.to_string(),
            started_at,
            finished_at: Some(now_secs()),
            status: status.to_string(),
            error,
            logs: Vec::new(),
        };
        self.record_run(job_name, run).await;
    }

    /// Mark that a scheduler loop is running for `job_name`. Health probes
    /// evaluate liveness only for scheduled jobs (trigger-only registrations
    /// have no loop to be alive).
    pub async fn mark_scheduled(&self, job_name: &str) {
        self.inner
            .lock()
            .await
            .scheduled
            .insert(job_name.to_string());
    }

    /// Definitions of jobs that currently have a running scheduler loop.
    pub async fn scheduled_jobs(&self) -> Vec<JobDefinition> {
        let inner = self.inner.lock().await;
        inner
            .scheduled
            .iter()
            .filter_map(|name| inner.jobs.get(name).map(|j| j.def.clone()))
            .collect()
    }

    /// Liveness verdict: alive when the most recent run started within 2x the
    /// job's interval. Wall-clock daily jobs get interval + 1 h slack, since
    /// their schedule is one occurrence per day. A scheduled job that has
    /// never run is `Pending` until 2x its interval has elapsed since
    /// `process_started_at`, then `Stale`.
    pub async fn liveness(&self, job_name: &str, now: u64, process_started_at: u64) -> JobLiveness {
        let inner = self.inner.lock().await;
        let Some(job) = inner.jobs.get(job_name) else {
            return JobLiveness::NotScheduled;
        };
        if !inner.scheduled.contains(job_name) {
            return JobLiveness::NotScheduled;
        }
        let window = match job.def.run_at_utc_hour {
            Some(_) => job.def.interval_secs.saturating_add(3600),
            None => job.def.interval_secs.saturating_mul(2),
        };
        match inner.history.get(job_name).and_then(|h| h.back()) {
            Some(run) if run.status == "failed" => JobLiveness::Failed,
            Some(run) if now.saturating_sub(run.started_at) <= window => JobLiveness::Ok,
            None if now.saturating_sub(process_started_at) <= window => JobLiveness::Pending,
            _ => JobLiveness::Stale,
        }
    }
}

// ── Scheduler ─────────────────────────────────────────────────────────────────

/// Seconds until the next daily occurrence of `utc_hour:00` UTC, from
/// `now_secs`. At exactly `utc_hour:00` the next run is a full day away.
pub(crate) fn next_daily_run_secs(now_secs: u64, utc_hour: u8) -> u64 {
    debug_assert!(utc_hour < 24);
    let day = 86_400u64;
    let days_since_epoch = now_secs / day;
    let today_target = days_since_epoch * day + (utc_hour as u64) * 3_600;
    if today_target > now_secs {
        today_target - now_secs
    } else {
        today_target + day - now_secs
    }
}

/// Start the scheduler loop for a single job.
///
/// Jobs with `run_at_utc_hour: Some(h)` run once per day at h:00 UTC
/// (wall-clock aligned); all others run on a fixed `interval_secs` interval
/// from process start.
pub fn spawn_job(registry: Arc<JobRegistry>, job_name: String, state: Arc<AppState>) {
    let registry = registry.clone();
    tokio::spawn(async move {
        // Health probes (business-continuity.md §2) evaluate liveness only
        // for jobs marked scheduled.
        registry.mark_scheduled(&job_name).await;
        // Get interval + wall-clock schedule from the registry
        let (interval_secs, utc_hour) = {
            let inner = registry.inner.lock().await;
            let def = inner.jobs.get(&job_name).map(|j| j.def.clone());
            match def {
                Some(def) => (def.interval_secs, def.run_at_utc_hour),
                None => (60, None),
            }
        };

        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(interval_secs));
        loop {
            if let Some(hour) = utc_hour {
                // Wall-clock schedule: sleep until the next h:00 UTC.
                let now = now_secs();
                let delay = next_daily_run_secs(now, hour);
                tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;
            } else {
                interval.tick().await;
            }

            let handler = {
                let inner = registry.inner.lock().await;
                inner.jobs.get(&job_name).map(|j| j.handler.clone())
            };

            let Some(handler) = handler else { break };

            let run_id = uuid::Uuid::new_v4().to_string();
            let started_at = now_secs();
            let log_sink = JobRunLog::new();

            info!(job = %job_name, "running scheduled job");
            let result = handler(state.clone(), log_sink.clone()).await;
            let finished_at = now_secs();

            let (status, error) = match &result {
                Ok(()) => ("success".to_string(), None),
                Err(e) => {
                    error!(job = %job_name, error = %e, "job failed");
                    ("failed".to_string(), Some(e.to_string()))
                }
            };

            let run = JobRun {
                id: run_id,
                job_name: job_name.clone(),
                started_at,
                finished_at: Some(finished_at),
                status,
                error,
                logs: log_sink.take().await,
            };

            registry.record_run(&job_name, run).await;
        }
    });
}

pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Register all background jobs into the state's job registry.
///
/// The actual background loops (merge processor, stale agent detector) are kept in their
/// existing modules; this function registers handlers into AppState's registry so that
/// `GET /admin/jobs` and `POST /admin/jobs/{name}/run` work correctly.
/// Call once from main after `build_state`.
pub async fn start_job_registry(state: Arc<AppState>) {
    let registry = Arc::clone(&state.job_registry);

    // TASK-273 (admin-panel.md: "Full job history stored and visible"):
    // attach the durable job-run store so every recorded run — scheduler
    // cycles, manual triggers, and external loops' record_cycle calls — is
    // persisted with logs. The in-memory ring becomes a fronting cache.
    registry
        .set_durable_store(Arc::clone(&state.job_runs))
        .await;

    // Register merge_processor job
    registry
        .register(
            JobDefinition {
                name: "merge_processor".to_string(),
                description: "Processes queued merge requests".to_string(),
                interval_secs: 5,
                enabled: true,
                run_at_utc_hour: None,
            },
            |state, _log| async move { crate::merge_processor::run_once(&state).await },
        )
        .await;

    // Register stale_agent_detector job
    registry
        .register(
            JobDefinition {
                name: "stale_agent_detector".to_string(),
                description: "Marks agents dead when heartbeat times out (>60s)".to_string(),
                interval_secs: 30,
                enabled: true,
                run_at_utc_hour: None,
            },
            |state, _log| async move { crate::stale_agents::run_once(&state).await },
        )
        .await;

    // Register retention cleanup job (nightly 02:00 UTC, business-continuity.md §5)
    registry
        .register(
            JobDefinition {
                name: "retention_cleanup".to_string(),
                description: "Deletes data older than configured retention policies".to_string(),
                interval_secs: 86400,
                enabled: true,
                run_at_utc_hour: Some(2),
            },
            |state, _log| async move { state.retention_store.run_cleanup(&state).await },
        )
        .await;

    // Register log compression job (observability.md §Log Retention &
    // Compression) — daily by default, configurable via
    // GYRE_LOG_COMPRESSION_INTERVAL (seconds).
    registry
        .register(
            JobDefinition {
                name: crate::log_compression::JOB_NAME.to_string(),
                description: "Zstd-compresses audit event details and agent log \
                     lines older than 7 days (observability §Log Retention & Compression)"
                    .to_string(),
                interval_secs: crate::log_compression::interval_secs(),
                enabled: true,
                run_at_utc_hour: None,
            },
            |state, _log| async move { crate::log_compression::run_once(&state).await.map(|_| ()) },
        )
        .await;

    // Register the budget rolling-window job (agent-runtime.md §4 Budget
    // Reset): hourly re-sync of the token/cost counters to the per-call
    // records inside the last 24 hours (rolling window, not calendar day).
    registry
        .register(
            JobDefinition {
                name: "spawn_budget_reset".to_string(),
                description: "Applies the rolling 24h budget window: re-syncs token/cost                      counters from per-call records inside the window (hourly)"
                    .to_string(),
                interval_secs: 3600,
                enabled: true,
                run_at_utc_hour: None,
            },
            |state, _log| async move { crate::api::budget::apply_rolling_24h_window(&state).await },
        )
        .await;

    // Register mirror sync job (runs every 60 seconds)
    registry
        .register(
            JobDefinition {
                name: "mirror_sync".to_string(),
                description: "Fetches latest refs for all mirror repositories".to_string(),
                interval_secs: 60,
                enabled: true,
                run_at_utc_hour: None,
            },
            |state, _log| async move { crate::mirror_sync::run_once(&state).await },
        )
        .await;

    // Register speculative merge job (runs every 60 seconds, M13.5)
    registry
        .register(
            JobDefinition {
                name: "speculative_merge".to_string(),
                description: "Checks active agent branches for conflicts against main (M13.5)"
                    .to_string(),
                interval_secs: 60,
                enabled: true,
                run_at_utc_hour: None,
            },
            |state, _log| async move { crate::speculative_merge::run_once(&state).await },
        )
        .await;

    // Register abandoned_branch_check job (runs daily, ui-layout.md §3)
    registry
        .register(
            JobDefinition {
                name: "abandoned_branch_check".to_string(),
                description:
                    "Flags spec-edit/* MRs with no activity for >7 days as priority-9 Inbox items"
                        .to_string(),
                interval_secs: 86400,
                enabled: true,
                run_at_utc_hour: None,
            },
            |state, _log| async move { crate::abandoned_branch::run_once(&state).await },
        )
        .await;

    // Register cross_workspace_link_staleness_check job (runs daily, HSI §6)
    registry
        .register(
            JobDefinition {
                name: "cross_workspace_link_staleness_check".to_string(),
                description:
                    "Re-resolves cross-workspace spec links and marks stale entries (HSI §6)"
                        .to_string(),
                interval_secs: 86400,
                enabled: true,
                run_at_utc_hour: None,
            },
            |state, _log| async move { crate::spec_link_staleness::run_once(&state).await },
        )
        .await;

    // Register orphaned workspace check job (runs daily,
    // user-management.md §Ownership Transfer & Reclamation)
    registry
        .register(
            JobDefinition {
                name: "orphaned_workspace_check".to_string(),
                description: "Tracks workspaces with no active Owner: notifies TenantAdmins \
             on the transition and flags workspaces ownerless beyond the configured period \
             (default 30 days) in the admin dashboard"
                    .to_string(),
                interval_secs: 86400,
                enabled: true,
                run_at_utc_hour: None,
            },
            |state, _log| async move { crate::workspace_ownership::run_once(&state).await },
        )
        .await;

    // Register trust_suggestion_check job (runs daily, HSI §2)
    registry
        .register(
            JobDefinition {
                name: "trust_suggestion_check".to_string(),
                description: "Queries 30-day gate failures and MR reverts per workspace; creates \
             TrustSuggestion notifications for Admin/Owner members when both counts \
             are 0 and trust can escalate (HSI §2)"
                    .to_string(),
                interval_secs: 86400,
                enabled: true,
                run_at_utc_hour: None,
            },
            |state, _log| async move { crate::trust_suggestion::run_once(&state).await },
        )
        .await;

    // Register message expiry job (hourly, message-bus.md §Implementation Notes)
    registry
        .register(
            JobDefinition {
                name: "message_expiry".to_string(),
                description: "Deletes expired Event-tier messages and acked dead-agent \
             inboxes (message-bus.md TTL)"
                    .to_string(),
                interval_secs: 3600,
                enabled: true,
                run_at_utc_hour: None,
            },
            |state, _log| async move {
                crate::message_dispatcher::run_message_expiry(&state)
                    .await
                    .map(|_| ())
            },
        )
        .await;

    // Register task_scheduler job (agent-runtime.md §1 Phases 3–4, task-115):
    // claims Backlog Delegation/Coordination tasks and runs repo orchestrators
    // under per-repo registry locks. The 30 s loop itself is spawned in main.rs
    // (signal_chain::spawn_task_scheduler) and records its cycles under this
    // same name so /healthz liveness and POST /admin/jobs/task_scheduler/run work.
    registry
        .register(
            JobDefinition {
                name: "task_scheduler".to_string(),
                description: "Claims Delegation/Coordination tasks and runs repo orchestrators \
             (agent-runtime.md §1 Phase 3)"
                    .to_string(),
                interval_secs: 30,
                enabled: true,
                run_at_utc_hour: None,
            },
            |state, _log| async move { crate::signal_chain::scheduler_run_once(&state).await },
        )
        .await;

    // Register dep_staleness_check job (runs daily, dependency-graph.md §Version Drift)
    registry
        .register(
            JobDefinition {
                name: "dep_staleness_check".to_string(),
                description:
                    "Scans dependency edges for version drift and time-based staleness (TASK-021)"
                        .to_string(),
                interval_secs: 86400,
                enabled: true,
                run_at_utc_hour: None,
            },
            |state, _log| async move { crate::dep_staleness::run_once(&state).await },
        )
        .await;

    // Register code-search file indexing (search.md §Index Updates: file
    // content is indexed asynchronously on push; also runs on a fixed interval
    // so the index stays consistent after restarts without a manual reindex).
    registry
        .register(
            JobDefinition {
                name: "search_code_index".to_string(),
                description: "Indexes repo file content for code search (search.md)".to_string(),
                interval_secs: 3600,
                enabled: true,
                run_at_utc_hour: None,
            },
            |state, _log| async move { crate::search_indexer::run_code_index_job(&state).await },
        )
        .await;

    // Register meta_spec_stale_pin_check job (agent-runtime.md §2 Stale Pin
    // Detection): compares spec-level binding pins against current versions.
    registry
        .register(
            JobDefinition {
                name: "meta_spec_stale_pin_check".to_string(),
                description: "Detects specs pinning old meta-spec versions and creates priority-6 \
                     MetaSpecDrift notifications (agent-runtime §2)"
                    .to_string(),
                interval_secs: 3600,
                enabled: true,
                run_at_utc_hour: None,
            },
            |state, _log| async move { crate::prompt_assembly::detect_stale_pins(&state).await },
        )
        .await;

    // Register secret expiry check job (every 5 minutes, platform-model.md §7)
    registry
        .register(
            JobDefinition {
                name: "secret_rotation_check".to_string(),
                description: "Creates SecretExpiring notifications for workspace admins \
                             when secrets expire within 1 hour (platform-model.md §7)"
                    .to_string(),
                interval_secs: 300,
                enabled: true,
                run_at_utc_hour: None,
            },
            |state, log| async move { crate::secret_rotation::run_once(&state, &log).await },
        )
        .await;

    // Register meta-spec conformance sweep job (runs daily,
    // meta-spec-reconciliation.md §10). Interval is configurable via
    // GYRE_META_SPEC_SWEEP_INTERVAL_SECS.
    registry
        .register(
            JobDefinition {
                name: "meta_spec_conformance_sweep".to_string(),
                description: "Sweeps all workspaces for meta-spec drift between the active \
                     meta-spec set and provenance-recorded set SHAs (§10)"
                    .to_string(),
                interval_secs: std::env::var("GYRE_META_SPEC_SWEEP_INTERVAL_SECS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(crate::reconciliation::SWEEP_INTERVAL_SECS),
                enabled: true,
                run_at_utc_hour: None,
            },
            |state, _log| async move {
                crate::reconciliation::run_conformance_sweep(&state)
                    .await
                    .map(|_| ())
            },
        )
        .await;

    // Register impersonation expiry job (runs every 30 seconds,
    // identity-security.md §Impersonation): ends sessions past their TTL
    // (emitting ImpersonationEnded) and expires stale approval requests.
    registry
        .register(
            JobDefinition {
                name: "impersonation_expiry".to_string(),
                description: "Ends impersonation sessions past their TTL and expires \
                     stale approval requests (identity-security.md §Impersonation)"
                    .to_string(),
                interval_secs: 30,
                enabled: true,
                run_at_utc_hour: None,
            },
            |state, _log| async move {
                crate::impersonation::expire_sessions(&state)
                    .await
                    .map(|_| ())
            },
        )
        .await;

    // Register the email digest drain (user-management.md §Email
    // Notifications): every 60 s it drains the durable email outbox at
    // each user's configured digest frequency — Immediate sends
    // individually, Hourly/Daily aggregate once per window. With no SMTP
    // relay configured (GYRE_SMTP_HOST) it holds the queue untouched.
    registry
        .register(
            JobDefinition {
                name: "email_digest".to_string(),
                description: "Drains the durable email outbox: renders per-type \
                     templates + digest aggregates in the user's locale and delivers \
                     via the configured SMTP relay (user-management.md §Email Notifications)"
                    .to_string(),
                interval_secs: 60,
                enabled: true,
                run_at_utc_hour: None,
            },
            |state, _log| async move { crate::email::run_digest_once(&state).await },
        )
        .await;

    // Register meta_spec_preview_gc job (§5 of meta-spec-reconciliation.md)
    registry
        .register(
            JobDefinition {
                name: "meta_spec_preview_gc".to_string(),
                description: "Tears down meta-spec preview runs past their TTL: kills agents, \
             removes worktrees, deletes preview branches, revokes tokens, releases \
             budget slots"
                    .to_string(),
                interval_secs: crate::api::meta_specs::preview_gc_interval_secs_from_env(),
                enabled: true,
                run_at_utc_hour: None,
            },
            |state| async move { crate::api::meta_specs::run_once(&state).await },
        )
        .await;

    // Register PR hygiene job (specs/development/agent-workflow.md §PRs as
    // the Collaboration Primitive, task-270): continuously (minutes-scale
    // interval, NOT the daily abandoned_branch_check cadence) monitors open
    // MRs for stale classes — unreviewed, conflicted, long-open, abandoned —
    // and acts: nudge reviewers, rebase, escalate, or close with
    // explanation. Interval configurable via GYRE_PR_HYGIENE_INTERVAL_SECS.
    registry
        .register(
            JobDefinition {
                name: crate::pr_hygiene::JOB_NAME.to_string(),
                description: "Nudges reviewers, rebases, escalates, or closes stale \
                     open merge requests (agent-workflow.md §PRs as the Collaboration \
                     Primitive)"
                    .to_string(),
                interval_secs: crate::pr_hygiene::interval_secs(),
                enabled: true,
                run_at_utc_hour: None,
            },
            |state| async move { crate::pr_hygiene::run_once(&state).await },
        )
        .await;

    // Schedulers are NOT spawned here — existing background tasks in main.rs handle
    // periodic execution. Handlers registered above enable on-demand triggering and
    // status tracking via POST /admin/jobs/{name}/run and GET /admin/jobs.

    // ── TASK-273 maintenance jobs (admin-panel.md §Background Jobs) ─────────

    // Self-managing job history cleanup: prunes durable job runs past the
    // retention (GYRE_JOB_HISTORY_RETENTION_DAYS, default 30 days).
    registry
        .register(
            JobDefinition {
                name: "job_history_cleanup".to_string(),
                description: "Prunes durable job-run history older than the configured \
                     retention (GYRE_JOB_HISTORY_RETENTION_DAYS, default 30 days) — \
                     self-managing (admin-panel.md §Background Jobs)"
                    .to_string(),
                interval_secs: 86400,
                enabled: true,
                run_at_utc_hour: Some(3),
            },
            |state, log| async move {
                crate::maintenance_jobs::job_history_cleanup(state, log).await
            },
        )
        .await;

    // Global repository maintenance: git gc (prune + repack) for every
    // stored repository, per-repo outcomes in the run logs.
    registry
        .register(
            JobDefinition {
                name: "repo_maintenance".to_string(),
                description: "Runs git gc --prune=now (unreachable-object prune + repack) \
                     in every stored repository; per-repo outcomes recorded in run logs \
                     (admin-panel.md §Background Jobs)"
                    .to_string(),
                interval_secs: 86400,
                enabled: true,
                run_at_utc_hour: Some(4),
            },
            |state, log| async move { crate::maintenance_jobs::repo_maintenance(state, log).await },
        )
        .await;

    // OAuth state cleanup: deletes expired authorization-request rows
    // from the Claude Max enrollment flow.
    registry
        .register(
            JobDefinition {
                name: "oauth_state_cleanup".to_string(),
                description: "Deletes expired OAuth authorization-request state rows \
                     (admin-panel.md §Background Jobs)"
                    .to_string(),
                interval_secs: 3600,
                enabled: true,
                run_at_utc_hour: None,
            },
            |state, log| async move {
                crate::maintenance_jobs::oauth_state_cleanup(state, log).await
            },
        )
        .await;

    // Remote agent cleanup: kills still-recorded-running instances whose
    // agent is Dead/Stopped or absent (including restart orphans).
    registry
        .register(
            JobDefinition {
                name: "remote_agent_cleanup".to_string(),
                description: "Kills recorded-running agent instances whose agent is \
                     Dead/Stopped or absent, including orphans left by a server restart, \
                     and marks them terminated (admin-panel.md §Background Jobs)"
                    .to_string(),
                interval_secs: 600,
                enabled: true,
                run_at_utc_hour: None,
            },
            |state, log| async move {
                crate::maintenance_jobs::remote_agent_cleanup(state, log).await
            },
        )
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;

    #[tokio::test]
    async fn register_and_list() {
        let registry = JobRegistry::new();
        registry
            .register(
                JobDefinition {
                    name: "test_job".to_string(),
                    description: "A test job".to_string(),
                    interval_secs: 10,
                    enabled: true,
                    run_at_utc_hour: None,
                },
                |_state, _log| async move { Ok(()) },
            )
            .await;

        let jobs = registry.list_jobs().await;
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].name, "test_job");
    }

    #[tokio::test]
    async fn trigger_records_success() {
        let registry = JobRegistry::new();
        registry
            .register(
                JobDefinition {
                    name: "ok_job".to_string(),
                    description: "Always succeeds".to_string(),
                    interval_secs: 60,
                    enabled: true,
                    run_at_utc_hour: None,
                },
                |_state, _log| async move { Ok(()) },
            )
            .await;

        let state = test_state();
        registry.trigger("ok_job", state).await.unwrap();

        let history = registry.history("ok_job").await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].status, "success");
        assert!(history[0].finished_at.is_some());
        assert!(history[0].error.is_none());
    }

    #[tokio::test]
    async fn trigger_records_failure() {
        let registry = JobRegistry::new();
        registry
            .register(
                JobDefinition {
                    name: "fail_job".to_string(),
                    description: "Always fails".to_string(),
                    interval_secs: 60,
                    enabled: true,
                    run_at_utc_hour: None,
                },
                |_state, _log| async move { Err(anyhow::anyhow!("intentional failure")) },
            )
            .await;

        let state = test_state();
        let result = registry.trigger("fail_job", state).await;
        assert!(result.is_err());

        let history = registry.history("fail_job").await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].status, "failed");
        assert!(history[0].error.is_some());
    }

    #[tokio::test]
    async fn trigger_unknown_job_errors() {
        let registry = JobRegistry::new();
        let state = test_state();
        let result = registry.trigger("no_such_job", state).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn history_capped_at_max() {
        let registry = JobRegistry::new();
        registry
            .register(
                JobDefinition {
                    name: "capped_job".to_string(),
                    description: "Test capacity".to_string(),
                    interval_secs: 1,
                    enabled: true,
                    run_at_utc_hour: None,
                },
                |_state, _log| async move { Ok(()) },
            )
            .await;

        let state = test_state();
        for _ in 0..=(MAX_HISTORY_PER_JOB + 5) {
            registry
                .trigger("capped_job", Arc::clone(&state))
                .await
                .ok();
        }

        let history = registry.history("capped_job").await;
        assert!(history.len() <= MAX_HISTORY_PER_JOB);
    }

    #[tokio::test]
    async fn all_history_aggregates_across_jobs() {
        let registry = JobRegistry::new();
        for name in &["job_a", "job_b"] {
            let n = name.to_string();
            registry
                .register(
                    JobDefinition {
                        name: n.clone(),
                        description: format!("{n} job"),
                        interval_secs: 60,
                        enabled: true,
                        run_at_utc_hour: None,
                    },
                    |_state, _log| async move { Ok(()) },
                )
                .await;
        }

        let state = test_state();
        registry.trigger("job_a", Arc::clone(&state)).await.ok();
        registry.trigger("job_b", Arc::clone(&state)).await.ok();

        let all = registry.all_history().await;
        assert_eq!(all.len(), 2);
    }

    /// §5 preview GC registration: `start_job_registry` must register the
    /// `meta_spec_preview_gc` job (name, default interval, enabled) with the
    /// handler bound to the real preview GC sweep — triggering it runs
    /// `meta_specs::run_once` and records a successful cycle, not a stub.
    #[tokio::test]
    async fn meta_spec_preview_gc_job_registered() {
        let state = test_state();
        start_job_registry(Arc::clone(&state)).await;

        let jobs = state.job_registry.list_jobs().await;
        let job = jobs
            .iter()
            .find(|j| j.name == "meta_spec_preview_gc")
            .expect("meta_spec_preview_gc must be registered by start_job_registry");
        assert!(job.enabled, "the preview GC job must be enabled");
        assert_eq!(
            job.interval_secs, 3600,
            "default GC interval must be 1 hour (GYRE_META_SPEC_PREVIEW_GC_INTERVAL_SECS unset)"
        );

        // Handler binding: the trigger must execute the real sweep against
        // the state (an empty store sweeps cleanly and records success).
        state
            .job_registry
            .trigger("meta_spec_preview_gc", Arc::clone(&state))
            .await
            .unwrap();
        let history = state.job_registry.history("meta_spec_preview_gc").await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].status, "success");
    }

    /// task-270 (agent-workflow.md §PRs as the Collaboration Primitive):
    /// the PR hygiene job must be registered on a minutes-scale interval —
    /// "continuously monitors" — never as a run_at_utc_hour daily job like
    /// abandoned_branch_check, and its handler must be wired so a manual
    /// trigger runs a real evaluation.
    #[tokio::test]
    async fn pr_hygiene_check_registered_on_interval_not_daily() {
        let state = test_state();
        start_job_registry(Arc::clone(&state)).await;

        let jobs = state.job_registry.list_jobs().await;
        let job = jobs
            .iter()
            .find(|j| j.name == "pr_hygiene_check")
            .expect("pr_hygiene_check must be registered");
        assert_eq!(
            job.run_at_utc_hour, None,
            "continuous cadence, not a run_at_utc_hour daily schedule"
        );
        assert!(
            job.interval_secs > 0 && job.interval_secs < 86400,
            "minutes-scale interval, got {}",
            job.interval_secs
        );

        // The registered handler runs a real evaluation.
        state
            .job_registry
            .trigger("pr_hygiene_check", Arc::clone(&state))
            .await
            .unwrap();
        let history = state.job_registry.history("pr_hygiene_check").await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].status, "success");
    }
}
