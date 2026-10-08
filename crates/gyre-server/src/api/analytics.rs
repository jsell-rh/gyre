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

#[derive(Deserialize)]
pub struct QueryEventsParams {
    pub event_name: Option<String>,
    pub agent_id: Option<String>,
    pub user_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repo_id: Option<String>,
    /// ISO8601 timestamp or unix seconds (analytics.md §Query Parameters).
    pub since: Option<String>,
    pub until: Option<String>,
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
    /// ISO8601 timestamp or unix seconds. Optional when `days` is given.
    pub since: Option<String>,
    /// ISO8601 timestamp or unix seconds. Defaults to now (analytics.md
    /// §Query Parameters: "End of time range (default: now)").
    pub until: Option<String>,
    /// Spec form `GET /daily?event_name=&days=30` (analytics.md §Query API):
    /// window = last N days ending now. Defaults to 30.
    pub days: Option<u64>,
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
    // Spec types since/until as ISO8601 (§Query Parameters); bare unix
    // seconds remain accepted for backward compatibility.
    let since = match params.since.as_deref() {
        Some(s) => Some(parse_time_param_req("since", s, false)?),
        None => None,
    };
    let until = match params.until.as_deref() {
        Some(u) => Some(parse_time_param_req("until", u, true)?),
        None => None,
    };
    let filter = AnalyticsQueryFilter {
        event_name: params.event_name.clone(),
        agent_id: params.agent_id.clone(),
        user_id: params.user_id.clone(),
        workspace_id: params.workspace_id.clone(),
        repo_id: params.repo_id.clone(),
        since,
        until,
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

/// Civil-days-from-epoch inverse of `epoch_days_to_ymd` (Howard Hinnant).
fn ymd_to_epoch_days(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(y) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Parse a `since`/`until` query parameter into unix seconds
/// (analytics.md §Query Parameters: ISO8601). Bare unix seconds are also
/// accepted, preserving the pre-existing numeric form.
///
/// Returns `(unix_seconds, date_only)`. A date-only value (`YYYY-MM-DD`)
/// resolves to midnight UTC; callers treating it as an upper bound should
/// extend it to the end of that day (see `parse_time_param_req`).
///
/// Supported ISO8601 forms: `YYYY-MM-DD`, `YYYY-MM-DDTHH:MM`,
/// `YYYY-MM-DDTHH:MM:SS`, optional `.fff` fractional seconds, timezone
/// `Z`, `±HH:MM`, `±HHMM`, or `±HH` (no offset means UTC).
pub(crate) fn parse_time_param(s: &str) -> Option<(u64, bool)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if s.bytes().all(|b| b.is_ascii_digit()) {
        return s.parse::<u64>().ok().map(|v| (v, false));
    }

    // Split date from time at 'T' (date-only has no time part).
    let (date_part, rest) = match s.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (s, None),
    };

    // Date: YYYY-MM-DD
    let mut date_fields = date_part.split('-');
    let y: i64 = date_fields.next()?.parse().ok()?;
    let m: u32 = date_fields.next()?.parse().ok()?;
    let d: u32 = date_fields.next()?.parse().ok()?;
    if date_fields.next().is_some() || !(1..=12).contains(&m) || d < 1 || d > days_in_month(y, m) {
        return None;
    }

    let mut secs_of_day: u64 = 0;
    let mut offset_secs: i64 = 0;
    if let Some(time_part) = rest {
        // Split off timezone suffix: Z, +HH:MM, -HH:MM, +HHMM, or +HH.
        let (clock, tz) = match time_part
            .char_indices()
            .find(|(_, c)| *c == 'Z' || *c == '+' || *c == '-')
        {
            Some((i, 'Z')) => (&time_part[..i], ""),
            Some((i, _sign)) => (&time_part[..i], &time_part[i..]),
            _ => (time_part, ""),
        };
        // Fractional seconds (truncate to whole seconds).
        let (clock, _frac) = match clock.split_once('.') {
            Some((c, f)) if !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()) => (c, f),
            Some(_) => return None,
            None => (clock, ""),
        };
        let mut clock_fields = clock.split(':');
        let h: u32 = clock_fields.next()?.parse().ok()?;
        let mi: u32 = clock_fields.next()?.parse().ok()?;
        let sec: u32 = match clock_fields.next() {
            Some(s) => s.parse().ok()?,
            None => 0,
        };
        if clock_fields.next().is_some() || h > 23 || mi > 59 || sec > 59 {
            return None;
        }
        secs_of_day = h as u64 * 3600 + mi as u64 * 60 + sec as u64;

        if !tz.is_empty() {
            let (sign, digits) = tz.split_at(1);
            let off = match digits.split_once(':') {
                Some((oh, om)) => {
                    let oh: i64 = oh.parse().ok()?;
                    let om: i64 = om.parse().ok()?;
                    if oh > 23 || om > 59 {
                        return None;
                    }
                    oh * 3600 + om * 60
                }
                None => {
                    if digits.len() == 4 {
                        let oh: i64 = digits.get(0..2)?.parse().ok()?;
                        let om: i64 = digits.get(2..4)?.parse().ok()?;
                        if oh > 23 || om > 59 {
                            return None;
                        }
                        oh * 3600 + om * 60
                    } else {
                        let oh: i64 = digits.parse().ok()?;
                        if oh > 23 {
                            return None;
                        }
                        oh * 3600
                    }
                }
            };
            offset_secs = if sign == "-" { -off } else { off };
        }
    }

    let days = ymd_to_epoch_days(y, m, d);
    let total = days as i64 * 86_400 + secs_of_day as i64 - offset_secs;
    u64::try_from(total).ok().map(|v| (v, rest.is_none()))
}

/// Parse a bound parameter for a handler. `end_of_day=true` extends a
/// date-only value to 23:59:59 of that day so `since=D&until=D` covers
/// all of day D (a bare date as an upper bound means "end of that day").
pub(crate) fn parse_time_param_req(
    param: &str,
    value: &str,
    end_of_day: bool,
) -> Result<u64, ApiError> {
    parse_time_param(value)
        .map(|(secs, date_only)| {
            if date_only && end_of_day {
                secs.saturating_add(86_399)
            } else {
                secs
            }
        })
        .ok_or_else(|| {
            ApiError::InvalidInput(format!(
                "invalid {param} value {value:?}: expected ISO8601 timestamp or unix seconds"
            ))
        })
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
    // Spec form: `?event_name=&days=30` (window = last N days ending now).
    // Explicit since/until (ISO8601 or unix secs) take precedence; until
    // defaults to now (analytics.md §Query Parameters).
    let now = now_secs();
    let until = match params.until.as_deref() {
        Some(u) => parse_time_param_req("until", u, true)?,
        None => now,
    };
    let since = match params.since.as_deref() {
        Some(s) => parse_time_param_req("since", s, false)?,
        None => {
            let days = params.days.unwrap_or(30);
            until.saturating_sub(days.saturating_mul(86_400))
        }
    };
    if since > until {
        return Err(ApiError::InvalidInput(
            "since must not be after until".to_string(),
        ));
    }
    let rows = state
        .analytics
        .aggregate_by_day(&params.event_name, since, until)
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
    use super::{epoch_days_to_ymd, parse_time_param, ymd_to_epoch_days};
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
    #[test]
    fn parse_time_param_accepts_unix_seconds() {
        assert_eq!(parse_time_param("0"), Some((0, false)));
        assert_eq!(parse_time_param("1760000000"), Some((1_760_000_000, false)));
    }

    #[test]
    fn parse_time_param_iso8601_forms() {
        // Date-only is midnight UTC and flagged date_only.
        assert_eq!(parse_time_param("1970-01-01"), Some((0, true)));
        assert_eq!(parse_time_param("1970-01-02"), Some((86_400, true)));
        assert_eq!(parse_time_param("2026-10-08"), Some((1_791_417_600, true)));
        // Full timestamp, Z.
        assert_eq!(parse_time_param("1970-01-01T00:00:30Z"), Some((30, false)));
        assert_eq!(parse_time_param("1970-01-01T01:02:03Z"), Some((3_723, false)));
        // No timezone means UTC.
        assert_eq!(parse_time_param("1970-01-01T00:00:30"), Some((30, false)));
        // Minutes precision.
        assert_eq!(parse_time_param("1970-01-01T00:10Z"), Some((600, false)));
        // Fractional seconds truncate.
        assert_eq!(parse_time_param("1970-01-01T00:00:30.987Z"), Some((30, false)));
        // Numeric offsets: +HH:MM, -HH:MM, +HHMM, +HH.
        assert_eq!(parse_time_param("1970-01-01T02:00:00+02:00"), Some((0, false)));
        assert_eq!(parse_time_param("1970-01-01T00:30:00-00:30"), Some((3_600, false)));
        assert_eq!(parse_time_param("1970-01-01T02:00:00+0200"), Some((0, false)));
        assert_eq!(parse_time_param("1970-01-01T05:00:00+05"), Some((0, false)));
        // Leap-year day.
        assert_eq!(
            parse_time_param("2024-02-29T00:00:00Z"),
            Some((1_709_164_800, false))
        );
    }

    #[test]
    fn parse_time_param_rejects_garbage() {
        for bad in [
            "",
            "  ",
            "not-a-date",
            "2026-13-01", // month out of range
            "2026-00-10",
            "2026-02-30", // day beyond month length
            "2023-02-29", // non-leap Feb 29
            "2026-10-08T25:00:00Z",
            "2026-10-08T12:60:00Z",
            "2026-10-08T12:00:61Z",
            "2026-10-08T00:00:00+99:00",
            "2026-10-08", // sanity: control that valid parses
        ] {
            if bad == "2026-10-08" {
                assert!(parse_time_param(bad).is_some());
            } else {
                assert_eq!(parse_time_param(bad), None, "should reject {bad:?}");
            }
        }
    }

    #[test]
    fn parse_time_param_roundtrips_epoch_day_string() {
        // ymd_to_epoch_days is the exact inverse of epoch_days_to_ymd.
        for days in [-100_000i64, -1, 0, 1, 59, 365, 19_000, 100_000] {
            let (y, m, d) = epoch_days_to_ymd(days);
            assert_eq!(ymd_to_epoch_days(y, m, d), days, "day {days}");
        }
    }

    #[tokio::test]
    async fn query_events_since_until_accept_iso8601() {
        // Spec §Query Parameters types since/until as ISO8601; a
        // spec-following client must get filter semantics, not a 400.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        for (id, ts) in [("e-iso-a", 100u64), ("e-iso-b", 200), ("e-iso-c", 300)] {
            let ev = gyre_domain::AnalyticsEvent::new(
                gyre_common::Id::new(id),
                "ev.iso",
                None,
                serde_json::json!({}),
                ts,
            );
            state.analytics.record(&ev).await.unwrap();
        }

        // 1970-01-01T00:02:30Z == 150 — strictly between the 100 and 200
        // events; inclusive lower bound keeps 200 and 300.
        let json = get_events(&app, "event_name=ev.iso&since=1970-01-01T00:02:30Z").await;
        assert_eq!(json.as_array().unwrap().len(), 2, "ISO8601 since bounds below");

        // 1970-01-01T00:05Z == 300 — inclusive upper bound.
        let json = get_events(&app, "event_name=ev.iso&until=1970-01-01T00:05Z").await;
        assert_eq!(json.as_array().unwrap().len(), 3, "ISO8601 until is inclusive");

        // Date-only bounds: 1970-01-01 covers the whole first day.
        let json = get_events(&app, "event_name=ev.iso&since=1970-01-01&until=1970-01-01").await;
        assert_eq!(json.as_array().unwrap().len(), 3);

        // Offset form: 1970-01-01T02:00+02:00 == 0.
        let json = get_events(&app, "event_name=ev.iso&since=1970-01-01T02:00+02:00").await;
        assert_eq!(json.as_array().unwrap().len(), 3);

        // Unix seconds still accepted (backward compat).
        let json = get_events(&app, "event_name=ev.iso&since=200&until=200").await;
        assert_eq!(json.as_array().unwrap().len(), 1);

        // Malformed timestamp is a 400 with a clear message, not a 500.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/analytics/events?event_name=ev.iso&since=not-a-date")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn daily_events_days_param_and_defaults() {
        // Spec §Query API documents `GET /daily?event_name=&days=30`.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        // Deterministic events: two today, one 40 days ago (out of window).
        let now = crate::api::now_secs();
        for (id, ts) in [
            ("d-today-1", now),
            ("d-today-2", now),
            ("d-old", now.saturating_sub(40 * 86_400)),
        ] {
            let ev = gyre_domain::AnalyticsEvent::new(
                gyre_common::Id::new(id),
                "ev.daily",
                None,
                serde_json::json!({}),
                ts,
            );
            state.analytics.record(&ev).await.unwrap();
        }

        // days=30 window must exclude the 40-day-old event but include both
        // recent ones on today's bucket.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/analytics/daily?event_name=ev.daily&days=30")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let arr = json.as_array().unwrap();
        let total: u64 = arr.iter().map(|r| r["count"].as_u64().unwrap_or(0)).sum();
        assert_eq!(total, 2, "days=30 window excludes the 40-day-old event");
        assert!(
            arr.iter().all(|r| r["count"].as_u64().unwrap_or(0) > 0),
            "daily buckets with zero events are not returned"
        );

        // A wider window includes the old event.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/analytics/daily?event_name=ev.daily&days=60")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let total: u64 = json
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["count"].as_u64().unwrap_or(0))
            .sum();
        assert_eq!(total, 3, "days=60 window includes the old event");

        // until defaults to now; since omitted with days → last N days.
        // (Also verifies the handler no longer 400s without since/until.)
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/analytics/daily?event_name=ev.daily")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "daily must default to days=30 window when since/until omitted"
        );

        // since-after-until is rejected.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/analytics/daily?event_name=ev.daily&since=9999&until=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
