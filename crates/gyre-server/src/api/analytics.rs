use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{AnalyticsEvent, CostEntry};
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
    pub properties: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct QueryEventsParams {
    pub event_name: Option<String>,
    pub since: Option<u64>,
    pub limit: Option<usize>,
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
    pub properties: serde_json::Value,
    pub timestamp: u64,
}

impl From<AnalyticsEvent> for AnalyticsEventResponse {
    fn from(e: AnalyticsEvent) -> Self {
        Self {
            id: e.id.to_string(),
            event_name: e.event_name,
            agent_id: e.agent_id,
            properties: e.properties,
            timestamp: e.timestamp,
        }
    }
}

#[derive(Serialize)]
pub struct DayCount {
    pub date: String,
    pub count: u64,
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
) -> Result<Json<Vec<AnalyticsEventResponse>>, ApiError> {
    let limit = params.limit.unwrap_or(100).min(1000);
    let events = state
        .analytics
        .query(params.event_name.as_deref(), params.since, limit)
        .await?;
    Ok(Json(events.into_iter().map(Into::into).collect()))
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
}
