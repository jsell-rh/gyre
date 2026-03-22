/// Budget governance API — GET/PUT workspace budget limits, GET tenant summary.
///
/// Workspaces are keyed by project_id (the current governance boundary).
/// Budget state is stored in-memory in AppState:
///   - `budget_configs`: entity_key -> BudgetConfig (limits)
///   - `budget_usages`: entity_key -> BudgetUsage  (real-time usage)
///
/// Entity keys: `"workspace:{project_id}"` or `"tenant:global"`.
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    Json,
};
use gyre_domain::{BudgetConfig, BudgetUsage};
use serde::{Deserialize, Serialize};

use super::super::auth::AdminOnly;
use super::now_secs;
use crate::{api::error::ApiError, AppState};

// ── Helpers ───────────────────────────────────────────────────────────────────

pub fn workspace_key(project_id: &str) -> String {
    format!("workspace:{project_id}")
}

pub fn tenant_key() -> &'static str {
    "tenant:global"
}

fn new_ws_usage(entity_id: &str) -> BudgetUsage {
    BudgetUsage {
        entity_type: "workspace".into(),
        entity_id: gyre_common::Id::new(entity_id.to_string()),
        tokens_used_today: 0,
        cost_today: 0.0,
        active_agents: 0,
        period_start: now_secs(),
    }
}

fn new_tenant_usage() -> BudgetUsage {
    BudgetUsage {
        entity_type: "tenant".into(),
        entity_id: gyre_common::Id::new("global".to_string()),
        tokens_used_today: 0,
        cost_today: 0.0,
        active_agents: 0,
        period_start: now_secs(),
    }
}

// ── Request / response shapes ─────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct BudgetResponse {
    pub entity_type: String,
    pub entity_id: String,
    pub config: BudgetConfig,
    pub usage: BudgetUsage,
}

#[derive(Debug, Deserialize)]
pub struct SetBudgetRequest {
    pub max_tokens_per_day: Option<u64>,
    pub max_cost_per_day: Option<f64>,
    pub max_concurrent_agents: Option<u32>,
    pub max_agent_lifetime_secs: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct TenantBudgetSummary {
    pub tenant_config: BudgetConfig,
    pub tenant_usage: BudgetUsage,
    pub workspaces: Vec<BudgetResponse>,
}

// ── GET /api/v1/workspaces/{id}/budget ────────────────────────────────────────

pub async fn get_workspace_budget(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<BudgetResponse>, ApiError> {
    let key = workspace_key(&id);
    let configs = state.budget_configs.lock().await;
    let usages = state.budget_usages.lock().await;
    let config = configs.get(&key).cloned().unwrap_or_default();
    let usage = usages
        .get(&key)
        .cloned()
        .unwrap_or_else(|| new_ws_usage(&id));
    Ok(Json(BudgetResponse {
        entity_type: "workspace".into(),
        entity_id: id,
        config,
        usage,
    }))
}

// ── PUT /api/v1/workspaces/{id}/budget  (Admin only) ─────────────────────────

pub async fn set_workspace_budget(
    State(state): State<Arc<AppState>>,
    _admin: AdminOnly,
    Path(id): Path<String>,
    Json(req): Json<SetBudgetRequest>,
) -> Result<Json<BudgetResponse>, ApiError> {
    let new_config = BudgetConfig {
        max_tokens_per_day: req.max_tokens_per_day,
        max_cost_per_day: req.max_cost_per_day,
        max_concurrent_agents: req.max_concurrent_agents,
        max_agent_lifetime_secs: req.max_agent_lifetime_secs,
    };

    // Cascade: workspace limits cannot exceed tenant limits.
    {
        let tenant_configs = state.budget_configs.lock().await;
        if let Some(tenant) = tenant_configs.get(tenant_key()) {
            if let (Some(ws), Some(t)) = (new_config.max_tokens_per_day, tenant.max_tokens_per_day)
            {
                if ws > t {
                    return Err(ApiError::InvalidInput(format!(
                        "workspace max_tokens_per_day ({ws}) exceeds tenant limit ({t})"
                    )));
                }
            }
            if let (Some(ws), Some(t)) = (new_config.max_cost_per_day, tenant.max_cost_per_day) {
                if ws > t {
                    return Err(ApiError::InvalidInput(format!(
                        "workspace max_cost_per_day ({ws:.4}) exceeds tenant limit ({t:.4})"
                    )));
                }
            }
            if let (Some(ws), Some(t)) = (
                new_config.max_concurrent_agents,
                tenant.max_concurrent_agents,
            ) {
                if ws > t {
                    return Err(ApiError::InvalidInput(format!(
                        "workspace max_concurrent_agents ({ws}) exceeds tenant limit ({t})"
                    )));
                }
            }
        }
    }

    let key = workspace_key(&id);
    {
        let mut configs = state.budget_configs.lock().await;
        configs.insert(key.clone(), new_config.clone());
    }
    let usages = state.budget_usages.lock().await;
    let usage = usages
        .get(&key)
        .cloned()
        .unwrap_or_else(|| new_ws_usage(&id));
    Ok(Json(BudgetResponse {
        entity_type: "workspace".into(),
        entity_id: id,
        config: new_config,
        usage,
    }))
}

// ── GET /api/v1/budget/summary  (Admin only) ─────────────────────────────────

pub async fn budget_summary(
    State(state): State<Arc<AppState>>,
    _admin: AdminOnly,
) -> Result<Json<TenantBudgetSummary>, ApiError> {
    let configs = state.budget_configs.lock().await;
    let usages = state.budget_usages.lock().await;

    let tenant_config = configs.get(tenant_key()).cloned().unwrap_or_default();
    let tenant_usage = usages
        .get(tenant_key())
        .cloned()
        .unwrap_or_else(new_tenant_usage);

    let mut workspaces = Vec::new();
    for (key, config) in configs.iter() {
        if let Some(ws_id) = key.strip_prefix("workspace:") {
            let usage = usages
                .get(key)
                .cloned()
                .unwrap_or_else(|| new_ws_usage(ws_id));
            workspaces.push(BudgetResponse {
                entity_type: "workspace".into(),
                entity_id: ws_id.to_string(),
                config: config.clone(),
                usage,
            });
        }
    }

    Ok(Json(TenantBudgetSummary {
        tenant_config,
        tenant_usage,
        workspaces,
    }))
}

// ── Internal helpers (called from spawn handler and cost recording) ───────────

/// Check if spawning another agent would exceed workspace/tenant budget limits.
pub async fn check_spawn_budget(state: &AppState, project_id: &str) -> Result<(), String> {
    let key = workspace_key(project_id);
    let configs = state.budget_configs.lock().await;
    let usages = state.budget_usages.lock().await;

    if let Some(config) = configs.get(&key) {
        if let Some(max) = config.max_concurrent_agents {
            let current = usages.get(&key).map(|u| u.active_agents).unwrap_or(0);
            if current >= max {
                return Err(format!(
                    "workspace budget exceeded: max_concurrent_agents={max} ({current} active)"
                ));
            }
        }
        if let Some(max_tokens) = config.max_tokens_per_day {
            let used = usages.get(&key).map(|u| u.tokens_used_today).unwrap_or(0);
            if used >= max_tokens {
                return Err(format!(
                    "workspace budget exceeded: max_tokens_per_day={max_tokens} (used {used} today)"
                ));
            }
        }
        if let Some(max_cost) = config.max_cost_per_day {
            let used = usages.get(&key).map(|u| u.cost_today).unwrap_or(0.0);
            if used >= max_cost {
                return Err(format!(
                    "workspace budget exceeded: max_cost_per_day=${max_cost:.4} (spent ${used:.4})"
                ));
            }
        }
    }

    // Tenant-level concurrent agent check.
    if let Some(t_config) = configs.get(tenant_key()) {
        if let Some(max) = t_config.max_concurrent_agents {
            let current = usages
                .get(tenant_key())
                .map(|u| u.active_agents)
                .unwrap_or(0);
            if current >= max {
                return Err(format!(
                    "tenant budget exceeded: max_concurrent_agents={max} ({current} active)"
                ));
            }
        }
    }

    Ok(())
}

/// Increment active-agent counters for a workspace and the tenant.
pub async fn increment_active_agents(state: &AppState, project_id: &str) {
    let ws_key = workspace_key(project_id);
    let mut usages = state.budget_usages.lock().await;
    let ws = usages
        .entry(ws_key)
        .or_insert_with(|| new_ws_usage(project_id));
    ws.active_agents = ws.active_agents.saturating_add(1);
    let tenant = usages
        .entry(tenant_key().to_string())
        .or_insert_with(new_tenant_usage);
    tenant.active_agents = tenant.active_agents.saturating_add(1);
}

/// Decrement active-agent counters for a workspace and the tenant.
pub async fn decrement_active_agents(state: &AppState, project_id: &str) {
    let ws_key = workspace_key(project_id);
    let mut usages = state.budget_usages.lock().await;
    if let Some(ws) = usages.get_mut(&ws_key) {
        ws.active_agents = ws.active_agents.saturating_sub(1);
    }
    if let Some(tenant) = usages.get_mut(tenant_key()) {
        tenant.active_agents = tenant.active_agents.saturating_sub(1);
    }
}

/// Add token/cost usage to workspace and tenant budgets.
pub async fn record_budget_usage(state: &AppState, project_id: &str, tokens: u64, cost_usd: f64) {
    let ws_key = workspace_key(project_id);
    let mut usages = state.budget_usages.lock().await;
    let ws = usages
        .entry(ws_key)
        .or_insert_with(|| new_ws_usage(project_id));
    ws.tokens_used_today = ws.tokens_used_today.saturating_add(tokens);
    ws.cost_today += cost_usd;
    let tenant = usages
        .entry(tenant_key().to_string())
        .or_insert_with(new_tenant_usage);
    tenant.tokens_used_today = tenant.tokens_used_today.saturating_add(tokens);
    tenant.cost_today += cost_usd;
}

/// Reset daily counters to zero. Called at midnight UTC by background job.
pub async fn reset_daily_counters(state: &AppState) {
    let now = now_secs();
    let mut usages = state.budget_usages.lock().await;
    for usage in usages.values_mut() {
        usage.tokens_used_today = 0;
        usage.cost_today = 0.0;
        usage.period_start = now;
    }
    tracing::info!("Budget daily counters reset at {now}");
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use http::StatusCode;
    use std::sync::Arc;
    use tower::ServiceExt;

    fn make_test_app() -> axum::Router {
        crate::build_router(crate::mem::test_state())
    }

    fn json_body(s: &str) -> Body {
        Body::from(s.to_string())
    }

    #[tokio::test]
    async fn get_workspace_budget_returns_defaults() {
        let app = make_test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/proj-1/budget")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["entity_type"], "workspace");
        assert_eq!(v["entity_id"], "proj-1");
        assert_eq!(v["usage"]["active_agents"], 0);
        assert_eq!(v["usage"]["tokens_used_today"], 0);
    }

    #[tokio::test]
    async fn set_workspace_budget_stores_limits() {
        let app = make_test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/workspaces/ws-1/budget")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(json_body(
                        r#"{"max_concurrent_agents":5,"max_tokens_per_day":100000}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["config"]["max_concurrent_agents"], 5);
        assert_eq!(v["config"]["max_tokens_per_day"], 100000);
    }

    #[tokio::test]
    async fn spawn_budget_rejection_at_limit() {
        let state = crate::mem::test_state();
        {
            let mut configs = state.budget_configs.lock().await;
            configs.insert(
                super::workspace_key("proj-x"),
                gyre_domain::BudgetConfig {
                    max_concurrent_agents: Some(0),
                    ..Default::default()
                },
            );
        }
        let result = super::check_spawn_budget(&state, "proj-x").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("max_concurrent_agents=0"));
    }

    #[tokio::test]
    async fn cascade_validation_rejects_workspace_exceeding_tenant() {
        let state = crate::mem::test_state();
        // Set tenant limit.
        {
            let mut configs = state.budget_configs.lock().await;
            configs.insert(
                super::tenant_key().to_string(),
                gyre_domain::BudgetConfig {
                    max_tokens_per_day: Some(1000),
                    ..Default::default()
                },
            );
        }
        let app = crate::build_router(Arc::clone(&state));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/workspaces/ws-cascade/budget")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(json_body(r#"{"max_tokens_per_day":9999999}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn budget_summary_includes_workspaces() {
        let app = make_test_app();
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/workspaces/proj-sum/budget")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(json_body(r#"{"max_concurrent_agents":3}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/budget/summary")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(v["tenant_config"].is_object());
        let ws_ids: Vec<_> = v["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["entity_id"].as_str().unwrap().to_string())
            .collect();
        assert!(ws_ids.contains(&"proj-sum".to_string()));
    }
}
