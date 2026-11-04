//! Job framework for gyre-server.
//!
//! Provides a registry for background jobs, a scheduler that runs them on fixed intervals,
//! and a run-history store for the last N executions per job.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::AppState;

const MAX_HISTORY_PER_JOB: usize = 50;

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
    pub status: String,
    pub error: Option<String>,
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
    dyn Fn(Arc<AppState>) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> + Send + Sync,
>;

struct RegisteredJob {
    def: JobDefinition,
    handler: JobHandler,
}

/// Central job registry — shared between the scheduler and API handlers.
#[derive(Clone, Default)]
pub struct JobRegistry {
    inner: Arc<Mutex<RegistryInner>>,
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

    /// Register a job. Idempotent; re-registration overwrites the definition.
    pub async fn register<F, Fut>(&self, def: JobDefinition, handler: F)
    where
        F: Fn(Arc<AppState>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let handler: JobHandler = Arc::new(move |state| Box::pin(handler(state)));
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

        let result = handler(state).await;

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
        };

        self.record_run(job_name, run).await;
        result
    }

    pub async fn record_run(&self, job_name: &str, run: JobRun) {
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

            info!(job = %job_name, "running scheduled job");
            let result = handler(state.clone()).await;
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
            |state| async move { crate::merge_processor::run_once(&state).await },
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
            |state| async move { crate::stale_agents::run_once(&state).await },
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
            |state| async move { state.retention_store.run_cleanup(&state).await },
        )
        .await;

    registry
        .register(
            JobDefinition {
                name: "spawn_budget_reset".to_string(),
                description: "Resets daily spawn budget counters at midnight UTC".to_string(),
                interval_secs: 86400,
                enabled: true,
                run_at_utc_hour: Some(0),
            },
            |state| async move { crate::api::budget::reset_daily_counters(&state).await },
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
            |state| async move { crate::mirror_sync::run_once(&state).await },
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
            |state| async move { crate::speculative_merge::run_once(&state).await },
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
            |state| async move { crate::abandoned_branch::run_once(&state).await },
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
            |state| async move { crate::spec_link_staleness::run_once(&state).await },
        )
        .await;

    // Register trust_suggestion_check job (runs daily, HSI §2)
    registry
        .register(
            JobDefinition {
                name: "trust_suggestion_check".to_string(),
                description:
                    "Evaluates workspace trust escalation criteria and creates TrustSuggestion \
             notifications for Admin/Owner members when criteria are met (HSI §2)"
                        .to_string(),
                interval_secs: 86400,
                enabled: true,
                run_at_utc_hour: None,
            },
            |state| async move { crate::trust_suggestion::run_once(&state).await },
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
            |state| async move {
                crate::message_dispatcher::run_message_expiry(&state)
                    .await
                    .map(|_| ())
            },
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
            |state| async move { crate::dep_staleness::run_once(&state).await },
        )
        .await;

    // Schedulers are NOT spawned here — existing background tasks in main.rs handle
    // periodic execution. Handlers registered above enable on-demand triggering and
    // status tracking via POST /admin/jobs/{name}/run and GET /admin/jobs.
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
                |_state| async move { Ok(()) },
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
                |_state| async move { Ok(()) },
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
                |_state| async move { Err(anyhow::anyhow!("intentional failure")) },
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
                |_state| async move { Ok(()) },
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
                    |_state| async move { Ok(()) },
                )
                .await;
        }

        let state = test_state();
        registry.trigger("job_a", Arc::clone(&state)).await.ok();
        registry.trigger("job_b", Arc::clone(&state)).await.ok();

        let all = registry.all_history().await;
        assert_eq!(all.len(), 2);
    }
}
