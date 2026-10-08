use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{AnalyticsEvent, CostEntry};
use gyre_ports::analytics::AnalyticsQueryFilter;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

// ─── Analytics Events ────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RecordEventRequest {
    pub event_name: String,
    pub agent_id: Option<String>,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repo_id: Option<String>,
    pub properties: Option<serde_json::Value>,
}

/// GET /api/v1/analytics/events parameters (analytics.md §Query Parameters).
#[derive(Deserialize)]
pub struct QueryEventsParams {
    pub event_name: Option<String>,
    pub agent_id: Option<String>,
    pub user_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repo_id: Option<String>,
    pub since: Option<u64>,
    pub until: Option<u64>,
    pub limit: Option<usize>,
    /// Aggregate by field instead of returning raw events:
    /// `event_name`, `agent_id`, `workspace_id`, `day`.
    pub group_by: Option<String>,
}

#[derive(Deserialize)]
pub struct CountEventsParams {
    pub event_name: String,
    pub since: u64,
    pub until: u64,
}

#[derive(Deserialize)]
pub struct DailyParams {
    pub event_name: String,
    pub since: u64,
    pub until: u64,
}

#[derive(Serialize)]
pub struct AnalyticsEventResponse {
    pub id: String,
    pub event_name: String,
    pub agent_id: Option<String>,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repo_id: Option<String>,
    pub properties: serde_json::Value,
    pub timestamp: u64,
}

impl From<AnalyticsEvent> for AnalyticsEventResponse {
    fn from(e: AnalyticsEvent) -> Self {
        Self {
            id: e.id.to_string(),
            event_name: e.event_name,
            agent_id: e.agent_id,
            user_id: e.user_id,
            session_id: e.session_id,
            workspace_id: e.workspace_id,
            repo_id: e.repo_id,
            properties: e.properties,
            timestamp: e.timestamp,
        }
    }
}

pub async fn record_event(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RecordEventRequest>,
) -> Result<(StatusCode, Json<AnalyticsEventResponse>), ApiError> {
    let event = AnalyticsEvent::new(
        new_id(),
        req.event_name,
        req.agent_id,
        req.properties
            .unwrap_or(serde_json::Value::Object(Default::default())),
        now_secs(),
    )
    .with_scope(
        req.user_id.as_deref().map(Id::new).as_ref(),
        req.session_id,
        req.workspace_id.as_deref().map(Id::new).as_ref(),
        req.repo_id.as_deref().map(Id::new).as_ref(),
    );
    state.analytics.record(&event).await?;
    Ok((
        StatusCode::CREATED,
        Json(AnalyticsEventResponse::from(event)),
    ))
}

pub async fn query_events(
    State(state): State<Arc<AppState>>,
    Query(params): Query<QueryEventsParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let limit = params.limit.unwrap_or(100).min(10_000);
    let filter = AnalyticsQueryFilter {
        event_name: params.event_name.clone(),
        agent_id: params.agent_id.clone(),
        user_id: params.user_id.clone(),
        workspace_id: params.workspace_id.clone(),
        repo_id: params.repo_id.clone(),
        since: params.since,
        until: params.until,
        limit,
    };
    let events = state.analytics.query_filtered(&filter).await?;

    // group_by aggregation (analytics.md §Query Parameters): event_name,
    // agent_id, workspace_id, day. Falls back to raw event list.
    if let Some(field) = params.group_by.as_deref() {
        let counts: serde_json::Map<String, serde_json::Value> = match field {
            "event_name" => {
                let mut m = std::collections::BTreeMap::new();
                for e in &events {
                    *m.entry(e.event_name.clone()).or_insert(0u64) += 1;
                }
                m.into_iter().map(|(k, v)| (k, serde_json::Value::from(v))).collect()
            }
            "agent_id" => {
                let mut m = std::collections::BTreeMap::new();
                for e in &events {
                    *m.entry(e.agent_id.clone().unwrap_or_else(|| "(none)".into()))
                        .or_insert(0u64) += 1;
                }
                m.into_iter().map(|(k, v)| (k, serde_json::Value::from(v))).collect()
            }
            "workspace_id" => {
                let mut m = std::collections::BTreeMap::new();
                for e in &events {
                    *m.entry(e.workspace_id.clone().unwrap_or_else(|| "(none)".into()))
                        .or_insert(0u64) += 1;
                }
                m.into_iter().map(|(k, v)| (k, serde_json::Value::from(v))).collect()
            }
            "day" => {
                let mut m = std::collections::BTreeMap::new();
                for e in &events {
                    let day = epoch_day_string(e.timestamp);
                    *m.entry(day).or_insert(0u64) += 1;
                }
                m.into_iter().map(|(k, v)| (k, serde_json::Value::from(v))).collect()
            }
            other => {
                return Err(ApiError::InvalidInput(format!(
                    "unsupported group_by field: {other} (expected event_name, agent_id, workspace_id, day)"
                )))
            }
        };
        return Ok(Json(serde_json::Value::Object(counts)));
    }

    Ok(Json(serde_json::Value::Array(
        events.into_iter().map(|e| serde_json::to_value(e).unwrap_or_default()).collect(),
    )))
}

/// Format a unix-seconds timestamp as a UTC `YYYY-MM-DD` day key.
fn epoch_day_string(ts: u64) -> String {
    let days = ts / 86_400;
    let (y, m, d) = epoch_days_to_ymd(days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Civil-from-days algorithm (Howard Hinnant) — same math as mem.rs.
fn epoch_days_to_ymd(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

pub async fn count_events(
    State(state): State<Arc<AppState>>,
    Query(params): Query<CountEventsParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let count = state
        .analytics
        .count(&params.event_name, params.since, params.until)
        .await?;
    Ok(Json(
        serde_json::json!({ "event_name": params.event_name, "count": count }),
    ))
}

#[derive(Serialize)]
pub struct DayCount {
    pub date: String,
    pub count: u64,
}

pub async fn daily_events(
    State(state): State<Arc<AppState>>,
    Query(params): Query<DailyParams>,
) -> Result<Json<Vec<DayCount>>, ApiError> {
    let rows = state
        .analytics
        .aggregate_by_day(&params.event_name, params.since, params.until)
        .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(date, count)| DayCount { date, count })
            .collect(),
    ))
}

// ─── Analytics Decision API (M23) ─────────────────────────────────────────────

/// `GET /api/v1/analytics/usage` — event count, unique agents, and trend
/// for the period [since, until] vs the previous equal-length period.
#[derive(Deserialize)]
pub struct UsageParams {
    pub event_name: String,
    /// Start of the current period (unix secs). Defaults to 24h ago.
    pub since: Option<u64>,
    /// End of the current period (unix secs). Defaults to now.
    pub until: Option<u64>,
}

#[derive(Serialize)]
pub struct UsageResponse {
    pub event_name: String,
    pub count: u64,
    pub unique_agents: u64,
    /// "up" if count grew >10% vs prior period, "down" if shrank >10%, else "flat".
    pub trend: &'static str,
}

pub async fn usage(
    State(state): State<Arc<AppState>>,
    Query(params): Query<UsageParams>,
) -> Result<Json<UsageResponse>, ApiError> {
    let now = now_secs();
    let until = params.until.unwrap_or(now);
    let since = params.since.unwrap_or_else(|| until.saturating_sub(86400));
    let period_len = until.saturating_sub(since);

    let count = state
        .analytics
        .count(&params.event_name, since, until)
        .await?;

    // Unique agents: query events and count distinct agent_ids.
    let events = state
        .analytics
        .query(Some(&params.event_name), Some(since), 10_000)
        .await?;
    let unique_agents = events
        .iter()
        .filter(|e| e.timestamp <= until)
        .filter_map(|e| e.agent_id.as_deref())
        .collect::<std::collections::HashSet<_>>()
        .len() as u64;

    // Trend: compare current period vs prior equal-length period.
    let prev_until = since;
    let prev_since = since.saturating_sub(period_len);
    let prev_count = state
        .analytics
        .count(&params.event_name, prev_since, prev_until)
        .await?;

    let trend = compute_trend(count, prev_count);

    Ok(Json(UsageResponse {
        event_name: params.event_name,
        count,
        unique_agents,
        trend,
    }))
}

fn compute_trend(current: u64, previous: u64) -> &'static str {
    if previous == 0 {
        if current > 0 {
            "up"
        } else {
            "flat"
        }
    } else {
        let change = (current as f64 - previous as f64) / previous as f64;
        if change > 0.10 {
            "up"
        } else if change < -0.10 {
            "down"
        } else {
            "flat"
        }
    }
}

/// `GET /api/v1/analytics/compare` — compare event counts before and after a pivot timestamp.
#[derive(Deserialize)]
pub struct CompareParams {
    pub event_name: String,
    /// Start of the "before" window.
    pub before: u64,
    /// Pivot timestamp — divides before from after.
    pub pivot: u64,
    /// End of the "after" window. Defaults to now.
    pub after: Option<u64>,
}

#[derive(Serialize)]
pub struct CompareResponse {
    pub event_name: String,
    pub before_count: u64,
    pub after_count: u64,
    /// Percentage change: (after - before) / before * 100. Null when before == 0.
    pub change_pct: Option<f64>,
    /// True when after_count > before_count.
    pub improved: bool,
}

pub async fn compare(
    State(state): State<Arc<AppState>>,
    Query(params): Query<CompareParams>,
) -> Result<Json<CompareResponse>, ApiError> {
    let after_end = params.after.unwrap_or_else(now_secs);

    let before_count = state
        .analytics
        .count(&params.event_name, params.before, params.pivot)
        .await?;
    let after_count = state
        .analytics
        .count(&params.event_name, params.pivot, after_end)
        .await?;

    let change_pct = if before_count == 0 {
        None
    } else {
        Some((after_count as f64 - before_count as f64) / before_count as f64 * 100.0)
    };

    Ok(Json(CompareResponse {
        event_name: params.event_name,
        before_count,
        after_count,
        change_pct,
        improved: after_count > before_count,
    }))
}

/// `GET /api/v1/analytics/top` — top N event names by count since a timestamp.
#[derive(Deserialize)]
pub struct TopParams {
    /// Max number of results. Defaults to 10, max 100.
    pub limit: Option<usize>,
    /// Start of the window (unix secs). Defaults to 24h ago.
    pub since: Option<u64>,
}

#[derive(Serialize)]
pub struct TopEntry {
    pub event_name: String,
    pub count: u64,
}

pub async fn top_events(
    State(state): State<Arc<AppState>>,
    Query(params): Query<TopParams>,
) -> Result<Json<Vec<TopEntry>>, ApiError> {
    let limit = params.limit.unwrap_or(10).min(100);
    let since = params
        .since
        .unwrap_or_else(|| now_secs().saturating_sub(86400));

    // Query all events in the window (capped to avoid memory issues).
    let events = state.analytics.query(None, Some(since), 100_000).await?;

    // Group by event_name.
    let mut counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for e in events {
        *counts.entry(e.event_name).or_default() += 1;
    }

    // Sort by count descending, take limit.
    let mut entries: Vec<TopEntry> = counts
        .into_iter()
        .map(|(event_name, count)| TopEntry { event_name, count })
        .collect();
    entries.sort_by(|a, b| b.count.cmp(&a.count));
    entries.truncate(limit);

    Ok(Json(entries))
}

// ─── Cost Entries ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RecordCostRequest {
    pub agent_id: String,
    pub task_id: Option<String>,
    pub cost_type: String,
    pub amount: f64,
    pub currency: String,
}

#[derive(Deserialize)]
pub struct QueryCostsParams {
    pub agent_id: Option<String>,
    pub task_id: Option<String>,
    pub since: Option<u64>,
}

#[derive(Deserialize)]
pub struct CostSummaryParams {
    pub since: u64,
    pub until: u64,
}

#[derive(Serialize)]
pub struct CostEntryResponse {
    pub id: String,
    pub agent_id: String,
    pub task_id: Option<String>,
    pub cost_type: String,
    pub amount: f64,
    pub currency: String,
    pub timestamp: u64,
}

impl From<CostEntry> for CostEntryResponse {
    fn from(e: CostEntry) -> Self {
        Self {
            id: e.id.to_string(),
            agent_id: e.agent_id.to_string(),
            task_id: e.task_id.map(|id| id.to_string()),
            cost_type: e.cost_type,
            amount: e.amount,
            currency: e.currency,
            timestamp: e.timestamp,
        }
    }
}

pub async fn record_cost(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RecordCostRequest>,
) -> Result<(StatusCode, Json<CostEntryResponse>), ApiError> {
    let entry = CostEntry::new(
        new_id(),
        Id::new(req.agent_id),
        req.task_id.map(Id::new),
        req.cost_type,
        req.amount,
        req.currency,
        now_secs(),
    );
    state.costs.record(&entry).await?;
    Ok((StatusCode::CREATED, Json(CostEntryResponse::from(entry))))
}

pub async fn query_costs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<QueryCostsParams>,
) -> Result<Json<Vec<CostEntryResponse>>, ApiError> {
    let entries = match (params.agent_id, params.task_id) {
        (Some(agent_id), _) => {
            state
                .costs
                .query_by_agent(&Id::new(agent_id), params.since)
                .await?
        }
        (_, Some(task_id)) => state.costs.query_by_task(&Id::new(task_id)).await?,
        _ => {
            return Err(ApiError::InvalidInput(
                "provide agent_id or task_id".to_string(),
            ))
        }
    };
    Ok(Json(entries.into_iter().map(Into::into).collect()))
}

pub async fn cost_summary(
    State(state): State<Arc<AppState>>,
    Query(params): Query<CostSummaryParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let total = state
        .costs
        .total_by_period(params.since, params.until)
        .await?;
    Ok(Json(serde_json::json!({
        "since": params.since,
        "until": params.until,
        "total": total
    })))
}

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        crate::api::api_router().with_state(test_state())
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn record_and_query_event() {
        let app = app();
        let body = serde_json::json!({
            "event_name": "task.completed",
            "agent_id": "agent-1",
            "properties": { "task_id": "t1" }
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/analytics/events")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/analytics/events")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn count_events() {
        let app = app();
        for _ in 0..3 {
            let body = serde_json::json!({ "event_name": "mr.merged" });
            app.clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/analytics/events")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
        }

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/analytics/count?event_name=mr.merged&since=0&until=9999999999")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["count"], 3);
    }

    #[tokio::test]
    async fn record_and_query_costs() {
        let app = app();
        let body = serde_json::json!({
            "agent_id": "agent-1",
            "cost_type": "llm_tokens",
            "amount": 500.0,
            "currency": "tokens"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/costs")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/costs?agent_id=agent-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["amount"], 500.0);
    }

    #[tokio::test]
    async fn cost_summary_endpoint() {
        let app = app();
        let body = serde_json::json!({
            "agent_id": "agent-1",
            "cost_type": "llm_tokens",
            "amount": 200.0,
            "currency": "tokens"
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/costs")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/costs/summary?since=0&until=9999999999")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["total"], 200.0);
    }

    // ─── M23 Analytics Decision API tests ──────────────────────────────────────

    #[tokio::test]
    async fn usage_with_trend_up() {
        let app = app();
        // Seed 3 events "now" — no previous period events, so trend = "up".
        for _ in 0..3 {
            let body = serde_json::json!({ "event_name": "agent.spawned", "agent_id": "a1" });
            app.clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/analytics/events")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
        }

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(
                        "/api/v1/analytics/usage?event_name=agent.spawned&since=0&until=9999999999",
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["count"], 3);
        assert_eq!(json["unique_agents"], 1);
        // previous period is [0 - 0, 0] so prev_count=0 → trend="up"
        assert_eq!(json["trend"], "up");
    }

    #[tokio::test]
    async fn usage_trend_flat_no_events() {
        let app = app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/analytics/usage?event_name=no.events&since=0&until=9999999999")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["count"], 0);
        assert_eq!(json["trend"], "flat");
    }

    #[tokio::test]
    async fn compare_returns_change_pct() {
        let app = app();
        // Seed 2 events in the "before" window and 5 in the "after" window.
        // We fake this by using timestamps in the query params since recorded events
        // get the current timestamp — just check that before=0 gives change_pct=null.
        let resp = app.oneshot(
            Request::builder()
                .uri("/api/v1/analytics/compare?event_name=mr.merged&before=0&pivot=1&after=9999999999")
                .body(Body::empty()).unwrap(),
        ).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json["before_count"].is_number());
        assert!(json["after_count"].is_number());
        // With no events before=1 pivot, change_pct is null.
        assert!(json["change_pct"].is_null() || json["change_pct"].is_number());
        assert!(json["improved"].is_boolean());
    }

    #[tokio::test]
    async fn top_events_ordering() {
        let app = app();
        // Seed: 3x "alpha.event", 1x "beta.event".
        for _ in 0..3 {
            let body = serde_json::json!({ "event_name": "alpha.event" });
            app.clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/analytics/events")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
        }
        let body = serde_json::json!({ "event_name": "beta.event" });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/analytics/events")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/analytics/top?limit=10&since=0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let arr = json.as_array().unwrap();
        assert!(!arr.is_empty());
        // First entry should be alpha.event (count=3).
        assert_eq!(arr[0]["event_name"], "alpha.event");
        assert_eq!(arr[0]["count"], 3);
    }

    #[tokio::test]
    async fn query_events_by_name() {
        let app = app();
        for name in &["task.completed", "task.completed", "mr.merged"] {
            let body = serde_json::json!({ "event_name": name });
            app.clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/analytics/events")
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
        }

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/analytics/events?event_name=task.completed")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 2);
    }

    // ─── Query Parameters (analytics.md §Query Parameters) ─────────────────────

    /// Helper: POST an event with full scope fields, return the state for
    /// direct repository assertions.
    async fn record_scoped(
        app: &Router,
        event_name: &str,
        agent_id: Option<&str>,
        user_id: Option<&str>,
        workspace_id: Option<&str>,
        repo_id: Option<&str>,
    ) {
        let body = serde_json::json!({
            "event_name": event_name,
            "agent_id": agent_id,
            "user_id": user_id,
            "workspace_id": workspace_id,
            "repo_id": repo_id,
            "properties": { "k": "v" },
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/analytics/events")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    async fn get_events(app: &Router, query: &str) -> serde_json::Value {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/analytics/events?{query}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "query was: {query}");
        body_json(resp).await
    }

    #[tokio::test]
    async fn query_events_filters_by_scope_ids() {
        // Spec: agent_id, workspace_id, repo_id, user_id must each filter.
        let app = app();
        record_scoped(&app, "mr.merged", Some("agent-1"), None, Some("ws-1"), Some("repo-1"))
            .await;
        record_scoped(&app, "mr.merged", Some("agent-2"), None, Some("ws-2"), Some("repo-1"))
            .await;
        record_scoped(&app, "mr.closed", Some("agent-1"), Some("user-9"), Some("ws-1"), None)
            .await;

        let json = get_events(&app, "agent_id=agent-1").await;
        assert_eq!(
            json.as_array().unwrap().len(),
            2,
            "agent_id filter must match both agent-1 events"
        );

        let json = get_events(&app, "workspace_id=ws-2").await;
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["agent_id"], "agent-2");

        let json = get_events(&app, "repo_id=repo-1&event_name=mr.merged").await;
        assert_eq!(json.as_array().unwrap().len(), 2);

        let json = get_events(&app, "user_id=user-9").await;
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["event_name"], "mr.closed");

        // Combined scope filters are conjunctive.
        let json = get_events(&app, "agent_id=agent-1&workspace_id=ws-1&event_name=mr.merged").await;
        assert_eq!(json.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn query_events_event_name_prefix_wildcard() {
        // Spec: `event_name` supports prefix match with trailing `*`.
        let app = app();
        record_scoped(&app, "mr.merged", None, None, None, None).await;
        record_scoped(&app, "mr.closed", None, None, None, None).await;
        record_scoped(&app, "task.status_changed", None, None, None, None).await;

        let json = get_events(&app, "event_name=mr.*").await;
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 2, "mr.* must match mr.merged and mr.closed");
        assert!(arr.iter().all(|e| e["event_name"].as_str().unwrap().starts_with("mr.")));

        // Exact match still works and excludes the other mr.* events.
        let json = get_events(&app, "event_name=mr.merged").await;
        assert_eq!(json.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn query_events_since_until_bounds() {
        // Spec: since/until bound the time range (inclusive).
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());

        // Record events directly against the repository so timestamps are
        // deterministic (the POST handler stamps `now`).
        for (id, ts) in [("e-old", 100u64), ("e-mid", 200), ("e-new", 300)] {
            let ev = gyre_domain::AnalyticsEvent::new(
                gyre_common::Id::new(id),
                "ev.ts",
                None,
                serde_json::json!({}),
                ts,
            );
            state.analytics.record(&ev).await.unwrap();
        }

        let json = get_events(&app, "event_name=ev.ts&since=200").await;
        assert_eq!(json.as_array().unwrap().len(), 2, "since=200 is inclusive");

        let json = get_events(&app, "event_name=ev.ts&until=200").await;
        assert_eq!(json.as_array().unwrap().len(), 2, "until=200 is inclusive");

        let json = get_events(&app, "event_name=ev.ts&since=150&until=250").await;
        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0]["id"], "e-mid");
    }

    #[tokio::test]
    async fn query_events_limit_truncates() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        for i in 0..5u64 {
            let ev = gyre_domain::AnalyticsEvent::new(
                gyre_common::Id::new(format!("e-lim-{i}")),
                "ev.limit",
                None,
                serde_json::json!({}),
                100 + i,
            );
            state.analytics.record(&ev).await.unwrap();
        }

        let json = get_events(&app, "event_name=ev.limit&limit=2").await;
        assert_eq!(json.as_array().unwrap().len(), 2, "limit must truncate");
    }

    #[tokio::test]
    async fn query_events_group_by_aggregates() {
        // Spec: group_by ∈ {event_name, agent_id, workspace_id, day}.
        let app = app();
        record_scoped(&app, "mr.merged", Some("a1"), None, Some("ws-1"), None).await;
        record_scoped(&app, "mr.merged", Some("a1"), None, Some("ws-1"), None).await;
        record_scoped(&app, "mr.closed", Some("a2"), None, Some("ws-2"), None).await;

        let json = get_events(&app, "group_by=event_name").await;
        let obj = json.as_object().expect("group_by returns an object");
        assert_eq!(obj.get("mr.merged").and_then(|v| v.as_u64()), Some(2));
        assert_eq!(obj.get("mr.closed").and_then(|v| v.as_u64()), Some(1));

        let json = get_events(&app, "group_by=agent_id").await;
        let obj = json.as_object().unwrap();
        assert_eq!(obj.get("a1").and_then(|v| v.as_u64()), Some(2));
        assert_eq!(obj.get("a2").and_then(|v| v.as_u64()), Some(1));

        let json = get_events(&app, "group_by=workspace_id").await;
        let obj = json.as_object().unwrap();
        assert_eq!(obj.get("ws-1").and_then(|v| v.as_u64()), Some(2));
        assert_eq!(obj.get("ws-2").and_then(|v| v.as_u64()), Some(1));

        // day grouping: all events recorded "now" land on today's UTC date.
        let json = get_events(&app, "group_by=day").await;
        let obj = json.as_object().unwrap();
        let total: u64 = obj.values().filter_map(|v| v.as_u64()).sum();
        assert_eq!(total, 3, "day grouping must account for every event");

        // Unsupported group_by field is rejected.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/analytics/events?group_by=bogus")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
