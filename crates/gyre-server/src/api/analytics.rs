use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{
    decisions::{format_iso8601_utc, BuiltinDecision, CustomDecisionSpec, DecisionResult},
    evaluate_custom, AnalyticsEvent, CostEntry,
};
use gyre_ports::analytics::{AnalyticsGroupBy, AnalyticsQueryFilter};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{auth::AuthenticatedAgent, AppState};

use super::error::ApiError;
use super::{new_id, now_secs};

// ─── Analytics Events ────────────────────────────────────────────────────────

/// Inbound analytics event (client-side push). The identity fields
/// `agent_id`/`user_id` are accepted for backward compatibility but
/// IGNORED — the caller identity from auth is always recorded (task-146
/// review F1, same anti-forgery rule as audit events / NEW-31).
/// `workspace_id`/`repo_id` are validated against the caller's tenant;
/// cross-tenant or unknown scope attribution rejects the write.
#[derive(Deserialize)]
pub struct RecordEventRequest {
    pub event_name: String,
    /// Ignored: derived from the verified caller identity.
    pub agent_id: Option<String>,
    /// Ignored: derived from the verified caller identity.
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    /// Validated against the caller's tenant; mismatch or unknown id
    /// rejects the write (Forbidden/NotFound).
    pub workspace_id: Option<String>,
    /// Validated against the caller's tenant (and against `workspace_id`
    /// when both are given); mismatch or unknown id rejects the write.
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
    /// `event_name`, `agent_id`, `workspace_id`, `day`. Aggregated
    /// server-side over the full filtered set; `limit` bounds the number
    /// of returned groups (task-146 review F1).
    pub group_by: Option<String>,
}

// ─── Auto-emit helpers ────────────────────────────────────────────────────────

/// Record the spec-required `task.status_changed` analytics event
/// (analytics.md §Auto-Emitted Events: trigger "Task transitions status",
/// properties `old_status`, `new_status`, `task_id`, `assigned_to`).
///
/// Every production path that persists a task status transition must call
/// this — the HTTP status endpoint and the MCP `update_task` tool are the
/// explicit paths; internal auto-advances (agent spawn/completion, admin
/// force-kill, stale-agent abort, spec rejection, repo archival) are the
/// rest. `old_status` is the status before the transition just persisted.
pub(crate) async fn record_task_status_changed(
    state: &AppState,
    task: &gyre_domain::Task,
    old_status: &gyre_domain::TaskStatus,
) {
    let ev = AnalyticsEvent::new(
        new_id(),
        "task.status_changed",
        task.assigned_to.as_ref().map(|id| id.to_string()),
        serde_json::json!({
            "task_id": task.id.to_string(),
            "old_status": format!("{:?}", old_status),
            "new_status": format!("{:?}", task.status),
            "assigned_to": task.assigned_to.as_ref().map(|id| id.to_string()),
        }),
        now_secs(),
    )
    // task-202/task-146 repair: the scope must carry the repo facet too —
    // search.md §Searchable Entities makes activity events filterable by
    // workspace AND repo, and Task.repo_id is non-optional (M34 hierarchy),
    // so it is derived from the persisted entity, never caller-supplied.
    .with_scope(None, None, Some(&task.workspace_id), Some(&task.repo_id));
    let _ = state.analytics.record(&ev).await;
}

/// Record the spec-required `agent.failed` analytics event (analytics.md
/// §Auto-Emitted Events: trigger "Agent fails or is killed", properties
/// `agent_id`, `task_id`, `reason`).
///
/// Every production path that persists an agent death must call this:
/// the API fail/stop endpoints and MCP tools, the ralph-loop iteration
/// ceiling and terminal re-spawn failure, the worker scheduler's
/// failed-launch teardown, the orchestrator process-death monitor, and
/// the agent status endpoint (operator/orchestrator kill path).
/// `reason` names the concrete cause; `duration_secs` is derived from the
/// agent's spawn time (extra context, same shape as the endpoint paths).
pub(crate) async fn record_agent_failed(
    state: &AppState,
    agent: &gyre_domain::Agent,
    reason: &str,
) {
    let now = now_secs();
    let ev = AnalyticsEvent::new(
        new_id(),
        "agent.failed",
        Some(agent.id.to_string()),
        serde_json::json!({
            "task_id": agent.current_task_id.as_ref().map(|id| id.to_string()),
            "reason": reason,
            "duration_secs": now.saturating_sub(agent.spawned_at),
        }),
        now,
    )
    .with_scope(
        None,
        None,
        Some(&agent.workspace_id),
        agent.repo_id.as_ref(),
    );
    let _ = state.analytics.record(&ev).await;
}

/// Record the spec-required `search.query` analytics event (analytics.md
/// §Auto-Emitted Events: trigger "Full-text search executed", properties
/// `query_length`, `entity_types`, `result_count`, `duration_ms`).
///
/// Every production path that executes a full-text search must call this:
/// the HTTP search endpoint (api/search.rs) and the MCP `gyre_search`
/// tool (mcp.rs) are the two — the MCP tool is the autonomous-agent path,
/// both must record. Identity is bound to the verified caller, never to
/// client-supplied values, and `workspace_scope` must already be validated
/// against the caller's tenant by the call site because it is the event's
/// workspace attribution (task-146 review round 2 rule).
pub(crate) async fn record_search_query(
    state: &AppState,
    auth: &AuthenticatedAgent,
    q: &str,
    results: &[gyre_ports::search::SearchResult],
    duration_ms: u64,
    workspace_scope: Option<&Id>,
) {
    // Same caller-bound identity rule as `record_event`: agent JWT callers
    // carry the agent identity, user/API-key callers carry user_id, and
    // impersonation sessions attribute to the impersonator.
    let agent_id = if auth.agent_id.is_empty() {
        None
    } else {
        Some(auth.agent_id.clone())
    };
    let user_id = auth
        .impersonation
        .as_ref()
        .map(|imp| imp.impersonator_id.clone())
        .or_else(|| auth.user_id.clone());
    let mut entity_types: Vec<String> = results.iter().map(|r| r.entity_type.clone()).collect();
    entity_types.sort();
    entity_types.dedup();
    let ev = AnalyticsEvent::new(
        new_id(),
        "search.query",
        agent_id,
        serde_json::json!({
            "query_length": q.chars().count(),
            "entity_types": entity_types,
            "result_count": results.len(),
            "duration_ms": duration_ms,
        }),
        now_secs(),
    )
    .with_scope(user_id.as_ref(), None, workspace_scope, None);
    let _ = state.analytics.record(&ev).await;
}

#[derive(Deserialize)]
pub struct CountEventsParams {
    pub event_name: String,
    /// Start of the window (unix secs). Defaults to 0 (all time).
    pub since: Option<u64>,
    /// End of the window (unix secs). Defaults to now.
    pub until: Option<u64>,
    /// Optional property-equality filter `key:value` — counts only
    /// events whose `properties[key]` equals `value` (string equality).
    /// Used by the Analytics Dashboard funnel view (analytics.md
    /// §Analytics Dashboard): a funnel step like
    /// `task.status_changed(InProgress)` counts status-change events
    /// whose `new_status` property is `InProgress`.
    pub property: Option<String>,
}

#[derive(Deserialize)]
pub struct DailyParams {
    /// Exact event-name filter. Optional: absent or empty (`?event_name=`)
    /// aggregates across ALL event names — the total daily event volume
    /// the Analytics Dashboard bar chart renders (analytics.md
    /// §Analytics Dashboard: "daily event volume for the last 30 days").
    /// The bare `?days=30` form (what the dashboard's chart sends) MUST
    /// work — it is not an error to omit the filter.
    #[serde(default)]
    pub event_name: Option<String>,
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
    auth: AuthenticatedAgent,
    Json(req): Json<RecordEventRequest>,
) -> Result<(StatusCode, Json<AnalyticsEventResponse>), ApiError> {
    // Bind the identity fields to the verified caller (task-146 review F1):
    // the request-body agent_id/user_id fields are ignored — the recorded
    // event always reflects who actually made the call, not what the caller
    // claims, so an authenticated caller cannot attribute analytics events
    // to an arbitrary user or agent. Agent JWT callers carry an agent
    // identity; user/API-key callers carry user_id; impersonation sessions
    // attribute to the impersonator (identity-security.md §Impersonation) —
    // the same rule the audit trail enforces (NEW-31).
    let agent_id = if auth.agent_id.is_empty() {
        None
    } else {
        Some(auth.agent_id.clone())
    };
    let user_id = auth
        .impersonation
        .as_ref()
        .map(|imp| imp.impersonator_id.clone())
        .or_else(|| auth.user_id.clone());

    // Caller-scope validation (task-146 review F1, task-102 F2 pattern in
    // audit.rs): workspace/repo attribution must resolve within the
    // caller's tenant. These scope fields are exactly what the query API
    // filters on, so persisting a caller-forged value would pollute
    // another scope's filtered results and the agent-consumable decision
    // loop. Mismatch or unknown id rejects the write.
    let workspace_id = match req.workspace_id {
        Some(w) => {
            let id = Id::new(w);
            super::audit::validate_workspace_scope(&state, &id, &auth).await?;
            Some(id)
        }
        None => None,
    };
    let repo_id = match req.repo_id {
        Some(r) => {
            let id = Id::new(r);
            super::audit::validate_repo_scope(&state, &id, &auth, workspace_id.as_ref()).await?;
            Some(id)
        }
        None => None,
    };

    let event = AnalyticsEvent::new(
        new_id(),
        req.event_name,
        agent_id,
        req.properties
            .unwrap_or(serde_json::Value::Object(Default::default())),
        now_secs(),
    )
    .with_scope(
        user_id.as_ref(),
        req.session_id,
        workspace_id.as_ref(),
        repo_id.as_ref(),
    );
    state.analytics.record(&event).await?;
    // search.md §Searchable Entities: activity events are indexed on write
    // (description, event_type, agent_id facets).
    crate::search_indexer::index(
        &state,
        crate::search_indexer::activity_event_document(&event),
    )
    .await;
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
    // group_by aggregation (analytics.md §Query Parameters): event_name,
    // agent_id, workspace_id, day. Falls back to raw event list.
    let group_by = match params.group_by.as_deref() {
        Some("event_name") => Some(AnalyticsGroupBy::EventName),
        Some("agent_id") => Some(AnalyticsGroupBy::AgentId),
        Some("workspace_id") => Some(AnalyticsGroupBy::WorkspaceId),
        Some("day") => Some(AnalyticsGroupBy::Day),
        Some(other) => {
            return Err(ApiError::InvalidInput(format!(
                "unsupported group_by field: {other} (expected event_name, agent_id, workspace_id, day)"
            )))
        }
        None => None,
    };
    if let Some(group_by) = group_by {
        // task-146 review F1: grouped counts aggregate the FULL filtered
        // set server-side (port-level GROUP BY, like aggregate_by_day) —
        // `limit` bounds the number of returned groups, never the input
        // to the aggregation, so grouped totals stay consistent with
        // /count instead of counting only the newest `limit` events.
        let rows = state.analytics.count_grouped(&filter, group_by).await?;
        let counts: serde_json::Map<String, serde_json::Value> = rows
            .into_iter()
            .map(|(k, v)| (k, serde_json::Value::from(v)))
            .collect();
        return Ok(Json(serde_json::Value::Object(counts)));
    }

    let events = state.analytics.query_filtered(&filter).await?;

    Ok(Json(serde_json::Value::Array(
        events
            .into_iter()
            .map(|e| serde_json::to_value(e).unwrap_or_default())
            .collect(),
    )))
}

/// Civil-from-days algorithm (Howard Hinnant) — same math as mem.rs.
/// Test-only: the inverse used to round-trip-check the production
/// `ymd_to_epoch_days` in `parse_time_param_roundtrips_epoch_day_string`.
#[cfg(test)]
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
    let total = days * 86_400 + secs_of_day as i64 - offset_secs;
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
    let until = params.until.unwrap_or_else(now_secs);
    let since = params.since.unwrap_or(0);
    if since > until {
        return Err(ApiError::InvalidInput(
            "since must not be after until".to_string(),
        ));
    }
    let count = match params.property.as_deref() {
        None => {
            state
                .analytics
                .count(&params.event_name, since, until)
                .await?
        }
        Some(filter) => {
            // Property filters are evaluated in Rust over the filtered
            // event set (the storage layer has no properties index) —
            // same fetch cap as the /top ranking.
            let (key, value) = filter.split_once(':').ok_or_else(|| {
                ApiError::InvalidInput(
                    "property filter must be 'key:value' (e.g. new_status:InProgress)".to_string(),
                )
            })?;
            let filter = AnalyticsQueryFilter {
                event_name: Some(params.event_name.clone()),
                since: Some(since),
                until: Some(until),
                limit: 100_000,
                ..Default::default()
            };
            let events = state.analytics.query_filtered(&filter).await?;
            events
                .iter()
                .filter(|e| e.properties.get(key).and_then(|v| v.as_str()) == Some(value))
                .count() as u64
        }
    };
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
    // Empty string (`?event_name=`) is the spec's "no filter" form, same
    // as omitting the parameter entirely — the dashboard's bar chart
    // sends the bare `?days=30` form and must get TOTAL daily volume.
    let event_name = params
        .event_name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty());
    let rows = state
        .analytics
        .aggregate_by_day(event_name, since, until)
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

/// `GET /api/v1/analytics/top` — top N event names by count since a
/// timestamp (default), or — with `?dimension=agent` — the top-agents
/// ranking the Analytics Dashboard renders (analytics.md §Analytics
/// Dashboard "Top agents"): agents ranked by events generated, with task
/// completion rate and average completion duration.
#[derive(Deserialize)]
pub struct TopParams {
    /// Max number of results. Defaults to 10, max 100.
    pub limit: Option<usize>,
    /// Start of the window (unix secs). Defaults to 24h ago (event
    /// dimension) or 30d ago (agent dimension, matching the dashboard's
    /// "Top agents — since 30d-ago" fetch).
    pub since: Option<u64>,
    /// Ranking dimension: `event_name` (default) or `agent`.
    pub dimension: Option<String>,
    /// Agent dimension only: restrict the ranking to one workspace.
    /// Validated against the caller's tenant — the same anti-forgery
    /// rule as `decide` (a forged id would leak another tenant's
    /// aggregated agent stats).
    pub workspace_id: Option<String>,
}

#[derive(Serialize)]
pub struct TopEntry {
    pub event_name: String,
    pub count: u64,
}

/// One ranked agent (`?dimension=agent`). Completion rate is
/// `agent.completed / (agent.completed + agent.failed)` — `None` when
/// the agent neither completed nor failed a task in the window (nothing
/// to divide). Average duration is the mean `duration_secs` property
/// over the agent's `agent.completed` events.
#[derive(Serialize)]
pub struct TopAgentEntry {
    pub agent_id: String,
    pub event_count: u64,
    pub completed: u64,
    pub failed: u64,
    pub completion_rate: Option<f64>,
    pub avg_duration_secs: Option<f64>,
}

#[derive(Default)]
struct AgentAgg {
    event_count: u64,
    completed: u64,
    failed: u64,
    duration_secs_sum: f64,
    duration_samples: u64,
}

pub async fn top_events(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Query(params): Query<TopParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let limit = params.limit.unwrap_or(10).min(100);

    match params.dimension.as_deref() {
        None | Some("event_name") => {
            let since = params
                .since
                .unwrap_or_else(|| now_secs().saturating_sub(86400));

            // Query all events in the window (capped to avoid memory issues).
            let events = state.analytics.query(None, Some(since), 100_000).await?;

            // Group by event_name.
            let mut counts: std::collections::HashMap<String, u64> =
                std::collections::HashMap::new();
            for e in events {
                *counts.entry(e.event_name).or_default() += 1;
            }

            // Sort by count descending, take limit.
            let mut entries: Vec<TopEntry> = counts
                .into_iter()
                .map(|(event_name, count)| TopEntry { event_name, count })
                .collect();
            entries.sort_by_key(|e| std::cmp::Reverse(e.count));
            entries.truncate(limit);

            Ok(Json(serde_json::to_value(entries).unwrap_or_default()))
        }
        Some("agent") => {
            let since = params
                .since
                .unwrap_or_else(|| now_secs().saturating_sub(30 * 86_400));

            // Optional workspace scoping, validated against the caller
            // (same rule as `decide` / `record_event`).
            let workspace_id = match params.workspace_id.as_deref() {
                Some(w) => {
                    let id = Id::new(w);
                    super::audit::validate_workspace_scope(&state, &id, &auth).await?;
                    Some(id.to_string())
                }
                None => None,
            };

            let filter = AnalyticsQueryFilter {
                workspace_id,
                since: Some(since),
                until: Some(now_secs()),
                limit: 100_000,
                ..Default::default()
            };
            let events = state.analytics.query_filtered(&filter).await?;

            let mut agents: std::collections::HashMap<String, AgentAgg> =
                std::collections::HashMap::new();
            for e in events {
                // Unattributed events cannot rank an agent.
                let Some(agent_id) = e.agent_id else { continue };
                let agg = agents.entry(agent_id).or_default();
                agg.event_count += 1;
                match e.event_name.as_str() {
                    "agent.completed" => {
                        agg.completed += 1;
                        if let Some(d) = e
                            .properties
                            .get("duration_secs")
                            .and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|i| i as f64)))
                        {
                            agg.duration_secs_sum += d;
                            agg.duration_samples += 1;
                        }
                    }
                    "agent.failed" => agg.failed += 1,
                    _ => {}
                }
            }

            let mut entries: Vec<TopAgentEntry> = agents
                .into_iter()
                .map(|(agent_id, agg)| TopAgentEntry {
                    agent_id,
                    event_count: agg.event_count,
                    completed: agg.completed,
                    failed: agg.failed,
                    completion_rate: if agg.completed + agg.failed > 0 {
                        Some(agg.completed as f64 / (agg.completed + agg.failed) as f64)
                    } else {
                        None
                    },
                    avg_duration_secs: if agg.duration_samples > 0 {
                        Some(agg.duration_secs_sum / agg.duration_samples as f64)
                    } else {
                        None
                    },
                })
                .collect();
            entries.sort_by(|a, b| {
                b.event_count
                    .cmp(&a.event_count)
                    .then_with(|| a.agent_id.cmp(&b.agent_id))
            });
            entries.truncate(limit);

            Ok(Json(serde_json::to_value(entries).unwrap_or_default()))
        }
        Some(other) => Err(ApiError::InvalidInput(format!(
            "unsupported dimension {other:?} (expected event_name or agent)"
        ))),
    }
}

// ─── Analytics Decision API (analytics.md §Decision API for Agents) ──────────

/// `GET /api/v1/analytics/decide?decision=<name>&context=<json>` —
/// evaluate a named built-in decision against current analytics data.
///
/// The evaluators are pure domain functions
/// (`gyre_domain::decisions`); this handler fetches the events each
/// decision consumes through the analytics port (scoped to the caller
/// when the context names a workspace/repo — same anti-forgery rule as
/// `record_event`, task-146 review F1) and the budget snapshot for
/// `budget.headroom`, then delegates the evaluation.
#[derive(Deserialize)]
pub struct DecideParams {
    pub decision: String,
    /// JSON object, URL-encoded (spec example passes
    /// `context=%7B%22flag%22%3A...%7D`). Defaults to `{}`.
    pub context: Option<String>,
}

pub async fn decide(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Query(params): Query<DecideParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Context is a JSON object passed as a (URL-encoded) query string.
    let context: serde_json::Value = match params.context.as_deref() {
        None | Some("") => serde_json::json!({}),
        Some(raw) => serde_json::from_str(raw)
            .map_err(|e| ApiError::InvalidInput(format!("invalid context JSON: {e}")))?,
    };
    let result = evaluate_builtin_decision(&state, &auth, &params.decision, context).await?;
    Ok(Json(result))
}

/// Core built-in decision evaluation, shared by the HTTP `decide` handler
/// above and the `gyre_analytics_decide` MCP tool (analytics.md §MCP Tool:
/// `gyre_analytics_decide`) — the MCP tool is a thin wrapper that must never
/// grow a second evaluator. Parses nothing transport-specific: callers hand
/// over the decision name and an already-decoded context object.
pub(crate) async fn evaluate_builtin_decision(
    state: &AppState,
    auth: &AuthenticatedAgent,
    decision_name: &str,
    context: serde_json::Value,
) -> Result<serde_json::Value, ApiError> {
    let builtin = BuiltinDecision::parse(decision_name).ok_or_else(|| {
        ApiError::InvalidInput(format!(
            "unknown decision {decision_name:?}: expected one of {}",
            BuiltinDecision::all_names().join(", ")
        ))
    })?;

    if !context.is_object() {
        return Err(ApiError::InvalidInput(
            "context must be a JSON object".to_string(),
        ));
    }

    // Caller-scope validation: a context naming a workspace/repo scopes
    // the events the decision is computed over, so a forged id would
    // leak another scope's aggregated analytics through the decision
    // outcome. Same validation as `record_event` (task-146 review F1).
    let (workspace_id, repo_id) =
        super::audit::validate_decision_scope(state, &context, auth).await?;

    let now = now_secs();

    // budget.headroom reads the budget stores, not analytics events.
    let budget = if builtin == BuiltinDecision::BudgetHeadroom {
        let ws = workspace_id.as_ref().ok_or_else(|| {
            ApiError::InvalidInput(
                "budget.headroom requires a workspace_id in the context".to_string(),
            )
        })?;
        let ws_key = super::budget::workspace_key(ws.as_str());
        let config = super::budget::effective_workspace_config(state, &ws_key).await;
        let usage = state.budget_usages.get_usage(&ws_key).await?;
        Some((config, usage))
    } else {
        None
    };

    // Fetch the events this decision consumes, scoped to the caller when
    // the context names a scope.
    let mut events: Vec<AnalyticsEvent> = Vec::new();
    if !builtin.event_names().is_empty() {
        let since = builtin.fetch_since(&context, now);
        for name in builtin.event_names() {
            let filter = AnalyticsQueryFilter {
                event_name: Some(name.to_string()),
                workspace_id: workspace_id.as_ref().map(|id| id.to_string()),
                repo_id: repo_id.as_ref().map(|id| id.to_string()),
                since: Some(since),
                until: Some(now),
                limit: 10_000,
                ..Default::default()
            };
            events.extend(state.analytics.query_filtered(&filter).await?);
        }
    }

    // merge_queue.health also counts currently-blocked (non-terminal)
    // queue entries — a real query against the merge queue port.
    let blocked_count = if builtin == BuiltinDecision::MergeQueueHealth {
        state.merge_queue.list_queue().await?.len() as u64
    } else {
        0
    };

    let outcome = builtin.evaluate(
        &context,
        &events,
        budget.as_ref().map(|(c, u)| (c, u.as_ref())),
        blocked_count,
        now,
    );
    let result = DecisionResult {
        decision: builtin.name().to_string(),
        reason: outcome
            .get("reason")
            .and_then(|r| r.as_str())
            .unwrap_or_default()
            .to_string(),
        outcome,
        evaluated_at: now,
    };
    Ok(result.response_json())
}

/// `POST /api/v1/analytics/decide/custom` — evaluate a custom decision
/// defined with the rule DSL (metric, window, threshold, comparison,
/// then). The first matching rule produces the outcome; when no rule
/// matches, the decision's `default` is returned.
pub async fn decide_custom(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let spec = CustomDecisionSpec::parse(&body).map_err(ApiError::InvalidInput)?;
    let result = evaluate_custom_decision(&state, &auth, &spec).await?;
    Ok(Json(result))
}

/// Core custom-decision evaluation, shared by the HTTP `decide_custom`
/// handler above and the `gyre_analytics_decide` MCP tool (which accepts
/// `custom_rules` tool arguments and assembles the spec itself). Takes an
/// already-parsed `CustomDecisionSpec`; transport-specific parsing stays
/// with each caller.
pub(crate) async fn evaluate_custom_decision(
    state: &AppState,
    auth: &AuthenticatedAgent,
    spec: &CustomDecisionSpec,
) -> Result<serde_json::Value, ApiError> {
    // Caller-scope validation on the spec's context (same rule as the
    // built-in decide handler above).
    let (workspace_id, repo_id) =
        super::audit::validate_decision_scope(state, &spec.context, auth).await?;

    let now = now_secs();

    // Fetch each distinct metric's events once, over the widest window
    // among the rules referencing it. The pure evaluator applies each
    // rule's own window and property filter.
    let mut events_by_metric: std::collections::HashMap<String, Vec<AnalyticsEvent>> =
        std::collections::HashMap::new();
    for metric in spec.metrics() {
        let Some(window_secs) = spec.fetch_window_for(&metric) else {
            continue; // unreachable: parse() validates every window
        };
        let filter = AnalyticsQueryFilter {
            event_name: Some(metric.clone()),
            workspace_id: workspace_id.as_ref().map(|id| id.to_string()),
            repo_id: repo_id.as_ref().map(|id| id.to_string()),
            since: Some(now.saturating_sub(window_secs)),
            until: Some(now),
            limit: 10_000,
            ..Default::default()
        };
        events_by_metric.insert(metric, state.analytics.query_filtered(&filter).await?);
    }

    let outcome = evaluate_custom(spec, &events_by_metric, now);
    let mut response = outcome;
    if let Some(obj) = response.as_object_mut() {
        obj.insert("decision".into(), serde_json::json!(spec.name));
        obj.insert(
            "evaluated_at".into(),
            serde_json::json!(format_iso8601_utc(now)),
        );
    }
    Ok(response)
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
    use super::{epoch_days_to_ymd, new_id, now_secs, parse_time_param, ymd_to_epoch_days};
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use gyre_common::Id;
    use gyre_domain::AnalyticsEvent;
    use http::{Request, StatusCode};
    use serde_json::json;
    use tower::ServiceExt;

    fn app() -> Router {
        // build_router layers require_auth + ABAC on every /api/v1/ route,
        // matching production: the record handler authenticates the caller
        // and binds/scopes the event to that identity (task-146 review F1).
        crate::build_router(test_state())
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
                    .header("Authorization", "Bearer test-token")
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
                    .header("Authorization", "Bearer test-token")
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
                        .header("Authorization", "Bearer test-token")
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
                    .header("Authorization", "Bearer test-token")
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
                    .header("Authorization", "Bearer test-token")
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
                    .header("Authorization", "Bearer test-token")
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
                    .header("Authorization", "Bearer test-token")
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
                    .header("Authorization", "Bearer test-token")
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
                        .header("Authorization", "Bearer test-token")
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
                    .header("Authorization", "Bearer test-token")
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
                    .header("Authorization", "Bearer test-token")
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
                .header("Authorization", "Bearer test-token")
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
                        .header("Authorization", "Bearer test-token")
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
                    .header("Authorization", "Bearer test-token")
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
                    .header("Authorization", "Bearer test-token")
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
                        .header("Authorization", "Bearer test-token")
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
                    .header("Authorization", "Bearer test-token")
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

    /// Helper: POST an event over HTTP. Scope fields in the body are
    /// validated against the caller (task-146 review F1); pass `None` to
    /// record a scope-free event.
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
                    .header("Authorization", "Bearer test-token")
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
                    .header("Authorization", "Bearer test-token")
                    .uri(format!("/api/v1/analytics/events?{query}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "query was: {query}");
        body_json(resp).await
    }

    // ─── task-146 review F1: caller-bound identity + scope validation ──────────

    /// POST an analytics event over HTTP, returning (status, parsed body).
    async fn post_event(app: &Router, body: serde_json::Value) -> (StatusCode, serde_json::Value) {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/analytics/events")
                    .header("Authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    #[tokio::test]
    async fn record_event_binds_identity_to_caller_not_request_body() {
        // F1 regression: agent_id/user_id in the request body are ignored —
        // the recorded event carries the verified caller identity (the
        // global test token authenticates as the "system" agent with no
        // user), so a caller cannot attribute analytics events to an
        // arbitrary user or agent.
        let app = app();
        let (status, json) = post_event(
            &app,
            serde_json::json!({
                "event_name": "feature.used",
                "agent_id": "forged-agent",
                "user_id": "forged-user",
                "session_id": "sess-1",
                "properties": {},
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(
            json["agent_id"].as_str().unwrap(),
            "system",
            "agent_id must be the caller identity, not the forged body value"
        );
        assert_ne!(json["agent_id"].as_str().unwrap(), "forged-agent");
        assert!(
            json["user_id"].is_null(),
            "user_id must come from the caller identity, not the body"
        );
        // session_id is not an identity/scope field and stays caller-supplied.
        assert_eq!(json["session_id"].as_str().unwrap(), "sess-1");

        // The persisted event (not just the response) carries the caller
        // identity — query it back through the API.
        let events = get_events(&app, "event_name=feature.used").await;
        let arr = events.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["agent_id"].as_str().unwrap(), "system");
        assert!(arr[0]["user_id"].is_null());
    }

    #[tokio::test]
    async fn record_event_workspace_scope_validated_against_caller_tenant() {
        // F1 regression: workspace attribution must resolve within the
        // caller's tenant (global test token -> "default"). A forged
        // cross-tenant attribution is rejected and never lands in the
        // victim scope's filtered query results.
        let state = test_state();
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                gyre_common::Id::new("ws-own"),
                gyre_common::Id::new("default"),
                "Own",
                "own",
                1000,
            ))
            .await
            .unwrap();
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                gyre_common::Id::new("ws-victim"),
                gyre_common::Id::new("tenant-other"),
                "Victim",
                "victim",
                1000,
            ))
            .await
            .unwrap();
        let app = crate::build_router(state);

        // Own-tenant workspace: accepted and attributed.
        let (status, json) = post_event(
            &app,
            serde_json::json!({
                "event_name": "feature.used",
                "workspace_id": "ws-own",
                "properties": {},
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(json["workspace_id"].as_str().unwrap(), "ws-own");

        // Cross-tenant workspace: Forbidden — no forged attribution.
        let (status, _) = post_event(
            &app,
            serde_json::json!({
                "event_name": "feature.used",
                "workspace_id": "ws-victim",
                "properties": {},
            }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // Unknown workspace: NotFound, not a silent attribution.
        let (status, _) = post_event(
            &app,
            serde_json::json!({
                "event_name": "feature.used",
                "workspace_id": "ws-nope",
                "properties": {},
            }),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // The rejected writes never polluted the victim scope's results —
        // the exact repro from the review finding.
        let events = get_events(&app, "workspace_id=ws-victim").await;
        assert_eq!(
            events.as_array().unwrap().len(),
            0,
            "forged event must not appear in the victim workspace's filtered results"
        );
    }

    #[tokio::test]
    async fn record_event_repo_scope_validated_against_caller_tenant() {
        // F1 regression: repo attribution requires the repo to exist, its
        // workspace to resolve within the caller's tenant, and — when the
        // event is also labeled with a workspace — the repo to belong to
        // that workspace (audit.rs task-102 F2 pattern).
        let state = test_state();
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                gyre_common::Id::new("ws-own"),
                gyre_common::Id::new("default"),
                "Own",
                "own",
                1000,
            ))
            .await
            .unwrap();
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                gyre_common::Id::new("ws-own-2"),
                gyre_common::Id::new("default"),
                "Own 2",
                "own-2",
                1000,
            ))
            .await
            .unwrap();
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                gyre_common::Id::new("ws-victim"),
                gyre_common::Id::new("tenant-other"),
                "Victim",
                "victim",
                1000,
            ))
            .await
            .unwrap();
        state
            .repos
            .create(&gyre_domain::Repository::new(
                gyre_common::Id::new("repo-own"),
                gyre_common::Id::new("ws-own"),
                "own-repo",
                "/repos/own-repo",
                1000,
            ))
            .await
            .unwrap();
        state
            .repos
            .create(&gyre_domain::Repository::new(
                gyre_common::Id::new("repo-own-2"),
                gyre_common::Id::new("ws-own-2"),
                "own-repo-2",
                "/repos/own-repo-2",
                1000,
            ))
            .await
            .unwrap();
        state
            .repos
            .create(&gyre_domain::Repository::new(
                gyre_common::Id::new("repo-victim"),
                gyre_common::Id::new("ws-victim"),
                "victim-repo",
                "/repos/victim-repo",
                1000,
            ))
            .await
            .unwrap();
        let app = crate::build_router(state);

        // Consistent own-tenant pair: accepted.
        let (status, json) = post_event(
            &app,
            serde_json::json!({
                "event_name": "feature.used",
                "workspace_id": "ws-own",
                "repo_id": "repo-own",
                "properties": {},
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(json["repo_id"].as_str().unwrap(), "repo-own");

        // Repo in another tenant: Forbidden.
        let (status, _) = post_event(
            &app,
            serde_json::json!({
                "event_name": "feature.used",
                "repo_id": "repo-victim",
                "properties": {},
            }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // Repo in the caller's tenant but labeled with a different
        // (individually authorized) workspace: inconsistent attribution
        // is Forbidden.
        let (status, _) = post_event(
            &app,
            serde_json::json!({
                "event_name": "feature.used",
                "workspace_id": "ws-own",
                "repo_id": "repo-own-2",
                "properties": {},
            }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // Unknown repo: NotFound.
        let (status, _) = post_event(
            &app,
            serde_json::json!({
                "event_name": "feature.used",
                "repo_id": "repo-nope",
                "properties": {},
            }),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // No forged event reached the victim repo's filtered results.
        let events = get_events(&app, "repo_id=repo-victim").await;
        assert_eq!(
            events.as_array().unwrap().len(),
            0,
            "forged event must not appear in the victim repo's filtered results"
        );
    }

    #[tokio::test]
    async fn query_events_filters_by_scope_ids() {
        // Spec: agent_id, workspace_id, repo_id, user_id must each filter.
        // Events are seeded directly against the repository: the POST
        // handler binds identity to the caller and validates scope fields
        // (task-146 review F1), so arbitrary scope attribution for filter
        // coverage is a storage-level concern — the HTTP attribution rules
        // are pinned by the record_event_* regression tests below.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let seed = |id: &str,
                    name: &str,
                    agent: Option<&str>,
                    user: Option<&str>,
                    ws: Option<&str>,
                    repo: Option<&str>| {
            gyre_domain::AnalyticsEvent::new(
                gyre_common::Id::new(id),
                name,
                agent.map(|s| s.to_string()),
                serde_json::json!({ "k": "v" }),
                100,
            )
            .with_scope(
                user.map(gyre_common::Id::new).as_ref(),
                None,
                ws.map(gyre_common::Id::new).as_ref(),
                repo.map(gyre_common::Id::new).as_ref(),
            )
        };
        state
            .analytics
            .record(&seed(
                "e-1",
                "mr.merged",
                Some("agent-1"),
                None,
                Some("ws-1"),
                Some("repo-1"),
            ))
            .await
            .unwrap();
        state
            .analytics
            .record(&seed(
                "e-2",
                "mr.merged",
                Some("agent-2"),
                None,
                Some("ws-2"),
                Some("repo-1"),
            ))
            .await
            .unwrap();
        state
            .analytics
            .record(&seed(
                "e-3",
                "mr.closed",
                Some("agent-1"),
                Some("user-9"),
                Some("ws-1"),
                None,
            ))
            .await
            .unwrap();

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
        let json = get_events(
            &app,
            "agent_id=agent-1&workspace_id=ws-1&event_name=mr.merged",
        )
        .await;
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
        assert!(arr
            .iter()
            .all(|e| e["event_name"].as_str().unwrap().starts_with("mr.")));

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
        // Seeded directly against the repository — the POST handler binds
        // identity/scope to the caller (task-146 review F1), so multi-agent /
        // multi-workspace aggregation coverage seeds at the storage level.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let seed = |id: &str, name: &str, agent: &str, ws: &str, ts: u64| {
            gyre_domain::AnalyticsEvent::new(
                gyre_common::Id::new(id),
                name,
                Some(agent.to_string()),
                serde_json::json!({}),
                ts,
            )
            .with_scope(None, None, Some(&gyre_common::Id::new(ws)), None)
        };
        for (id, ts) in [("g-1", 100u64), ("g-2", 101)] {
            state
                .analytics
                .record(&seed(id, "mr.merged", "a1", "ws-1", ts))
                .await
                .unwrap();
        }
        state
            .analytics
            .record(&seed("g-3", "mr.closed", "a2", "ws-2", 102))
            .await
            .unwrap();

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

        // day grouping: the deterministic timestamps all land on the same
        // 1970-01-01 UTC bucket.
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

    #[tokio::test]
    async fn query_events_group_by_counts_full_filtered_set() {
        // task-146 review round 2 F1 regression: group_by must aggregate
        // the FULL filtered set, not the limit-truncated event list — the
        // exact repro from the review seeds 150 events (default limit is
        // 100) and the grouped total must agree with /count. The old
        // in-memory grouping returned {"mr.merged": 100} here.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        for i in 0..150u64 {
            let ev = gyre_domain::AnalyticsEvent::new(
                gyre_common::Id::new(format!("g-full-{i}")),
                "mr.merged",
                None,
                serde_json::json!({}),
                1000 + i,
            );
            state.analytics.record(&ev).await.unwrap();
        }
        for i in 0..7u64 {
            let ev = gyre_domain::AnalyticsEvent::new(
                gyre_common::Id::new(format!("g-full-closed-{i}")),
                "mr.closed",
                None,
                serde_json::json!({}),
                1000 + i,
            );
            state.analytics.record(&ev).await.unwrap();
        }

        // Grouped totals over the full set (150 > default limit 100).
        let json = get_events(&app, "group_by=event_name").await;
        let obj = json.as_object().expect("group_by returns an object");
        assert_eq!(
            obj.get("mr.merged").and_then(|v| v.as_u64()),
            Some(150),
            "group_by must count the full filtered set, not the newest 100"
        );
        assert_eq!(obj.get("mr.closed").and_then(|v| v.as_u64()), Some(7));

        // Consistency with /count on the same question (the review's
        // disagreement repro). i64::MAX, not u64::MAX: the SQL adapters
        // bind `until` as i64.
        let count = state
            .analytics
            .count("mr.merged", 0, i64::MAX as u64)
            .await
            .unwrap();
        assert_eq!(count, 150);

        // limit bounds the number of returned GROUPS (ordered by key:
        // "mr.closed" sorts before "mr.merged"), never the totals.
        let json = get_events(&app, "group_by=event_name&limit=1").await;
        let obj = json.as_object().unwrap();
        assert_eq!(obj.len(), 1, "limit bounds the number of groups");
        assert_eq!(obj.get("mr.closed").and_then(|v| v.as_u64()), Some(7));

        // The grouped path honors the same filter parameters as the raw
        // path. "mr.m*" is a discriminating prefix (matches mr.merged,
        // not mr.closed) and still yields the full 150 total.
        let json = get_events(&app, "group_by=event_name&event_name=mr.m*").await;
        let obj = json.as_object().unwrap();
        assert_eq!(obj.len(), 1, "mr.m* prefix must exclude mr.closed");
        assert_eq!(obj.get("mr.merged").and_then(|v| v.as_u64()), Some(150));
        // The broader "mr.*" prefix spans both event names; grouped
        // totals remain the full filtered set, never the limit-truncated
        // newest-100 list.
        let json = get_events(&app, "group_by=event_name&event_name=mr.*").await;
        let obj = json.as_object().unwrap();
        assert_eq!(obj.len(), 2);
        assert_eq!(obj.get("mr.merged").and_then(|v| v.as_u64()), Some(150));
        assert_eq!(obj.get("mr.closed").and_then(|v| v.as_u64()), Some(7));
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
        assert_eq!(
            parse_time_param("1970-01-01T01:02:03Z"),
            Some((3_723, false))
        );
        // No timezone means UTC.
        assert_eq!(parse_time_param("1970-01-01T00:00:30"), Some((30, false)));
        // Minutes precision.
        assert_eq!(parse_time_param("1970-01-01T00:10Z"), Some((600, false)));
        // Fractional seconds truncate.
        assert_eq!(
            parse_time_param("1970-01-01T00:00:30.987Z"),
            Some((30, false))
        );
        // Numeric offsets: +HH:MM, -HH:MM, +HHMM, +HH.
        assert_eq!(
            parse_time_param("1970-01-01T02:00:00+02:00"),
            Some((0, false))
        );
        assert_eq!(
            parse_time_param("1970-01-01T00:30:00-00:30"),
            Some((3_600, false))
        );
        assert_eq!(
            parse_time_param("1970-01-01T02:00:00+0200"),
            Some((0, false))
        );
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
        assert_eq!(
            json.as_array().unwrap().len(),
            2,
            "ISO8601 since bounds below"
        );

        // 1970-01-01T00:05Z == 300 — inclusive upper bound.
        let json = get_events(&app, "event_name=ev.iso&until=1970-01-01T00:05Z").await;
        assert_eq!(
            json.as_array().unwrap().len(),
            3,
            "ISO8601 until is inclusive"
        );

        // Date-only bounds: 1970-01-01 covers the whole first day.
        let json = get_events(&app, "event_name=ev.iso&since=1970-01-01&until=1970-01-01").await;
        assert_eq!(json.as_array().unwrap().len(), 3);

        // Offset form: 1970-01-01T02:00+02:00 == 0. The `+` must be
        // percent-encoded as %2B — a raw `+` decodes as a space in query
        // strings (application/x-www-form-urlencoded), which no timestamp
        // format uses.
        let json = get_events(&app, "event_name=ev.iso&since=1970-01-01T02:00%2B02:00").await;
        assert_eq!(json.as_array().unwrap().len(), 3);

        // Unix seconds still accepted (backward compat).
        let json = get_events(&app, "event_name=ev.iso&since=200&until=200").await;
        assert_eq!(json.as_array().unwrap().len(), 1);

        // Malformed timestamp is a 400 with a clear message, not a 500.
        let resp = app
            .oneshot(
                Request::builder()
                    .header("Authorization", "Bearer test-token")
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
                    .header("Authorization", "Bearer test-token")
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
                    .header("Authorization", "Bearer test-token")
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
                    .header("Authorization", "Bearer test-token")
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
                    .header("Authorization", "Bearer test-token")
                    .uri("/api/v1/analytics/daily?event_name=ev.daily&since=9999&until=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn daily_events_bare_days_form_returns_total_volume() {
        // The Analytics Dashboard bar chart (analytics.md §Analytics
        // Dashboard: "daily event volume for the last 30 days") sends the
        // bare `GET /daily?days=30` form with NO event_name. That request
        // must NOT 400 on a missing required parameter (task-149 review
        // finding), and it must return TOTAL daily volume across ALL
        // event names — not the per-event-name series — so the chart's
        // bars are the aggregate counts. Seeded events deliberately use
        // two DISTINCT event names on the same day: the total form must
        // count both, the per-name form only one.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let now = crate::api::now_secs();
        for (id, name, ts) in [
            ("tv-1", "agent.spawned", now),
            ("tv-2", "agent.completed", now),
            ("tv-3", "agent.spawned", now),
            // Out of window (40 days ago): excluded from every form.
            ("tv-old", "agent.spawned", now.saturating_sub(40 * 86_400)),
        ] {
            let ev = gyre_domain::AnalyticsEvent::new(
                gyre_common::Id::new(id),
                name,
                None,
                serde_json::json!({}),
                ts,
            );
            state.analytics.record(&ev).await.unwrap();
        }

        // Bare `?days=30` — the exact request the dashboard chart sends.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .header("Authorization", "Bearer test-token")
                    .uri("/api/v1/analytics/daily?days=30")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "bare ?days=30 (no event_name) must not be rejected — \
             the dashboard bar chart sends exactly this form"
        );
        let json = body_json(resp).await;
        let total: u64 = json
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["count"].as_u64().unwrap_or(0))
            .sum();
        assert_eq!(
            total, 3,
            "no event_name = total daily volume across ALL event names \
             (2x agent.spawned + 1x agent.completed today, old event excluded)"
        );

        // Spec-documented empty-value form `?event_name=&days=30` is the
        // same "no filter" request.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .header("Authorization", "Bearer test-token")
                    .uri("/api/v1/analytics/daily?event_name=&days=30")
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
        assert_eq!(total, 3, "empty event_name aggregates all names");

        // Explicit event_name still filters to that name's series.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .header("Authorization", "Bearer test-token")
                    .uri("/api/v1/analytics/daily?event_name=agent.spawned&days=30")
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
        assert_eq!(total, 2, "explicit event_name filters to that name only");
    }

    // ─── Decision API (analytics.md §Decision API for Agents) ─────────────────

    /// Percent-encode a query-parameter value (the `context` JSON is
    /// passed URL-encoded per the spec example).
    fn urlencode(s: &str) -> String {
        let mut out = String::new();
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(b as char)
                }
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
        out
    }

    /// GET /analytics/decide with a JSON context, returning
    /// (status, parsed body).
    async fn decide(
        app: &Router,
        decision: &str,
        context: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let uri = format!(
            "/api/v1/analytics/decide?decision={}&context={}",
            decision,
            urlencode(&serde_json::to_string(&context).unwrap())
        );
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .header("Authorization", "Bearer test-token")
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    /// POST /analytics/decide/custom, returning (status, parsed body).
    async fn decide_custom(
        app: &Router,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/analytics/decide/custom")
                    .header("Authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    /// Seed an analytics event with a CONTROLLED timestamp straight into
    /// the store (the HTTP record endpoint stamps `now`, which cannot
    /// exercise historical windows).
    async fn seed_event(state: &crate::AppState, ev: gyre_domain::AnalyticsEvent) {
        state.analytics.record(&ev).await.unwrap();
    }

    fn make_event(
        name: &str,
        props: serde_json::Value,
        timestamp: u64,
    ) -> gyre_domain::AnalyticsEvent {
        gyre_domain::AnalyticsEvent::new(super::new_id(), name, None, props, timestamp)
    }

    #[tokio::test]
    async fn decide_unknown_decision_is_rejected() {
        let app = app();
        let (status, body) = decide(&app, "feature.explode", serde_json::json!({})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body["error"]
                .as_str()
                .unwrap_or_default()
                .contains("feature.promote"),
            "error should list the built-in decisions: {body}"
        );
    }

    #[tokio::test]
    async fn decide_all_builtins_on_empty_data_return_defaults_not_errors() {
        // Edge case from the implementation plan: empty analytics data
        // returns reasonable defaults, not errors.
        let state = test_state();
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                gyre_common::Id::new("ws-empty"),
                gyre_common::Id::new("default"),
                "Empty",
                "empty",
                1000,
            ))
            .await
            .unwrap();
        let app = crate::build_router(state);

        let (status, body) =
            decide(&app, "feature.promote", serde_json::json!({"flag": "f"})).await;
        assert_eq!(status, StatusCode::OK, "body: {body}");
        assert_eq!(body["promote"], serde_json::json!(false));
        assert_eq!(body["confidence"], serde_json::json!(0.0));
        assert!(body["evaluated_at"].as_str().unwrap().ends_with('Z'));

        let (status, body) =
            decide(&app, "feature.rollback", serde_json::json!({"flag": "f"})).await;
        assert_eq!(status, StatusCode::OK, "body: {body}");
        assert_eq!(body["rollback"], serde_json::json!(false));
        assert_eq!(body["trigger"], serde_json::json!("none"));

        let (status, body) = decide(&app, "gate.effectiveness", serde_json::json!({})).await;
        assert_eq!(status, StatusCode::OK, "body: {body}");
        assert_eq!(body["effective"], serde_json::json!(false));
        assert_eq!(body["catch_rate"], serde_json::json!(0.0));
        assert_eq!(body["false_positive_rate"], serde_json::json!(0.0));

        let (status, body) = decide(&app, "agent.performance", serde_json::json!({})).await;
        assert_eq!(status, StatusCode::OK, "body: {body}");
        assert_eq!(body["healthy"], serde_json::json!(true));
        assert_eq!(body["failure_rate"], serde_json::json!(0.0));

        let (status, body) = decide(
            &app,
            "budget.headroom",
            serde_json::json!({"workspace_id": "ws-empty"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {body}");
        assert_eq!(body["tokens_remaining_pct"], serde_json::json!(100.0));
        assert_eq!(body["cost_remaining_pct"], serde_json::json!(100.0));
        assert_eq!(
            body["projected_exhaustion_at"],
            serde_json::json!("unknown")
        );

        let (status, body) = decide(&app, "merge_queue.health", serde_json::json!({})).await;
        assert_eq!(status, StatusCode::OK, "body: {body}");
        assert_eq!(body["healthy"], serde_json::json!(true));
        assert_eq!(body["failure_rate"], serde_json::json!(0.0));
        assert_eq!(body["blocked_count"], serde_json::json!(0));
    }

    #[tokio::test]
    async fn decide_feature_promote_from_real_event_data() {
        let state = test_state();
        let now = super::now_secs();
        let day = 86_400u64;
        // Current window: 100 uses of the flag by distinct users, no
        // errors, flat latency. Baseline window: same shape.
        for i in 0..100 {
            let mut cur = make_event(
                "feature.used",
                serde_json::json!({"flag": "new-diff-viewer", "latency_ms": 100}),
                now - day,
            );
            cur.user_id = Some(format!("user-{i}"));
            seed_event(&state, cur).await;
            let mut prev = make_event(
                "feature.used",
                serde_json::json!({"flag": "new-diff-viewer", "latency_ms": 100}),
                now - 8 * day,
            );
            prev.user_id = Some(format!("old-{i}"));
            seed_event(&state, prev).await;
        }
        let app = crate::build_router(state);

        let (status, body) = decide(
            &app,
            "feature.promote",
            serde_json::json!({"flag": "new-diff-viewer", "baseline_days": 7}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {body}");
        assert_eq!(body["decision"], serde_json::json!("feature.promote"));
        assert_eq!(body["promote"], serde_json::json!(true), "body: {body}");
        assert_eq!(body["metrics"]["sample_size"], serde_json::json!(100));
        assert_eq!(body["metrics"]["adoption_rate"], serde_json::json!(1.0));
        assert_eq!(body["metrics"]["error_rate_delta"], serde_json::json!(0.0));
        assert!(body["confidence"].as_f64().unwrap() >= 0.9);
        assert!(body["reason"].as_str().unwrap().contains("Adoption"));
    }

    #[tokio::test]
    async fn decide_feature_promote_rejects_error_spike() {
        let state = test_state();
        let now = super::now_secs();
        let day = 86_400u64;
        for i in 0..50 {
            let mut cur = make_event(
                "feature.used",
                serde_json::json!({"flag": "bad-flag", "latency_ms": 100}),
                now - day,
            );
            cur.user_id = Some(format!("user-{i}"));
            seed_event(&state, cur).await;
        }
        for _ in 0..20 {
            seed_event(
                &state,
                make_event(
                    "feature.error",
                    serde_json::json!({"flag": "bad-flag"}),
                    now - day,
                ),
            )
            .await;
        }
        let app = crate::build_router(state);

        let (status, body) = decide(
            &app,
            "feature.promote",
            serde_json::json!({"flag": "bad-flag"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["promote"], serde_json::json!(false), "body: {body}");
        assert!(body["metrics"]["error_rate_delta"].as_f64().unwrap() > 0.2);
    }

    #[tokio::test]
    async fn decide_feature_rollback_triggers_on_gate_failure_spike() {
        let state = test_state();
        let now = super::now_secs();
        let w = 1_800u64;
        // Before window: 1 failed / 50 passed (~2%).
        for i in 0..50 {
            seed_event(
                &state,
                make_event(
                    "gate.passed",
                    serde_json::json!({"mr_id": format!("b{i}")}),
                    now - 2 * w,
                ),
            )
            .await;
        }
        seed_event(
            &state,
            make_event(
                "gate.failed",
                serde_json::json!({"mr_id": "bf"}),
                now - 2 * w,
            ),
        )
        .await;
        // Current window: 25 failed / 48 passed (~34%).
        for i in 0..48 {
            seed_event(
                &state,
                make_event(
                    "gate.passed",
                    serde_json::json!({"mr_id": format!("c{i}")}),
                    now - w / 2,
                ),
            )
            .await;
        }
        for i in 0..25 {
            seed_event(
                &state,
                make_event(
                    "gate.failed",
                    serde_json::json!({"mr_id": format!("cf{i}")}),
                    now - w / 2,
                ),
            )
            .await;
        }
        let app = crate::build_router(state);

        let (status, body) = decide(
            &app,
            "feature.rollback",
            serde_json::json!({"flag": "optimistic-merge", "window_mins": 30}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {body}");
        assert_eq!(body["rollback"], serde_json::json!(true));
        assert_eq!(
            body["trigger"],
            serde_json::json!("gate.failure_rate_spike")
        );
        assert!(body["metrics"]["before_rate"].as_f64().unwrap() < 0.05);
        assert!(body["metrics"]["after_rate"].as_f64().unwrap() > 0.3);
        assert_eq!(body["metrics"]["affected_mrs"], serde_json::json!(25));
        assert!(body["reason"].as_str().unwrap().contains("increased"));
    }

    #[tokio::test]
    async fn decide_gate_effectiveness_and_agent_performance() {
        let state = test_state();
        let now = super::now_secs();
        // Gate: 90 passed, 10 failed (all real) → catch rate 0.1, fp 0.0.
        for i in 0..90 {
            seed_event(
                &state,
                make_event(
                    "gate.passed",
                    serde_json::json!({"gate_type": "lint", "mr_id": i}),
                    now - 100,
                ),
            )
            .await;
        }
        for i in 0..10 {
            seed_event(
                &state,
                make_event(
                    "gate.failed",
                    serde_json::json!({"gate_type": "lint", "mr_id": 100 + i, "false_positive": false}),
                    now - 100,
                ),
            )
            .await;
        }
        // Agents: 90 completed at ~100s, 10 failed → failure rate 0.1.
        for i in 0..90 {
            seed_event(
                &state,
                make_event(
                    "agent.completed",
                    serde_json::json!({"duration_secs": 80 + i % 40}),
                    now - 100,
                ),
            )
            .await;
        }
        for _ in 0..10 {
            seed_event(
                &state,
                make_event("agent.failed", serde_json::json!({}), now - 100),
            )
            .await;
        }
        let app = crate::build_router(state);

        let (status, body) = decide(&app, "gate.effectiveness", serde_json::json!({})).await;
        assert_eq!(status, StatusCode::OK, "body: {body}");
        assert_eq!(body["effective"], serde_json::json!(true));
        assert_eq!(body["catch_rate"], serde_json::json!(0.1));
        assert_eq!(body["false_positive_rate"], serde_json::json!(0.0));

        let (status, body) = decide(&app, "agent.performance", serde_json::json!({})).await;
        assert_eq!(status, StatusCode::OK, "body: {body}");
        assert_eq!(body["healthy"], serde_json::json!(true));
        assert_eq!(body["failure_rate"], serde_json::json!(0.1));
        let p50 = body["p50_duration_secs"].as_f64().unwrap();
        assert!((80.0..=120.0).contains(&p50), "p50 {p50}");
        assert!(body["p99_duration_secs"].as_f64().unwrap() >= p50);
    }

    #[tokio::test]
    async fn decide_budget_headroom_from_real_usage() {
        let state = test_state();
        let ws = gyre_common::Id::new("ws-budget");
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                ws.clone(),
                gyre_common::Id::new("default"),
                "Budget",
                "budget",
                1000,
            ))
            .await
            .unwrap();
        let now = super::now_secs();
        let ws_key = format!("workspace:{}", ws.as_str());
        state
            .budget_configs
            .set_config(
                &ws_key,
                &gyre_domain::BudgetConfig {
                    max_tokens_per_day: Some(10_000),
                    max_cost_per_day: Some(50.0),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        state
            .budget_usages
            .set_usage(
                &ws_key,
                &gyre_domain::BudgetUsage {
                    entity_type: "workspace".into(),
                    entity_id: ws.clone(),
                    tokens_used_today: 2_500,
                    cost_today: 10.0,
                    active_agents: 1,
                    period_start: now - 3_600,
                    compute_minutes: 60,
                },
            )
            .await
            .unwrap();
        let app = crate::build_router(state);

        let (status, body) = decide(
            &app,
            "budget.headroom",
            serde_json::json!({"workspace_id": ws.as_str()}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {body}");
        assert_eq!(body["tokens_remaining_pct"], serde_json::json!(75.0));
        assert_eq!(body["cost_remaining_pct"], serde_json::json!(80.0));
        // Token burn 2500/h → 7500 left → 3h out.
        assert_eq!(
            body["projected_exhaustion_at"].as_str().unwrap(),
            gyre_domain::decisions::format_iso8601_utc(now + 10_800)
        );
    }

    #[tokio::test]
    async fn decide_budget_headroom_requires_workspace() {
        let app = app();
        let (status, body) = decide(&app, "budget.headroom", serde_json::json!({})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
    }

    #[tokio::test]
    async fn decide_merge_queue_health_from_real_events_and_queue() {
        let state = test_state();
        let now = super::now_secs();
        for i in 0..90 {
            seed_event(
                &state,
                make_event(
                    "merge_queue.processed",
                    serde_json::json!({"mr_id": i, "outcome": "merged", "wait_secs": 300 + i % 60}),
                    now - 1_000,
                ),
            )
            .await;
        }
        for i in 0..10 {
            seed_event(
                &state,
                make_event(
                    "merge_queue.processed",
                    serde_json::json!({"mr_id": 100 + i, "outcome": "failed", "wait_secs": 400}),
                    now - 1_000,
                ),
            )
            .await;
        }
        // Two non-terminal (blocked) queue entries.
        for i in 0..2 {
            state
                .merge_queue
                .enqueue(&gyre_domain::MergeQueueEntry::new(
                    gyre_common::Id::new(format!("mq-{i}")),
                    gyre_common::Id::new(format!("mr-{i}")),
                    50,
                    now - 60,
                ))
                .await
                .unwrap();
        }
        let app = crate::build_router(state);

        let (status, body) = decide(&app, "merge_queue.health", serde_json::json!({})).await;
        assert_eq!(status, StatusCode::OK, "body: {body}");
        assert_eq!(body["healthy"], serde_json::json!(true));
        assert_eq!(body["failure_rate"], serde_json::json!(0.1));
        assert_eq!(body["blocked_count"], serde_json::json!(2));
        let avg = body["avg_wait_secs"].as_f64().unwrap();
        assert!(avg > 300.0 && avg < 400.0, "avg {avg}");
    }

    #[tokio::test]
    async fn decide_custom_rule_dsl_first_match_and_default() {
        let state = test_state();
        let now = super::now_secs();
        // 2 spec approvals in the last week (threshold 3 → no match);
        // 1 failed + 3 merged queue entries (threshold 5 → no match).
        for i in 0..2 {
            seed_event(
                &state,
                make_event(
                    "spec.approved",
                    serde_json::json!({"spec_path": i}),
                    now - 100,
                ),
            )
            .await;
        }
        for i in 0..3 {
            seed_event(
                &state,
                make_event(
                    "merge_queue.processed",
                    serde_json::json!({"mr_id": i, "outcome": "merged", "wait_secs": 10}),
                    now - 100,
                ),
            )
            .await;
        }
        seed_event(
            &state,
            make_event(
                "merge_queue.processed",
                serde_json::json!({"mr_id": 9, "outcome": "failed", "wait_secs": 10}),
                now - 100,
            ),
        )
        .await;
        let app = crate::build_router(state);

        // The spec's own example decision (analytics.md §POST decide/custom).
        let (status, body) = decide_custom(
            &app,
            serde_json::json!({
                "name": "spec-approval-pace",
                "description": "Are specs being approved fast enough to unblock the merge queue?",
                "rules": [
                    {"metric": "spec.approved", "window": "7d", "threshold": 3, "comparison": "gte", "then": "approve"},
                    {"metric": "merge_queue.processed", "filter": {"outcome": "failed"}, "window": "24h", "threshold": 5, "comparison": "gte", "then": "escalate"}
                ],
                "default": "hold",
                "context": {}
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {body}");
        assert_eq!(body["decision"], serde_json::json!("spec-approval-pace"));
        assert_eq!(body["outcome"], serde_json::json!("hold"));
        assert!(body["matched_rule"].is_null());
        let reason = body["reason"].as_str().unwrap();
        assert!(reason.contains("count 2"), "reason: {reason}");
        assert!(reason.contains("count 1"), "reason: {reason}");
        assert!(body["evaluated_at"].as_str().unwrap().ends_with('Z'));

        // Same store, lower threshold → rule 0 matches and wins.
        let (status, body) = decide_custom(
            &app,
            serde_json::json!({
                "name": "spec-approval-pace",
                "rules": [
                    {"metric": "spec.approved", "window": "7d", "threshold": 2, "comparison": "gte", "then": "approve"},
                    {"metric": "spec.approved", "window": "7d", "threshold": 1, "comparison": "gte", "then": "never"}
                ],
                "default": "hold"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {body}");
        assert_eq!(body["outcome"], serde_json::json!("approve"));
        assert_eq!(body["matched_rule"], serde_json::json!(0));
    }

    #[tokio::test]
    async fn decide_custom_invalid_dsl_is_rejected() {
        let app = app();
        let (status, body) = decide_custom(
            &app,
            serde_json::json!({
                "name": "bad",
                "rules": [
                    {"metric": "m", "window": "7d", "threshold": 1, "comparison": "approx", "then": "x"}
                ],
                "default": "hold"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
        assert!(body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("comparison"));
    }

    #[tokio::test]
    async fn decide_rejects_cross_tenant_scope_in_context() {
        // Same anti-forgery rule as record_event (task-146 review F1): a
        // context naming another tenant's workspace must not scope the
        // decision's event queries — Forbidden, not silently ignored.
        let state = test_state();
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                gyre_common::Id::new("ws-victim"),
                gyre_common::Id::new("tenant-other"),
                "Victim",
                "victim",
                1000,
            ))
            .await
            .unwrap();
        let app = crate::build_router(state);

        let (status, _) = decide(
            &app,
            "merge_queue.health",
            serde_json::json!({"workspace_id": "ws-victim"}),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (status, _) = decide_custom(
            &app,
            serde_json::json!({
                "name": "d",
                "rules": [{"metric": "m", "window": "1h", "threshold": 1, "comparison": "gte", "then": "x"}],
                "default": "hold",
                "context": {"workspace_id": "ws-victim"}
            }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    // ─── Analytics Dashboard support (task-149, analytics.md §Analytics
    // Dashboard: top agents, funnel counts) ────────────────────────────────

    /// Seed an event directly through the analytics port. The HTTP ingest
    /// binds agent identity to the caller, which would attribute every
    /// seeded event to the test caller — the ranking tests need distinct
    /// agents, so they bypass the endpoint and write through the port the
    /// endpoint itself persists to.
    async fn seed_agent_event(
        state: &crate::AppState,
        event_name: &str,
        agent_id: Option<&str>,
        properties: serde_json::Value,
        workspace_id: Option<&str>,
    ) {
        let mut ev = AnalyticsEvent::new(
            new_id(),
            event_name,
            agent_id.map(|a| a.to_string()),
            properties,
            now_secs(),
        );
        if let Some(ws) = workspace_id {
            ev = ev.with_scope(None, None, Some(&Id::new(ws)), None);
        }
        state.analytics.record(&ev).await.unwrap();
    }

    async fn get_json(app: &Router, uri: &str) -> (StatusCode, serde_json::Value) {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .header("Authorization", "Bearer test-token")
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        (status, body_json(resp).await)
    }

    #[tokio::test]
    async fn top_agents_dimension_ranks_and_computes_rates() {
        let state = test_state();
        // agent-a: 4 events — spawned, 2 completions (durations 100s/200s),
        // 1 failure → completion rate 2/3, avg duration 150s.
        seed_agent_event(&state, "agent.spawned", Some("agent-a"), json!({}), None).await;
        seed_agent_event(
            &state,
            "agent.completed",
            Some("agent-a"),
            json!({"duration_secs": 100}),
            None,
        )
        .await;
        seed_agent_event(
            &state,
            "agent.completed",
            Some("agent-a"),
            json!({"duration_secs": 200}),
            None,
        )
        .await;
        seed_agent_event(&state, "agent.failed", Some("agent-a"), json!({}), None).await;
        // agent-b: 1 event, no completions or failures → null rates.
        seed_agent_event(&state, "agent.spawned", Some("agent-b"), json!({}), None).await;
        // Unattributed event cannot rank an agent.
        seed_agent_event(&state, "agent.spawned", None, json!({}), None).await;

        let app = crate::build_router(state);
        let (status, json) = get_json(
            &app,
            "/api/v1/analytics/top?dimension=agent&since=0&limit=10",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 2, "unattributed event must not rank: {arr:?}");
        assert_eq!(arr[0]["agent_id"], "agent-a");
        assert_eq!(arr[0]["event_count"], 4);
        assert_eq!(arr[0]["completed"], 2);
        assert_eq!(arr[0]["failed"], 1);
        let rate = arr[0]["completion_rate"].as_f64().unwrap();
        assert!((rate - 2.0 / 3.0).abs() < 1e-9, "rate was {rate}");
        assert_eq!(arr[0]["avg_duration_secs"].as_f64().unwrap(), 150.0);
        assert_eq!(arr[1]["agent_id"], "agent-b");
        assert_eq!(arr[1]["event_count"], 1);
        assert!(arr[1]["completion_rate"].is_null());
        assert!(arr[1]["avg_duration_secs"].is_null());
    }

    #[tokio::test]
    async fn top_agents_workspace_scope_validated_against_caller_tenant() {
        // Same anti-forgery rule as record_event/decide: the workspace
        // filter must resolve within the caller's tenant, and the ranking
        // must only cover events actually attributed to that workspace.
        let state = test_state();
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                Id::new("ws-own"),
                Id::new("default"),
                "Own",
                "own",
                1000,
            ))
            .await
            .unwrap();
        state
            .workspaces
            .create(&gyre_domain::Workspace::new(
                Id::new("ws-victim"),
                Id::new("tenant-other"),
                "Victim",
                "victim",
                1000,
            ))
            .await
            .unwrap();
        seed_agent_event(
            &state,
            "agent.spawned",
            Some("agent-own"),
            json!({}),
            Some("ws-own"),
        )
        .await;
        seed_agent_event(
            &state,
            "agent.spawned",
            Some("agent-victim"),
            json!({}),
            Some("ws-victim"),
        )
        .await;

        let app = crate::build_router(state);
        let (status, json) = get_json(
            &app,
            "/api/v1/analytics/top?dimension=agent&since=0&workspace_id=ws-own",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1, "only the own-workspace agent ranks: {arr:?}");
        assert_eq!(arr[0]["agent_id"], "agent-own");

        // Cross-tenant workspace: Forbidden — no leaked ranking.
        let (status, _) = get_json(
            &app,
            "/api/v1/analytics/top?dimension=agent&since=0&workspace_id=ws-victim",
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // Unknown workspace: NotFound, not a silent empty ranking.
        let (status, _) = get_json(
            &app,
            "/api/v1/analytics/top?dimension=agent&since=0&workspace_id=ws-nope",
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn top_dimension_rejects_unknown() {
        let app = app();
        let (status, body) = get_json(&app, "/api/v1/analytics/top?dimension=bogus").await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");
    }

    #[tokio::test]
    async fn count_property_filter_matches_property_equality() {
        // Funnel support: a `task.status_changed(InProgress)` step counts
        // only the events whose `new_status` property is `InProgress`.
        let state = test_state();
        seed_agent_event(
            &state,
            "task.status_changed",
            Some("agent-a"),
            json!({"new_status": "InProgress"}),
            None,
        )
        .await;
        seed_agent_event(
            &state,
            "task.status_changed",
            Some("agent-a"),
            json!({"new_status": "InProgress"}),
            None,
        )
        .await;
        seed_agent_event(
            &state,
            "task.status_changed",
            Some("agent-a"),
            json!({"new_status": "Blocked"}),
            None,
        )
        .await;

        let app = crate::build_router(state);
        let (status, json) = get_json(
            &app,
            "/api/v1/analytics/count?event_name=task.status_changed&since=0&property=new_status:InProgress",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["count"], 2, "body: {json}");

        // Without the property filter the count is the raw total.
        let (status, json) = get_json(
            &app,
            "/api/v1/analytics/count?event_name=task.status_changed&since=0",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["count"], 3);

        // since/until are now optional (default 0..now) — the dashboard
        // funnel fetches rely on the default window.
        let (status, json) = get_json(
            &app,
            "/api/v1/analytics/count?event_name=task.status_changed",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["count"], 3);

        // Malformed property filter: 400, not a silent zero count.
        let (status, _) = get_json(
            &app,
            "/api/v1/analytics/count?event_name=task.status_changed&property=new_status",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
