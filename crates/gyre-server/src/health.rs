//! Infrastructure health probes (business-continuity.md §2).
//!
//! Three levels, all unauthenticated (registered outside the auth layer in
//! `build_router`):
//! - `GET /health`  — process alive; dependency-free (LB liveness).
//! - `GET /healthz` — DB connectivity + every scheduled background job alive
//!   (Kubernetes liveness).
//! - `GET /readyz`  — DB connectivity + migration state + merge processor
//!   running (Kubernetes readiness).
//!
//! `/healthz` and `/readyz` return structured `{status, checks}` JSON:
//! status is `"ok"` only when all checks pass; any failing check returns 503.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde_json::{json, Map, Value};
use std::sync::Arc;
use tracing::instrument;

use crate::jobs::JobLiveness;
use crate::AppState;

const REQUIRED_JOBS: &[&str] = &[
    "merge_processor",
    "stale_agent_detector",
    "retention_cleanup",
    "spawn_budget_reset",
];

/// GET /health - load-balancer liveness: process alive, no dependency checks.
#[instrument]
pub async fn health_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({"status": "ok", "version": "0.1.0"})),
    )
}

/// One named probe check: pass verdict plus the value reported in `checks`.
struct Check {
    ok: bool,
    value: String,
}

impl Check {
    fn pass(value: impl Into<String>) -> Self {
        Self {
            ok: true,
            value: value.into(),
        }
    }
    fn fail(value: impl Into<String>) -> Self {
        Self {
            ok: false,
            value: value.into(),
        }
    }
    fn liveness(l: JobLiveness) -> Self {
        if l.is_live() {
            Self::pass(l.as_str())
        } else {
            Self::fail(l.as_str())
        }
    }
}

/// DB connectivity: a real query round-trip via the storage port.
/// `not_configured` (pure in-memory mode, no DB backend) passes — there is
/// no database to be disconnected from.
async fn database_check(state: &AppState) -> Check {
    match &state.storage {
        Some(storage) => match storage.health_check().await {
            Ok(()) => Check::pass("ok"),
            Err(e) => Check::fail(format!("error: {e:#}")),
        },
        None => Check::pass("not_configured"),
    }
}

/// Migration state: every embedded migration applied against the real
/// database (0 pending).
async fn migrations_check(state: &AppState) -> Check {
    match &state.storage {
        Some(storage) => match storage.migrations_pending().await {
            Ok(0) => Check::pass("ok"),
            Ok(n) => Check::fail(format!("{n} pending")),
            Err(e) => Check::fail(format!("error: {e:#}")),
        },
        None => Check::pass("not_configured"),
    }
}

/// Assemble the structured `{status, checks}` probe response: status is
/// `"ok"` only when every check passed; any failing check returns HTTP 503.
fn probe_response(checks: Vec<(String, Check)>) -> impl IntoResponse {
    let mut all_ok = true;
    let mut map = Map::new();
    for (name, check) in checks {
        all_ok &= check.ok;
        map.insert(name, Value::String(check.value));
    }
    let body = json!({
        "status": if all_ok { "ok" } else { "error" },
        "checks": map,
    });
    let code = if all_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (code, Json(body))
}

/// GET /healthz - Kubernetes liveness probe: DB connectivity plus every
/// scheduled background job alive (last run within its liveness window;
/// see `jobs::JobRegistry::liveness`).
pub async fn healthz_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let now = crate::jobs::now_secs();
    let mut checks = vec![("database".to_string(), database_check(&state).await)];
    for name in REQUIRED_JOBS {
        let liveness = state
            .job_registry
            .liveness(name, now, state.started_at_secs)
            .await;
        checks.push(((*name).to_string(), Check::liveness(liveness)));
    }
    for def in state.job_registry.scheduled_jobs().await {
        if REQUIRED_JOBS.contains(&def.name.as_str()) {
            continue;
        }
        let liveness = state
            .job_registry
            .liveness(&def.name, now, state.started_at_secs)
            .await;
        checks.push((def.name, Check::liveness(liveness)));
    }
    probe_response(checks)
}

/// GET /readyz - Kubernetes readiness probe: DB connectivity, migration
/// state, and the merge processor loop running.
pub async fn readyz_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let now = crate::jobs::now_secs();
    let merge = state
        .job_registry
        .liveness("merge_processor", now, state.started_at_secs)
        .await;
    let checks = vec![
        ("database".to_string(), database_check(&state).await),
        ("migrations".to_string(), migrations_check(&state).await),
        ("merge_processor".to_string(), Check::liveness(merge)),
    ];
    probe_response(checks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::{self, JobDefinition};
    use crate::mem;
    use axum::body::Body;
    use axum::routing::get;
    use axum::Router;
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn get_json(app: Router, uri: &str) -> (StatusCode, serde_json::Value) {
        let response = app
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    fn probe_router(state: Arc<AppState>) -> Router {
        Router::new()
            .route("/healthz", get(healthz_handler))
            .route("/readyz", get(readyz_handler))
            .with_state(state)
    }

    /// Test storage port whose backend is unreachable — the probes must
    /// propagate the real port error, not report a fabricated state.
    struct BrokenStorage;

    #[async_trait::async_trait]
    impl gyre_ports::storage::StoragePort for BrokenStorage {
        async fn health_check(&self) -> anyhow::Result<()> {
            Err(anyhow::anyhow!("connection refused"))
        }
        async fn migrations_pending(&self) -> anyhow::Result<usize> {
            Err(anyhow::anyhow!("connection refused"))
        }
    }

    fn sqlite_state() -> (tempfile::NamedTempFile, Arc<AppState>) {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let storage = gyre_adapters::SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        let state = mem::test_state_with_storage(Arc::new(storage));
        (tmp, state)
    }

    /// Register + schedule + record one run of a job (mirrors what the
    /// scheduler loops do at runtime).
    async fn seed_job(state: &AppState, name: &str, interval_secs: u64, age_secs: u64) {
        state
            .job_registry
            .register(
                JobDefinition {
                    name: name.to_string(),
                    description: format!("{name} test job"),
                    interval_secs,
                    enabled: true,
                    run_at_utc_hour: None,
                },
                |_state| async move { Ok(()) },
            )
            .await;
        state.job_registry.mark_scheduled(name).await;
        if age_secs != u64::MAX {
            state
                .job_registry
                .record_cycle(name, jobs::now_secs() - age_secs, &Ok(()))
                .await;
        }
    }

    #[tokio::test]
    async fn health_returns_200() {
        let app = Router::new().route("/health", get(health_handler));
        let (status, json) = get_json(app, "/health").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["status"], "ok");
    }

    #[tokio::test]
    async fn health_returns_correct_json() {
        let app = Router::new().route("/health", get(health_handler));
        let (status, json) = get_json(app, "/health").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["status"], "ok");
        assert_eq!(json["version"], "0.1.0");
    }

    #[tokio::test]
    async fn healthz_ok_with_real_db_and_healthy_jobs() {
        let (_tmp, state) = sqlite_state();
        seed_job(&state, "merge_processor", 5, 0).await;
        seed_job(&state, "stale_agent_detector", 30, 0).await;
        // Daily job scheduled but never due yet: passes as "pending".
        state
            .job_registry
            .register(
                JobDefinition {
                    name: "retention_cleanup".to_string(),
                    description: "nightly".to_string(),
                    interval_secs: 86400,
                    enabled: true,
                    run_at_utc_hour: Some(2),
                },
                |_state| async move { Ok(()) },
            )
            .await;
        state.job_registry.mark_scheduled("retention_cleanup").await;
        seed_job(&state, "spawn_budget_reset", 86400, u64::MAX).await;

        let (status, json) = get_json(probe_router(state), "/healthz").await;
        assert_eq!(status, StatusCode::OK, "body: {json}");
        assert_eq!(json["status"], "ok");
        assert_eq!(json["checks"]["database"], "ok");
        assert_eq!(json["checks"]["merge_processor"], "ok");
        assert_eq!(json["checks"]["stale_agent_detector"], "ok");
        assert_eq!(json["checks"]["retention_cleanup"], "pending");
        assert_eq!(json["checks"]["spawn_budget_reset"], "pending");
    }

    #[tokio::test]
    async fn healthz_fails_when_required_job_is_missing() {
        let (_tmp, state) = sqlite_state();
        seed_job(&state, "merge_processor", 5, 0).await;
        let (status, json) = get_json(probe_router(state), "/healthz").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(json["checks"]["stale_agent_detector"], "not_scheduled");
        assert_eq!(json["checks"]["spawn_budget_reset"], "not_scheduled");
    }

    #[tokio::test]
    async fn healthz_fails_when_recent_job_run_failed() {
        let (_tmp, state) = sqlite_state();
        seed_job(&state, "merge_processor", 5, 0).await;
        state
            .job_registry
            .record_cycle(
                "merge_processor",
                jobs::now_secs(),
                &Err(anyhow::anyhow!("failed cycle")),
            )
            .await;
        let (status, json) = get_json(probe_router(state), "/healthz").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(json["checks"]["merge_processor"], "failed");
    }

    #[tokio::test]
    async fn healthz_503_when_job_loop_is_stale() {
        let (_tmp, state) = sqlite_state();
        // merge_processor interval 5s -> window 10s; last run 1h ago is dead.
        seed_job(&state, "merge_processor", 5, 3600).await;
        seed_job(&state, "stale_agent_detector", 30, 0).await;

        let (status, json) = get_json(probe_router(state), "/healthz").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "body: {json}");
        assert_eq!(json["status"], "error");
        assert_eq!(json["checks"]["merge_processor"], "stale");
        // Healthy checks keep their real values.
        assert_eq!(json["checks"]["database"], "ok");
        assert_eq!(json["checks"]["stale_agent_detector"], "ok");
    }

    #[tokio::test]
    async fn healthz_503_when_database_unreachable() {
        let state = mem::test_state_with_storage(Arc::new(BrokenStorage));
        seed_job(&state, "merge_processor", 5, 0).await;

        let (status, json) = get_json(probe_router(state), "/healthz").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "body: {json}");
        assert_eq!(json["status"], "error");
        let db = json["checks"]["database"].as_str().unwrap();
        assert!(db.starts_with("error:"), "database check: {db}");
    }

    #[tokio::test]
    async fn readyz_ok_when_migrations_applied_and_merge_processor_running() {
        let (_tmp, state) = sqlite_state();
        // SqliteStorage's constructor applied every embedded migration.
        seed_job(&state, "merge_processor", 5, 0).await;

        let (status, json) = get_json(probe_router(state), "/readyz").await;
        assert_eq!(status, StatusCode::OK, "body: {json}");
        assert_eq!(json["status"], "ok");
        assert_eq!(json["checks"]["database"], "ok");
        assert_eq!(json["checks"]["migrations"], "ok");
        assert_eq!(json["checks"]["merge_processor"], "ok");
    }

    #[tokio::test]
    async fn readyz_503_when_storage_broken_and_merge_processor_not_running() {
        let state = mem::test_state_with_storage(Arc::new(BrokenStorage));

        let (status, json) = get_json(probe_router(state), "/readyz").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "body: {json}");
        assert_eq!(json["status"], "error");
        assert!(json["checks"]["database"]
            .as_str()
            .unwrap()
            .starts_with("error:"));
        assert!(json["checks"]["migrations"]
            .as_str()
            .unwrap()
            .starts_with("error:"));
        assert_eq!(json["checks"]["merge_processor"], "not_scheduled");
    }
}
