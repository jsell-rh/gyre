//! Analytics Decision API domain types and evaluators
//! (analytics.md §Decision API for Agents).
//!
//! Agents use analytics to make autonomous data-driven decisions — should
//! this feature flag be promoted, did this change degrade the merge queue,
//! is this agent persona performing within norms. This module holds the
//! decision types and the six built-in decision evaluators plus the custom
//! rule-DSL parser/evaluator.
//!
//! Every evaluator is a **pure function over analytics data**: it takes
//! already-fetched [`AnalyticsEvent`]s (and, for `budget.headroom`, the
//! budget config/usage snapshot) plus the decision context and `now`, and
//! returns the decision-specific outcome object. There is no storage
//! access here — the server fetches the events through the analytics port
//! and hands them in, keeping the hexagonal boundary intact.
//!
//! ## Semantics
//!
//! The spec names the decisions and their response shapes but not the
//! exact metric derivations, so each evaluator below documents the
//! concrete computation it performs over the auto-emitted event schema
//! (analytics.md §Auto-Emitted Events). Default thresholds live in the
//! evaluator and can be overridden through the decision `context`.

use crate::{AnalyticsEvent, BudgetConfig, BudgetUsage};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

/// A decision evaluation request: the named decision plus its context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRequest {
    pub decision: String,
    pub context: Value,
}

/// The result of a decision evaluation. `outcome` carries the
/// decision-specific fields (e.g. `promote`, `confidence`, `metrics` for
/// `feature.promote`); `evaluated_at` is unix seconds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionResult {
    pub decision: String,
    pub outcome: Value,
    pub reason: String,
    pub evaluated_at: u64,
}

impl DecisionResult {
    /// Serialize to the spec response shape (analytics.md §Decision API):
    /// the decision-specific outcome fields flattened alongside
    /// `decision`, `reason`, and an ISO8601 `evaluated_at` timestamp.
    pub fn response_json(&self) -> Value {
        let mut obj = Map::new();
        obj.insert("decision".into(), json!(self.decision));
        if let Some(outcome) = self.outcome.as_object() {
            for (k, v) in outcome {
                obj.insert(k.clone(), v.clone());
            }
        }
        if !obj.contains_key("reason") {
            obj.insert("reason".into(), json!(self.reason));
        }
        obj.insert(
            "evaluated_at".into(),
            json!(format_iso8601_utc(self.evaluated_at)),
        );
        Value::Object(obj)
    }
}

// ─── Built-in decisions ───────────────────────────────────────────────────────

/// The six built-in decisions (analytics.md §`GET /api/v1/analytics/decide`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinDecision {
    FeaturePromote,
    FeatureRollback,
    GateEffectiveness,
    AgentPerformance,
    BudgetHeadroom,
    MergeQueueHealth,
}

impl BuiltinDecision {
    pub fn name(&self) -> &'static str {
        match self {
            Self::FeaturePromote => "feature.promote",
            Self::FeatureRollback => "feature.rollback",
            Self::GateEffectiveness => "gate.effectiveness",
            Self::AgentPerformance => "agent.performance",
            Self::BudgetHeadroom => "budget.headroom",
            Self::MergeQueueHealth => "merge_queue.health",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "feature.promote" => Self::FeaturePromote,
            "feature.rollback" => Self::FeatureRollback,
            "gate.effectiveness" => Self::GateEffectiveness,
            "agent.performance" => Self::AgentPerformance,
            "budget.headroom" => Self::BudgetHeadroom,
            "merge_queue.health" => Self::MergeQueueHealth,
            _ => return None,
        })
    }

    /// All built-in decision names, for error messages.
    pub fn all_names() -> &'static [&'static str] {
        &[
            "feature.promote",
            "feature.rollback",
            "gate.effectiveness",
            "agent.performance",
            "budget.headroom",
            "merge_queue.health",
        ]
    }

    /// The analytics event names this decision's evaluator consumes.
    /// Empty for `budget.headroom` (it reads the budget stores instead).
    pub fn event_names(&self) -> &'static [&'static str] {
        match self {
            Self::FeaturePromote => &["feature.used", "feature.error"],
            Self::FeatureRollback => &["gate.passed", "gate.failed"],
            Self::GateEffectiveness => &["gate.passed", "gate.failed"],
            Self::AgentPerformance => &["agent.completed", "agent.failed"],
            Self::BudgetHeadroom => &[],
            Self::MergeQueueHealth => &["merge_queue.processed"],
        }
    }

    /// Earliest event timestamp the evaluator needs given `context`
    /// (evaluators compare a current window against the equal-length
    /// window before it, so the fetch range is twice the window).
    pub fn fetch_since(&self, context: &Value, now: u64) -> u64 {
        match self {
            Self::FeaturePromote => {
                let days = context_u64(context, "baseline_days").unwrap_or(7).max(1);
                now.saturating_sub(2 * days * 86_400)
            }
            Self::FeatureRollback => {
                let mins = context_u64(context, "window_mins").unwrap_or(30).max(1);
                now.saturating_sub(2 * mins * 60)
            }
            Self::GateEffectiveness => {
                let days = context_u64(context, "window_days").unwrap_or(7).max(1);
                now.saturating_sub(days * 86_400)
            }
            Self::AgentPerformance => {
                let days = context_u64(context, "window_days").unwrap_or(7).max(1);
                now.saturating_sub(days * 86_400)
            }
            Self::BudgetHeadroom => now,
            Self::MergeQueueHealth => {
                let hours = context_u64(context, "window_hours").unwrap_or(24).max(1);
                now.saturating_sub(hours * 3_600)
            }
        }
    }

    /// Evaluate the decision. `events` must cover [`Self::event_names`]
    /// from [`Self::fetch_since`] to `now`; `budget` is only read by
    /// `budget.headroom`; `blocked_count` (non-terminal merge queue
    /// entries) is only read by `merge_queue.health`.
    pub fn evaluate(
        &self,
        context: &Value,
        events: &[AnalyticsEvent],
        budget: Option<(&BudgetConfig, Option<&BudgetUsage>)>,
        blocked_count: u64,
        now: u64,
    ) -> Value {
        match self {
            Self::FeaturePromote => evaluate_feature_promote(context, events, now),
            Self::FeatureRollback => evaluate_feature_rollback(context, events, now),
            Self::GateEffectiveness => evaluate_gate_effectiveness(context, events, now),
            Self::AgentPerformance => evaluate_agent_performance(context, events, now),
            Self::BudgetHeadroom => match budget {
                Some((config, usage)) => evaluate_budget_headroom(context, config, usage, now),
                None => json!({
                    "tokens_remaining_pct": 100.0,
                    "cost_remaining_pct": 100.0,
                    "projected_exhaustion_at": "unknown",
                    "reason": "no budget configuration available",
                }),
            },
            Self::MergeQueueHealth => {
                evaluate_merge_queue_health(context, events, blocked_count, now)
            }
        }
    }
}

// ─── Shared helpers ───────────────────────────────────────────────────────────

/// Read a u64 field from a JSON context.
fn context_u64(context: &Value, key: &str) -> Option<u64> {
    context.get(key).and_then(|v| v.as_u64())
}

/// Read an f64 field from a JSON context.
fn context_f64(context: &Value, key: &str) -> Option<f64> {
    context.get(key).and_then(|v| v.as_f64())
}

/// Read a string field from a JSON context.
fn context_str<'a>(context: &'a Value, key: &str) -> Option<&'a str> {
    context.get(key).and_then(|v| v.as_str())
}

/// Confidence score from sample size (acceptance criterion: "confidence
/// scores are computed from sample size and statistical significance").
///
/// Uses the standard-error shrinkage `1 - 1/sqrt(n)`: with n=1 the estimate
/// is worthless (0), it crosses 0.9 at n=100 and is capped at 0.99 — no
/// finite sample gives certainty. Monotonically increasing in n.
fn significance_confidence(sample_size: u64) -> f64 {
    if sample_size == 0 {
        0.0
    } else {
        (1.0 - 1.0 / (sample_size as f64).sqrt()).min(0.99)
    }
}

/// Nearest-rank percentile over an UNSORTED slice of observations.
/// Returns 0.0 for an empty slice.
fn percentile(observations: &mut [f64], p: f64) -> f64 {
    if observations.is_empty() {
        return 0.0;
    }
    observations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = observations.len();
    let rank = ((p / 100.0) * n as f64).ceil() as usize;
    let rank = rank.clamp(1, n);
    observations[rank - 1]
}

/// Format unix seconds as an ISO8601 UTC timestamp
/// (`YYYY-MM-DDTHH:MM:SSZ`, the `evaluated_at` form in analytics.md).
pub fn format_iso8601_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        m,
        d,
        rem / 3_600,
        (rem % 3_600) / 60,
        rem % 60
    )
}

/// Civil-from-days (Howard Hinnant's algorithm) — days since 1970-01-01
/// to (year, month, day), valid for the full proleptic Gregorian range.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
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

/// Parse a rule-DSL window string into seconds: `<n>s`, `<n>m`, `<n>h`,
/// `<n>d`, `<n>w`, or a bare number of seconds (analytics.md
/// §`POST /api/v1/analytics/decide/custom` uses `"7d"`/`"24h"` forms).
pub fn parse_window_secs(window: &str) -> Option<u64> {
    let window = window.trim();
    if window.is_empty() {
        return None;
    }
    let (num_part, mult) = match window.as_bytes()[window.len() - 1] {
        b's' => (&window[..window.len() - 1], 1u64),
        b'm' => (&window[..window.len() - 1], 60),
        b'h' => (&window[..window.len() - 1], 3_600),
        b'd' => (&window[..window.len() - 1], 86_400),
        b'w' => (&window[..window.len() - 1], 604_800),
        _ => (window, 1),
    };
    let n: u64 = num_part.parse().ok()?;
    // Cap at 365 days so a rule can never force an unbounded fetch.
    Some(n.saturating_mul(mult).min(365 * 86_400))
}

/// Extract a numeric property from an event's properties.
fn prop_f64(event: &AnalyticsEvent, key: &str) -> Option<f64> {
    event.properties.get(key).and_then(|v| v.as_f64())
}

/// Extract a string property from an event's properties.
fn prop_str<'a>(event: &'a AnalyticsEvent, key: &str) -> Option<&'a str> {
    event.properties.get(key).and_then(|v| v.as_str())
}

/// The acting subject of an event: user, session, or agent — whichever
/// identifies the actor for adoption/dedup purposes.
fn event_actor(event: &AnalyticsEvent) -> Option<&str> {
    event
        .user_id
        .as_deref()
        .or(event.session_id.as_deref())
        .or(event.agent_id.as_deref())
}

/// Events with the given exact name in `[since, until]`.
fn events_named<'a>(
    events: &'a [AnalyticsEvent],
    name: &str,
    since: u64,
    until: u64,
) -> impl Iterator<Item = &'a AnalyticsEvent> + use<'a> {
    let name = name.to_string();
    events
        .iter()
        .filter(move |e| e.event_name == name && e.timestamp >= since && e.timestamp <= until)
}

// ─── feature.promote ──────────────────────────────────────────────────────────

/// `feature.promote` — should this feature flag be promoted to all users?
///
/// Computation over `feature.used` / `feature.error` events for the flag
/// (identified by `properties.flag`):
///
/// - **adoption_rate**: distinct actors using the flag ÷ distinct actors
///   using ANY feature (`feature.used`, any flag) in the current window —
///   the fraction of active feature users that adopted this flag.
/// - **error_rate_delta**: flag error rate (`feature.error` ÷
///   (`feature.used` + `feature.error`)) in the current window minus the
///   equal-length baseline window before it.
/// - **p99_latency_delta_ms**: p99 of `properties.latency_ms` on the
///   flag's `feature.used` events, current window minus baseline window.
/// - **sample_size**: `feature.used` events for the flag in the current
///   window.
///
/// Promotes when adoption ≥ 50%, error rate did not grow by more than 1
/// point, p99 latency did not grow by more than 100 ms, and the sample is
/// statistically usable (≥ 30 uses). Context overrides:
/// `baseline_days` (7), `min_adoption` (0.5), `max_error_rate_delta`
/// (0.01), `max_p99_latency_delta_ms` (100), `min_sample_size` (30).
fn evaluate_feature_promote(context: &Value, events: &[AnalyticsEvent], now: u64) -> Value {
    let flag = context_str(context, "flag").unwrap_or_default();
    let baseline_days = context_u64(context, "baseline_days").unwrap_or(7).max(1);
    let min_adoption = context_f64(context, "min_adoption").unwrap_or(0.5);
    let max_error_rate_delta = context_f64(context, "max_error_rate_delta").unwrap_or(0.01);
    let max_p99_latency_delta_ms =
        context_f64(context, "max_p99_latency_delta_ms").unwrap_or(100.0);
    let min_sample_size = context_u64(context, "min_sample_size").unwrap_or(30);

    let window = baseline_days * 86_400;
    let cur_since = now.saturating_sub(window);
    let prev_since = now.saturating_sub(2 * window);

    let is_flag = |e: &&AnalyticsEvent| prop_str(e, "flag") == Some(flag);
    let used_cur = events_named(events, "feature.used", cur_since, now)
        .filter(is_flag)
        .count() as u64;
    let used_prev = events_named(events, "feature.used", prev_since, cur_since)
        .filter(is_flag)
        .count() as u64;
    let err_cur = events_named(events, "feature.error", cur_since, now)
        .filter(is_flag)
        .count() as u64;
    let err_prev = events_named(events, "feature.error", prev_since, cur_since)
        .filter(is_flag)
        .count() as u64;

    let error_rate_cur = err_cur as f64 / (used_cur + err_cur).max(1) as f64;
    let error_rate_prev = err_prev as f64 / (used_prev + err_prev).max(1) as f64;
    let error_rate_delta = error_rate_cur - error_rate_prev;

    // Adoption: distinct actors on this flag vs distinct actors on any
    // feature in the same window.
    let flag_actors: HashSet<&str> = events_named(events, "feature.used", cur_since, now)
        .filter(is_flag)
        .filter_map(|e| event_actor(e))
        .collect();
    let all_actors: HashSet<&str> = events_named(events, "feature.used", cur_since, now)
        .filter_map(|e| event_actor(e))
        .collect();
    let adoption_rate = if all_actors.is_empty() {
        0.0
    } else {
        flag_actors.len() as f64 / all_actors.len() as f64
    };

    let mut lat_cur: Vec<f64> = events_named(events, "feature.used", cur_since, now)
        .filter(is_flag)
        .filter_map(|e| prop_f64(e, "latency_ms"))
        .collect();
    let mut lat_prev: Vec<f64> = events_named(events, "feature.used", prev_since, cur_since)
        .filter(is_flag)
        .filter_map(|e| prop_f64(e, "latency_ms"))
        .collect();
    let p99_cur = percentile(&mut lat_cur, 99.0);
    let p99_prev = percentile(&mut lat_prev, 99.0);
    let p99_latency_delta_ms = if lat_cur.is_empty() || lat_prev.is_empty() {
        0.0
    } else {
        p99_cur - p99_prev
    };

    let sample_size = used_cur;
    let promote = sample_size >= min_sample_size
        && adoption_rate >= min_adoption
        && error_rate_delta <= max_error_rate_delta
        && p99_latency_delta_ms <= max_p99_latency_delta_ms;

    let reason = if sample_size < min_sample_size {
        format!(
            "Insufficient sample: {sample_size} uses of flag {flag:?} in last {baseline_days} \
             days (need {min_sample_size}); adoption rate {:.0}%",
            adoption_rate * 100.0
        )
    } else {
        format!(
            "Adoption rate {:.0}% (target: {:.0}%), error rate delta {:+.3}, p99 latency \
             delta {:+.0} ms over {sample_size} uses of flag {flag:?}",
            adoption_rate * 100.0,
            min_adoption * 100.0,
            error_rate_delta,
            p99_latency_delta_ms
        )
    };

    json!({
        "promote": promote,
        "confidence": significance_confidence(sample_size),
        "reason": reason,
        "metrics": {
            "adoption_rate": round4(adoption_rate),
            "error_rate_delta": round4(error_rate_delta),
            "p99_latency_delta_ms": p99_latency_delta_ms,
            "sample_size": sample_size,
        },
    })
}

/// Round to 4 decimal places to keep serialized rates stable in tests.
fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

// ─── feature.rollback ─────────────────────────────────────────────────────────

/// `feature.rollback` — should this feature be rolled back?
///
/// Compares the gate failure rate (`gate.failed` ÷ all gate events) in the
/// last `window_mins` minutes against the equal-length window before it.
/// When `context.flag` is given, gate events explicitly attributed to a
/// DIFFERENT flag (`properties.flag`) are excluded; unattributed gate
/// events count — for the rollback (safety) direction, an unattributed
/// failure may well be caused by this flag, so excluding it would
/// under-count the blast radius.
///
/// Rolls back when the failure rate grew by ≥ 10 points and at least 5
/// gates ran in the window. Context overrides: `window_mins` (30),
/// `min_rate_increase` (0.1), `min_sample_size` (5).
fn evaluate_feature_rollback(context: &Value, events: &[AnalyticsEvent], now: u64) -> Value {
    let flag = context_str(context, "flag");
    let window_mins = context_u64(context, "window_mins").unwrap_or(30).max(1);
    let min_rate_increase = context_f64(context, "min_rate_increase").unwrap_or(0.1);
    let min_sample_size = context_u64(context, "min_sample_size").unwrap_or(5);

    let window = window_mins * 60;
    let cur_since = now.saturating_sub(window);
    let prev_since = now.saturating_sub(2 * window);

    let attributed_to_other = |e: &&AnalyticsEvent| match (flag, prop_str(e, "flag")) {
        (Some(f), Some(p)) => p != f,
        _ => false,
    };
    let gate_events = |since: u64, until: u64| {
        events
            .iter()
            .filter(move |e| {
                (e.event_name == "gate.passed" || e.event_name == "gate.failed")
                    && e.timestamp >= since
                    && e.timestamp <= until
            })
            .filter(|e| !attributed_to_other(e))
    };

    let cur: Vec<&AnalyticsEvent> = gate_events(cur_since, now).collect();
    let prev: Vec<&AnalyticsEvent> = gate_events(prev_since, cur_since).collect();

    let failed_cur = cur.iter().filter(|e| e.event_name == "gate.failed").count();
    let failed_prev = prev
        .iter()
        .filter(|e| e.event_name == "gate.failed")
        .count();
    let after_rate = failed_cur as f64 / cur.len().max(1) as f64;
    let before_rate = failed_prev as f64 / prev.len().max(1) as f64;

    let affected_mrs: HashSet<&str> = cur
        .iter()
        .filter(|e| e.event_name == "gate.failed")
        .filter_map(|e| prop_str(e, "mr_id"))
        .collect();

    let rollback =
        cur.len() as u64 >= min_sample_size && (after_rate - before_rate) >= min_rate_increase;
    let trigger = if rollback {
        "gate.failure_rate_spike"
    } else {
        "none"
    };

    let reason = if rollback {
        format!(
            "Gate failure rate increased from {:.0}% to {:.0}% in last {window_mins} min",
            before_rate * 100.0,
            after_rate * 100.0
        )
    } else {
        format!(
            "No gate failure spike: rate {:.0}% now vs {:.0}% before (rollback needs \
             ≥{:.0}% increase over ≥{min_sample_size} gate runs)",
            after_rate * 100.0,
            before_rate * 100.0,
            min_rate_increase * 100.0
        )
    };

    json!({
        "rollback": rollback,
        "confidence": significance_confidence(cur.len() as u64),
        "reason": reason,
        "trigger": trigger,
        "metrics": {
            "before_rate": round4(before_rate),
            "after_rate": round4(after_rate),
            "affected_mrs": affected_mrs.len(),
        },
    })
}

// ─── gate.effectiveness ───────────────────────────────────────────────────────

/// `gate.effectiveness` — is this quality gate catching real bugs?
///
/// - **catch_rate**: `gate.failed` ÷ all gate events in the window — the
///   fraction of runs where the gate caught something.
/// - **false_positive_rate**: `gate.failed` events marked
///   `properties.false_positive == true` (the failure was later
///   overturned) ÷ all `gate.failed` events.
///
/// Effective when ≥ 5 gate runs occurred, the gate catches ≥ 5% of runs,
/// and ≤ 30% of its failures were false positives. Context overrides:
/// `window_days` (7), `gate_type` (filter on `properties.gate_type`),
/// `min_catch_rate` (0.05), `max_false_positive_rate` (0.3),
/// `min_sample_size` (5).
fn evaluate_gate_effectiveness(context: &Value, events: &[AnalyticsEvent], now: u64) -> Value {
    let window_days = context_u64(context, "window_days").unwrap_or(7).max(1);
    let gate_type = context_str(context, "gate_type");
    let min_catch_rate = context_f64(context, "min_catch_rate").unwrap_or(0.05);
    let max_false_positive_rate = context_f64(context, "max_false_positive_rate").unwrap_or(0.3);
    let min_sample_size = context_u64(context, "min_sample_size").unwrap_or(5);

    let since = now.saturating_sub(window_days * 86_400);
    let matches_type =
        |e: &&AnalyticsEvent| gate_type.is_none_or(|t| prop_str(e, "gate_type") == Some(t));
    let gate_events = |name: &str| {
        events_named(events, name, since, now)
            .filter(matches_type)
            .collect::<Vec<_>>()
    };

    let passed = gate_events("gate.passed");
    let failed = gate_events("gate.failed");
    let total = (passed.len() + failed.len()) as u64;
    let false_positives = failed
        .iter()
        .filter(|e| {
            e.properties
                .get("false_positive")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        })
        .count();

    let catch_rate = failed.len() as f64 / total.max(1) as f64;
    let false_positive_rate = if failed.is_empty() {
        0.0
    } else {
        false_positives as f64 / failed.len() as f64
    };
    let effective = total >= min_sample_size
        && catch_rate >= min_catch_rate
        && false_positive_rate <= max_false_positive_rate;

    let reason = if total < min_sample_size {
        format!("Insufficient data: {total} gate runs in last {window_days} days (need {min_sample_size})")
    } else if effective {
        format!(
            "Gate caught {:.0}% of {} runs with {:.0}% false positives",
            catch_rate * 100.0,
            total,
            false_positive_rate * 100.0
        )
    } else {
        format!(
            "Gate not effective: catch rate {:.0}% (min {:.0}%), false positive rate \
             {:.0}% (max {:.0}%) over {total} runs",
            catch_rate * 100.0,
            min_catch_rate * 100.0,
            false_positive_rate * 100.0,
            max_false_positive_rate * 100.0
        )
    };

    json!({
        "effective": effective,
        "confidence": significance_confidence(total),
        "reason": reason,
        "false_positive_rate": round4(false_positive_rate),
        "catch_rate": round4(catch_rate),
        "sample_size": total,
    })
}

// ─── agent.performance ────────────────────────────────────────────────────────

/// `agent.performance` — is this agent persona performing within norms?
///
/// Over `agent.completed` (durations from `properties.duration_secs`) and
/// `agent.failed` events in the window: **failure_rate** = failed ÷ total,
/// **p50/p99_duration_secs** = percentiles of the completion durations.
/// Healthy when the failure rate ≤ 20% and p99 ≤ 3600 s; no activity in
/// the window is healthy by default (nothing is wrong). Context overrides:
/// `window_days` (7), `agent_id` (filter), `persona` (filter on
/// `properties.persona`), `max_failure_rate` (0.2), `max_p99_secs`
/// (3600).
fn evaluate_agent_performance(context: &Value, events: &[AnalyticsEvent], now: u64) -> Value {
    let window_days = context_u64(context, "window_days").unwrap_or(7).max(1);
    let agent_filter = context_str(context, "agent_id");
    let persona = context_str(context, "persona");
    let max_failure_rate = context_f64(context, "max_failure_rate").unwrap_or(0.2);
    let max_p99_secs = context_f64(context, "max_p99_secs").unwrap_or(3600.0);

    let since = now.saturating_sub(window_days * 86_400);
    let matches_agent = |e: &&AnalyticsEvent| {
        agent_filter.is_none_or(|a| e.agent_id.as_deref() == Some(a))
            && persona.is_none_or(|p| prop_str(e, "persona") == Some(p))
    };
    let completed = events_named(events, "agent.completed", since, now)
        .filter(matches_agent)
        .count() as u64;
    let failed = events_named(events, "agent.failed", since, now)
        .filter(matches_agent)
        .count() as u64;

    let mut durations: Vec<f64> = events_named(events, "agent.completed", since, now)
        .filter(matches_agent)
        .filter_map(|e| prop_f64(e, "duration_secs"))
        .collect();
    // percentile() sorts in place; both ranks read the same distribution.
    let p50 = percentile(&mut durations, 50.0);
    let p99 = percentile(&mut durations, 99.0);

    let total = completed + failed;
    let failure_rate = failed as f64 / total.max(1) as f64;
    let healthy = total == 0 || (failure_rate <= max_failure_rate && p99 <= max_p99_secs);

    let reason = if total == 0 {
        format!("No agent activity in last {window_days} days — within norms by default")
    } else {
        format!(
            "Failure rate {:.0}% (max {:.0}%), p99 duration {:.0}s (max {:.0}s) over \
             {total} runs",
            failure_rate * 100.0,
            max_failure_rate * 100.0,
            p99,
            max_p99_secs
        )
    };

    json!({
        "healthy": healthy,
        "confidence": significance_confidence(total),
        "reason": reason,
        "p50_duration_secs": p50,
        "p99_duration_secs": p99,
        "failure_rate": round4(failure_rate),
        "sample_size": total,
    })
}

// ─── budget.headroom ──────────────────────────────────────────────────────────

/// `budget.headroom` — how much budget headroom does this workspace have?
///
/// Reads the workspace's effective budget config (daily limits) and its
/// real-time usage snapshot: **tokens_remaining_pct** /
/// **cost_remaining_pct** are `100 × (1 − used/limit)` clamped to [0, 100]
/// (100 when no limit is configured), and
/// **projected_exhaustion_at** extrapolates the consumption rate
/// (`used ÷ seconds since period_start`) to the earlier of the two
/// limits' exhaustion times — `"none"` when a limit can never be reached
/// (no limit or zero burn rate), `"unknown"` when no usage snapshot
/// exists yet.
fn evaluate_budget_headroom(
    _context: &Value,
    config: &BudgetConfig,
    usage: Option<&BudgetUsage>,
    now: u64,
) -> Value {
    let (tokens_used, cost_used, period_start) = match usage {
        Some(u) => (u.tokens_used_today, u.cost_today, u.period_start),
        None => (0, 0.0, now),
    };

    let tokens_remaining_pct = remaining_pct(
        tokens_used as f64,
        config.max_tokens_per_day.map(|v| v as f64),
    );
    let cost_remaining_pct = remaining_pct(cost_used, config.max_cost_per_day);

    // Projected exhaustion: earliest of the token and cost limits, from
    // the observed burn rate since the daily period started.
    let elapsed = now.saturating_sub(period_start).max(1) as f64;
    let eta_for = |used: f64, limit: Option<f64>| -> Option<u64> {
        let limit = limit?;
        if limit <= 0.0 || used >= limit || used <= 0.0 {
            return None; // no limit, already exhausted, or no burn rate
        }
        let rate = used / elapsed; // units per second
        Some(now + ((limit - used) / rate) as u64)
    };
    let projected = match usage {
        None => "unknown".to_string(),
        Some(_) => {
            let etas = [
                eta_for(
                    tokens_used as f64,
                    config.max_tokens_per_day.map(|v| v as f64),
                ),
                eta_for(cost_used, config.max_cost_per_day),
            ];
            match etas.into_iter().flatten().min() {
                Some(eta) => format_iso8601_utc(eta),
                None => "none".to_string(),
            }
        }
    };

    let reason = format!(
        "Tokens {:.0}% remaining, cost {:.0}% remaining; projected exhaustion: {projected}",
        tokens_remaining_pct, cost_remaining_pct
    );

    json!({
        "tokens_remaining_pct": round4(tokens_remaining_pct),
        "cost_remaining_pct": round4(cost_remaining_pct),
        "projected_exhaustion_at": projected,
        "reason": reason,
        "tokens_used_today": tokens_used,
        "cost_today": cost_used,
    })
}

fn remaining_pct(used: f64, limit: Option<f64>) -> f64 {
    match limit {
        Some(limit) if limit > 0.0 => (100.0 * (1.0 - used / limit)).clamp(0.0, 100.0),
        _ => 100.0,
    }
}

// ─── merge_queue.health ───────────────────────────────────────────────────────

/// `merge_queue.health` — is the merge queue healthy or degraded?
///
/// Over `merge_queue.processed` events in the window:
/// **failure_rate** = `outcome == "failed"` ÷ total, **avg_wait_secs** =
/// mean of `properties.wait_secs`. **blocked_count** is the number of
/// currently non-terminal queue entries (passed in by the server from the
/// merge queue port). Healthy when the failure rate ≤ 20%, average wait
/// ≤ 3600 s, and ≤ 10 entries blocked; an idle queue (no processing in
/// the window) is healthy. Context overrides: `window_hours` (24),
/// `max_failure_rate` (0.2), `max_avg_wait_secs` (3600),
/// `max_blocked_count` (10).
fn evaluate_merge_queue_health(
    context: &Value,
    events: &[AnalyticsEvent],
    blocked_count: u64,
    now: u64,
) -> Value {
    let window_hours = context_u64(context, "window_hours").unwrap_or(24).max(1);
    let max_failure_rate = context_f64(context, "max_failure_rate").unwrap_or(0.2);
    let max_avg_wait_secs = context_f64(context, "max_avg_wait_secs").unwrap_or(3600.0);
    let max_blocked_count = context_u64(context, "max_blocked_count").unwrap_or(10);

    let since = now.saturating_sub(window_hours * 3_600);
    let processed: Vec<&AnalyticsEvent> =
        events_named(events, "merge_queue.processed", since, now).collect();

    let total = processed.len() as u64;
    let failed = processed
        .iter()
        .filter(|e| prop_str(e, "outcome") == Some("failed"))
        .count() as u64;
    let waits: Vec<f64> = processed
        .iter()
        .filter_map(|e| prop_f64(e, "wait_secs"))
        .collect();
    let avg_wait_secs = if waits.is_empty() {
        0.0
    } else {
        waits.iter().sum::<f64>() / waits.len() as f64
    };
    let failure_rate = failed as f64 / total.max(1) as f64;

    let healthy = total == 0
        || (failure_rate <= max_failure_rate
            && avg_wait_secs <= max_avg_wait_secs
            && blocked_count <= max_blocked_count);

    let reason = if total == 0 {
        format!("Merge queue idle: no entries processed in last {window_hours}h, {blocked_count} blocked")
    } else {
        format!(
            "Failure rate {:.0}% (max {:.0}%), avg wait {:.0}s (max {:.0}s), {blocked_count} \
             blocked over {total} processed",
            failure_rate * 100.0,
            max_failure_rate * 100.0,
            avg_wait_secs,
            max_avg_wait_secs
        )
    };

    json!({
        "healthy": healthy,
        "confidence": significance_confidence(total),
        "reason": reason,
        "avg_wait_secs": round4(avg_wait_secs),
        "failure_rate": round4(failure_rate),
        "blocked_count": blocked_count,
        "sample_size": total,
    })
}

// ─── Custom decision rule DSL ─────────────────────────────────────────────────

/// One rule in a custom decision (analytics.md
/// §`POST /api/v1/analytics/decide/custom`): count events of `metric`
/// inside `window`, optionally narrowed by `filter` (property equality
/// match), compare the count against `threshold` with `comparison`, and
/// on match produce `then`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomRule {
    /// Event name to count, e.g. "spec.approved".
    pub metric: String,
    /// Window string, e.g. "7d", "24h", "30m".
    pub window: String,
    /// Count threshold the rule compares against.
    pub threshold: f64,
    /// Comparison operator: `gte`, `gt`, `lte`, `lt`, `eq`.
    pub comparison: String,
    /// Outcome produced when the rule matches.
    pub then: Value,
    /// Optional property equality filter, e.g. `{"outcome": "failed"}`.
    #[serde(default)]
    pub filter: Option<Value>,
}

/// A custom decision definition: named rules + a default outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomDecisionSpec {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub rules: Vec<CustomRule>,
    #[serde(default = "default_outcome")]
    pub default: Value,
    #[serde(default)]
    pub context: Value,
}

fn default_outcome() -> Value {
    Value::String("default".into())
}

/// Comparison operators supported by the rule DSL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    Gte,
    Gt,
    Lte,
    Lt,
    Eq,
}

impl Comparison {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "gte" => Self::Gte,
            "gt" => Self::Gt,
            "lte" => Self::Lte,
            "lt" => Self::Lt,
            "eq" => Self::Eq,
            _ => return None,
        })
    }

    pub fn matches(&self, count: f64, threshold: f64) -> bool {
        match self {
            Self::Gte => count >= threshold,
            Self::Gt => count > threshold,
            Self::Lte => count <= threshold,
            Self::Lt => count < threshold,
            Self::Eq => (count - threshold).abs() < f64::EPSILON,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Gte => "gte",
            Self::Gt => "gt",
            Self::Lte => "lte",
            Self::Lt => "lt",
            Self::Eq => "eq",
        }
    }
}

impl CustomDecisionSpec {
    /// Parse and validate a custom decision definition from JSON.
    /// Returns a human-readable error for every invalid field so agents
    /// can fix their rule without a round trip.
    pub fn parse(body: &Value) -> Result<Self, String> {
        let spec: CustomDecisionSpec = serde_json::from_value(body.clone())
            .map_err(|e| format!("invalid decision body: {e}"))?;
        if spec.name.trim().is_empty() {
            return Err("name must be a non-empty string".into());
        }
        for (i, rule) in spec.rules.iter().enumerate() {
            if rule.metric.trim().is_empty() {
                return Err(format!("rules[{i}].metric must be a non-empty event name"));
            }
            if parse_window_secs(&rule.window).is_none() {
                return Err(format!(
                    "rules[{i}].window {:?} is invalid: expected e.g. \"7d\", \"24h\", \"30m\"",
                    rule.window
                ));
            }
            if Comparison::parse(&rule.comparison).is_none() {
                return Err(format!(
                    "rules[{i}].comparison {:?} is invalid: expected gte, gt, lte, lt, or eq",
                    rule.comparison
                ));
            }
            if let Some(filter) = &rule.filter {
                if !filter.is_object() {
                    return Err(format!(
                        "rules[{i}].filter must be an object of property equalities"
                    ));
                }
            }
        }
        Ok(spec)
    }

    /// Distinct metric names referenced by the rules — the event queries
    /// the caller must run before evaluating.
    pub fn metrics(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut metrics: Vec<String> = Vec::new();
        for rule in &self.rules {
            if seen.insert(rule.metric.clone()) {
                metrics.push(rule.metric.clone());
            }
        }
        metrics
    }

    /// Widest window (in seconds) among the rules referencing `metric` —
    /// the fetch window for that metric's events.
    pub fn fetch_window_for(&self, metric: &str) -> Option<u64> {
        self.rules
            .iter()
            .filter(|r| r.metric == metric)
            .filter_map(|r| parse_window_secs(&r.window))
            .max()
    }
}

/// Does the event match the rule's property-equality filter?
fn matches_filter(event: &AnalyticsEvent, filter: Option<&Value>) -> bool {
    let Some(filter) = filter.and_then(|f| f.as_object()) else {
        return true;
    };
    filter
        .iter()
        .all(|(k, v)| event.properties.get(k) == Some(v))
}

/// Evaluate a custom decision (analytics.md §`POST
/// /api/v1/analytics/decide/custom`): the FIRST rule whose metric count
/// in its window matches its threshold comparison produces the outcome;
/// when no rule matches, the decision's `default` is the outcome.
///
/// `events_by_metric` maps each rule metric to the events fetched for it
/// (covering the widest window among that metric's rules); the evaluator
/// applies each rule's own window and filter. Returns the outcome object
/// with `outcome`, `matched_rule`, and `reason` fields.
pub fn evaluate_custom(
    spec: &CustomDecisionSpec,
    events_by_metric: &HashMap<String, Vec<AnalyticsEvent>>,
    now: u64,
) -> Value {
    let mut rule_details: Vec<String> = Vec::new();
    for (i, rule) in spec.rules.iter().enumerate() {
        let window_secs = parse_window_secs(&rule.window).unwrap_or(0);
        let since = now.saturating_sub(window_secs);
        let events = events_by_metric.get(&rule.metric);
        let count = events
            .map(|evts| {
                evts.iter()
                    .filter(|e| e.timestamp >= since && e.timestamp <= now)
                    .filter(|e| matches_filter(e, rule.filter.as_ref()))
                    .count()
            })
            .unwrap_or(0) as f64;
        let comparison = Comparison::parse(&rule.comparison);
        let Some(comparison) = comparison else {
            continue; // parse() validated; unreachable via the API
        };
        let matched = comparison.matches(count, rule.threshold);
        let detail = format!(
            "{} {} {} in last {}: count {}",
            rule.metric,
            comparison.as_str(),
            rule.threshold,
            rule.window,
            count
        );
        if matched {
            return json!({
                "outcome": rule.then,
                "matched_rule": i,
                "reason": format!("rule {i} matched: {detail}"),
            });
        }
        rule_details.push(format!("rule {i} not matched: {detail}"));
    }
    let reason = if rule_details.is_empty() {
        "no rules defined; default outcome".to_string()
    } else {
        rule_details.join("; ")
    };
    json!({
        "outcome": spec.default,
        "matched_rule": Value::Null,
        "reason": reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::Id;

    fn ev(name: &str, props: Value, timestamp: u64) -> AnalyticsEvent {
        AnalyticsEvent::new(Id::new("e"), name, None, props, timestamp)
    }

    fn ev_scoped(name: &str, props: Value, timestamp: u64, actor: &str) -> AnalyticsEvent {
        let mut e = ev(name, props, timestamp);
        e.user_id = Some(actor.to_string());
        e
    }

    const NOW: u64 = 1_800_000_000;

    // ─── helpers ──────────────────────────────────────────────────────────────

    #[test]
    fn iso8601_formatting() {
        assert_eq!(format_iso8601_utc(0), "1970-01-01T00:00:00Z");
        // Well-known instant: 2023-11-14T22:13:20Z.
        assert_eq!(format_iso8601_utc(1_700_000_000), "2023-11-14T22:13:20Z");
        // Leap-year day: 2024-02-29T12:00:00Z = 1709208000.
        assert_eq!(format_iso8601_utc(1_709_208_000), "2024-02-29T12:00:00Z");
    }

    #[test]
    fn window_parsing() {
        assert_eq!(parse_window_secs("7d"), Some(7 * 86_400));
        assert_eq!(parse_window_secs("24h"), Some(24 * 3_600));
        assert_eq!(parse_window_secs("30m"), Some(1_800));
        assert_eq!(parse_window_secs("45s"), Some(45));
        assert_eq!(parse_window_secs("2w"), Some(1_209_600));
        assert_eq!(parse_window_secs("90"), Some(90));
        assert_eq!(parse_window_secs(""), None);
        assert_eq!(parse_window_secs("abc"), None);
        assert_eq!(parse_window_secs("7x"), None);
    }

    #[test]
    fn percentile_nearest_rank() {
        let mut v: Vec<f64> = (1..=100).map(|i| i as f64).collect();
        assert_eq!(percentile(&mut v, 50.0), 50.0);
        assert_eq!(percentile(&mut v, 99.0), 99.0);
        assert_eq!(percentile(&mut v, 100.0), 100.0);
        let mut one = vec![42.0];
        assert_eq!(percentile(&mut one, 99.0), 42.0);
        let mut none: Vec<f64> = vec![];
        assert_eq!(percentile(&mut none, 50.0), 0.0);
    }

    #[test]
    fn significance_confidence_grows_with_sample() {
        assert_eq!(significance_confidence(0), 0.0);
        assert_eq!(significance_confidence(1), 0.0);
        let c100 = significance_confidence(100);
        let c1000 = significance_confidence(1000);
        assert!(c100 >= 0.9);
        assert!(c1000 > c100);
        assert!(c1000 <= 0.99);
    }

    // ─── feature.promote ──────────────────────────────────────────────────────

    #[test]
    fn feature_promote_promotes_on_healthy_metrics() {
        let day = 86_400u64;
        let cur = NOW - day; // inside the 7-day current window
        let mut events = Vec::new();
        // 100 flag users, 120 all-feature users → adoption 100/120 ≈ 0.83.
        for i in 0..100 {
            events.push(ev_scoped(
                "feature.used",
                json!({"flag": "new-diff-viewer", "latency_ms": 100 + i % 10}),
                cur,
                &format!("user-{i}"),
            ));
        }
        for i in 0..20 {
            events.push(ev_scoped(
                "feature.used",
                json!({"flag": "other-flag", "latency_ms": 90}),
                cur,
                &format!("other-{i}"),
            ));
        }
        // Baseline window: 100 uses at 110ms → p99 delta ≈ -1..+9 ms, no errors.
        for i in 0..100 {
            events.push(ev_scoped(
                "feature.used",
                json!({"flag": "new-diff-viewer", "latency_ms": 100 + i % 10}),
                NOW - 8 * day,
                &format!("old-{i}"),
            ));
        }

        let ctx = json!({"flag": "new-diff-viewer", "baseline_days": 7, "min_sample_size": 30});
        let out = evaluate_feature_promote(&ctx, &events, NOW);
        assert_eq!(out["promote"], json!(true), "outcome: {out}");
        assert_eq!(out["metrics"]["sample_size"], json!(100));
        let adoption = out["metrics"]["adoption_rate"].as_f64().unwrap();
        assert!(
            (adoption - 100.0 / 120.0).abs() < 1e-3,
            "adoption {adoption}"
        );
        assert!(out["confidence"].as_f64().unwrap() >= 0.9);
        assert!(out["reason"].as_str().unwrap().contains("Adoption"));
    }

    #[test]
    fn feature_promote_rejects_on_error_spike() {
        let day = 86_400u64;
        let cur = NOW - day;
        let mut events = Vec::new();
        for i in 0..50 {
            events.push(ev_scoped(
                "feature.used",
                json!({"flag": "f", "latency_ms": 100}),
                cur,
                &format!("u{i}"),
            ));
        }
        // 30% error rate now, 0 before → error_rate_delta 0.3 > 0.01.
        for _ in 0..20 {
            events.push(ev("feature.error", json!({"flag": "f"}), cur));
        }
        let ctx = json!({"flag": "f", "min_sample_size": 30});
        let out = evaluate_feature_promote(&ctx, &events, NOW);
        assert_eq!(out["promote"], json!(false), "outcome: {out}");
        assert!(out["metrics"]["error_rate_delta"].as_f64().unwrap() > 0.2);
    }

    #[test]
    fn feature_promote_empty_data_is_not_an_error() {
        let out = evaluate_feature_promote(&json!({"flag": "f"}), &[], NOW);
        assert_eq!(out["promote"], json!(false));
        assert_eq!(out["confidence"], json!(0.0));
        assert_eq!(out["metrics"]["sample_size"], json!(0));
        assert!(out["reason"]
            .as_str()
            .unwrap()
            .contains("Insufficient sample"));
    }

    // ─── feature.rollback ────────────────────────────────────────────────────

    #[test]
    fn feature_rollback_triggers_on_gate_failure_spike() {
        let w = 1_800u64;
        let mut events = Vec::new();
        // Before window: 2% failure (1 failed / 50 passed).
        for i in 0..50 {
            events.push(ev(
                "gate.passed",
                json!({"mr_id": format!("b{i}")}),
                NOW - 2 * w,
            ));
        }
        events.push(ev("gate.failed", json!({"mr_id": "bf"}), NOW - 2 * w));
        // Current window: 34% failure (25 failed / 48 passed → ~0.34).
        for i in 0..48 {
            events.push(ev(
                "gate.passed",
                json!({"mr_id": format!("c{i}")}),
                NOW - w / 2,
            ));
        }
        for i in 0..25 {
            events.push(ev(
                "gate.failed",
                json!({"mr_id": format!("cf{i}")}),
                NOW - w / 2,
            ));
        }

        let ctx = json!({"flag": "optimistic-merge", "window_mins": 30});
        let out = evaluate_feature_rollback(&ctx, &events, NOW);
        assert_eq!(out["rollback"], json!(true), "outcome: {out}");
        assert_eq!(out["trigger"], json!("gate.failure_rate_spike"));
        assert!(out["metrics"]["before_rate"].as_f64().unwrap() < 0.05);
        assert!(out["metrics"]["after_rate"].as_f64().unwrap() > 0.3);
        assert_eq!(out["metrics"]["affected_mrs"], json!(25));
    }

    #[test]
    fn feature_rollback_quiet_with_no_spike() {
        let w = 1_800u64;
        let events: Vec<AnalyticsEvent> = (0..20)
            .map(|i| ev("gate.passed", json!({"mr_id": i}), NOW - w / 2))
            .collect();
        let out = evaluate_feature_rollback(&json!({}), &events, NOW);
        assert_eq!(out["rollback"], json!(false));
        assert_eq!(out["trigger"], json!("none"));
    }

    // ─── gate.effectiveness ──────────────────────────────────────────────────

    #[test]
    fn gate_effectiveness_catches_real_bugs() {
        let mut events = Vec::new();
        for i in 0..90 {
            events.push(ev(
                "gate.passed",
                json!({"gate_type": "lint", "mr_id": i}),
                NOW - 100,
            ));
        }
        for i in 0..10 {
            events.push(ev(
                "gate.failed",
                json!({"gate_type": "lint", "mr_id": 100 + i, "false_positive": false}),
                NOW - 100,
            ));
        }
        let out = evaluate_gate_effectiveness(&json!({}), &events, NOW);
        assert_eq!(out["effective"], json!(true), "outcome: {out}");
        assert_eq!(out["catch_rate"], json!(0.1));
        assert_eq!(out["false_positive_rate"], json!(0.0));
    }

    #[test]
    fn gate_effectiveness_flags_false_positives() {
        let mut events = Vec::new();
        for i in 0..95 {
            events.push(ev(
                "gate.passed",
                json!({"gate_type": "lint", "mr_id": i}),
                NOW - 100,
            ));
        }
        for i in 0..5 {
            events.push(ev(
                "gate.failed",
                json!({"gate_type": "lint", "mr_id": 100 + i, "false_positive": true}),
                NOW - 100,
            ));
        }
        let out = evaluate_gate_effectiveness(&json!({}), &events, NOW);
        assert_eq!(out["effective"], json!(false), "outcome: {out}");
        assert_eq!(out["false_positive_rate"], json!(1.0));
    }

    // ─── agent.performance ───────────────────────────────────────────────────

    #[test]
    fn agent_performance_healthy_within_norms() {
        let mut events = Vec::new();
        // Durations clustered around 100s: p99 well under 3600.
        for i in 0..90 {
            events.push(ev(
                "agent.completed",
                json!({"duration_secs": 80 + i % 40, "persona": "implementer"}),
                NOW - 100,
            ));
        }
        for _ in 0..10 {
            events.push(ev(
                "agent.failed",
                json!({"persona": "implementer"}),
                NOW - 100,
            ));
        }
        let out = evaluate_agent_performance(&json!({"persona": "implementer"}), &events, NOW);
        assert_eq!(out["healthy"], json!(true), "outcome: {out}");
        assert_eq!(out["failure_rate"], json!(0.1));
        let p50 = out["p50_duration_secs"].as_f64().unwrap();
        assert!((80.0..=120.0).contains(&p50), "p50 {p50}");
        assert!(out["p99_duration_secs"].as_f64().unwrap() > p50);
    }

    #[test]
    fn agent_performance_degraded_on_failures() {
        let mut events = Vec::new();
        for _ in 0..50 {
            events.push(ev(
                "agent.completed",
                json!({"duration_secs": 100}),
                NOW - 100,
            ));
        }
        for _ in 0..50 {
            events.push(ev("agent.failed", json!({}), NOW - 100));
        }
        let out = evaluate_agent_performance(&json!({}), &events, NOW);
        assert_eq!(out["healthy"], json!(false), "outcome: {out}");
        assert_eq!(out["failure_rate"], json!(0.5));
    }

    #[test]
    fn agent_performance_empty_is_healthy() {
        let out = evaluate_agent_performance(&json!({}), &[], NOW);
        assert_eq!(out["healthy"], json!(true));
        assert_eq!(out["failure_rate"], json!(0.0));
        assert_eq!(out["p50_duration_secs"], json!(0.0));
    }

    // ─── budget.headroom ─────────────────────────────────────────────────────

    #[test]
    fn budget_headroom_projects_exhaustion() {
        let config = BudgetConfig {
            max_tokens_per_day: Some(10_000),
            max_cost_per_day: Some(50.0),
            ..Default::default()
        };
        let usage = BudgetUsage {
            entity_type: "workspace".into(),
            entity_id: Id::new("ws"),
            tokens_used_today: 2_500,
            cost_today: 10.0,
            active_agents: 2,
            period_start: NOW - 3_600,
            compute_minutes: 30,
        };
        let out = evaluate_budget_headroom(&json!({}), &config, Some(&usage), NOW);
        assert_eq!(out["tokens_remaining_pct"], json!(75.0), "outcome: {out}");
        assert_eq!(out["cost_remaining_pct"], json!(80.0));
        // Token burn 2500/h → 7500 remaining → 3h → NOW + 10800.
        assert_eq!(
            out["projected_exhaustion_at"],
            json!(format_iso8601_utc(NOW + 10_800))
        );
    }

    #[test]
    fn budget_headroom_no_limit_never_exhausts() {
        let out = evaluate_budget_headroom(&json!({}), &BudgetConfig::default(), None, NOW);
        assert_eq!(out["tokens_remaining_pct"], json!(100.0));
        assert_eq!(out["cost_remaining_pct"], json!(100.0));
        assert_eq!(out["projected_exhaustion_at"], json!("unknown"));
    }

    // ─── merge_queue.health ──────────────────────────────────────────────────

    #[test]
    fn merge_queue_health_healthy() {
        let events: Vec<AnalyticsEvent> = (0..90)
            .map(|i| {
                ev(
                    "merge_queue.processed",
                    json!({"mr_id": i, "outcome": "merged", "wait_secs": 300 + i % 60}),
                    NOW - 1_000,
                )
            })
            .chain((0..10).map(|i| {
                ev(
                    "merge_queue.processed",
                    json!({"mr_id": 100 + i, "outcome": "failed", "wait_secs": 400}),
                    NOW - 1_000,
                )
            }))
            .collect();
        let out = evaluate_merge_queue_health(&json!({}), &events, 2, NOW);
        assert_eq!(out["healthy"], json!(true), "outcome: {out}");
        assert_eq!(out["failure_rate"], json!(0.1));
        assert_eq!(out["blocked_count"], json!(2));
        let avg = out["avg_wait_secs"].as_f64().unwrap();
        assert!(avg > 300.0 && avg < 400.0, "avg {avg}");
    }

    #[test]
    fn merge_queue_health_degraded_on_failures() {
        let events: Vec<AnalyticsEvent> = (0..50)
            .map(|i| {
                ev(
                    "merge_queue.processed",
                    json!({"mr_id": i, "outcome": "failed", "wait_secs": 500}),
                    NOW - 1_000,
                )
            })
            .collect();
        let out = evaluate_merge_queue_health(&json!({}), &events, 0, NOW);
        assert_eq!(out["healthy"], json!(false), "outcome: {out}");
        assert_eq!(out["failure_rate"], json!(1.0));
    }

    #[test]
    fn merge_queue_health_idle_is_healthy() {
        let out = evaluate_merge_queue_health(&json!({}), &[], 0, NOW);
        assert_eq!(out["healthy"], json!(true));
        assert_eq!(out["failure_rate"], json!(0.0));
        assert_eq!(out["avg_wait_secs"], json!(0.0));
    }

    // ─── custom rule DSL ─────────────────────────────────────────────────────

    fn spec(body: Value) -> CustomDecisionSpec {
        CustomDecisionSpec::parse(&body).expect("valid spec")
    }

    #[test]
    fn custom_spec_parse_and_validate() {
        let body = json!({
            "name": "spec-approval-pace",
            "rules": [
                {"metric": "spec.approved", "window": "7d", "threshold": 3, "comparison": "gte", "then": "approve"},
                {"metric": "merge_queue.processed", "filter": {"outcome": "failed"}, "window": "24h", "threshold": 5, "comparison": "gte", "then": "escalate"}
            ],
            "default": "hold"
        });
        let s = spec(body);
        assert_eq!(s.name, "spec-approval-pace");
        assert_eq!(s.rules.len(), 2);
        assert_eq!(s.metrics(), vec!["spec.approved", "merge_queue.processed"]);
        assert_eq!(s.fetch_window_for("spec.approved"), Some(7 * 86_400));
        assert_eq!(
            s.fetch_window_for("merge_queue.processed"),
            Some(24 * 3_600)
        );

        // Invalid comparison / window / name rejected with a field error.
        assert!(CustomDecisionSpec::parse(&json!({
            "name": "x", "rules": [{"metric": "m", "window": "7d", "threshold": 1, "comparison": "approx", "then": "y"}]
        })).is_err());
        assert!(CustomDecisionSpec::parse(&json!({
            "name": "x", "rules": [{"metric": "m", "window": "soon", "threshold": 1, "comparison": "gte", "then": "y"}]
        })).is_err());
        assert!(CustomDecisionSpec::parse(&json!({"name": "", "rules": []})).is_err());
    }

    #[test]
    fn custom_eval_first_matching_rule_wins() {
        let s = spec(json!({
            "name": "d",
            "rules": [
                {"metric": "spec.approved", "window": "7d", "threshold": 3, "comparison": "gte", "then": "approve"},
                {"metric": "spec.approved", "window": "7d", "threshold": 1, "comparison": "gte", "then": "fallback"}
            ],
            "default": "hold"
        }));
        let events: Vec<AnalyticsEvent> = (0..5)
            .map(|i| ev("spec.approved", json!({"spec_path": i}), NOW - 100))
            .collect();
        let mut by_metric = HashMap::new();
        by_metric.insert("spec.approved".to_string(), events);
        let out = evaluate_custom(&s, &by_metric, NOW);
        assert_eq!(out["outcome"], json!("approve"));
        assert_eq!(out["matched_rule"], json!(0));
        assert!(out["reason"].as_str().unwrap().contains("rule 0 matched"));
    }

    #[test]
    fn custom_eval_default_when_no_rule_matches() {
        let s = spec(json!({
            "name": "spec-approval-pace",
            "rules": [
                {"metric": "spec.approved", "window": "7d", "threshold": 3, "comparison": "gte", "then": "approve"},
                {"metric": "merge_queue.processed", "filter": {"outcome": "failed"}, "window": "24h", "threshold": 5, "comparison": "gte", "then": "escalate"}
            ],
            "default": "hold"
        }));
        // 2 approvals in window (threshold 3), 0 failed queue entries.
        let approvals: Vec<AnalyticsEvent> = (0..2)
            .map(|i| ev("spec.approved", json!({}), NOW - 100))
            .collect();
        let queue: Vec<AnalyticsEvent> = (0..3)
            .map(|i| {
                ev(
                    "merge_queue.processed",
                    json!({"outcome": "merged", "wait_secs": 10}),
                    NOW - 100,
                )
            })
            .chain(std::iter::once(ev(
                "merge_queue.processed",
                json!({"outcome": "failed", "wait_secs": 10}),
                NOW - 100,
            )))
            .collect();
        let mut by_metric = HashMap::new();
        by_metric.insert("spec.approved".to_string(), approvals);
        by_metric.insert("merge_queue.processed".to_string(), queue);
        let out = evaluate_custom(&s, &by_metric, NOW);
        assert_eq!(out["outcome"], json!("hold"));
        assert!(out["matched_rule"].is_null());
        // The reason names each rule's actual count, like the spec example.
        let reason = out["reason"].as_str().unwrap();
        assert!(reason.contains("count 2"), "reason: {reason}");
        assert!(reason.contains("count 1"), "reason: {reason}");
    }

    #[test]
    fn custom_eval_respects_window_and_filter() {
        // Rule window is 1h; the 5 events are 2h old → no match, default.
        let s = spec(json!({
            "name": "d",
            "rules": [{"metric": "m", "window": "1h", "threshold": 5, "comparison": "gte", "then": "go"}],
            "default": "stay"
        }));
        let events: Vec<AnalyticsEvent> = (0..5)
            .map(|i| ev("m", json!({}), NOW - 2 * 3_600 + i))
            .collect();
        let mut by_metric = HashMap::new();
        by_metric.insert("m".to_string(), events);
        let out = evaluate_custom(&s, &by_metric, NOW);
        assert_eq!(out["outcome"], json!("stay"));

        // Filter narrows the count: 5 events, 2 with outcome=failed, rule
        // needs gte 5 over the filtered set.
        let s = spec(json!({
            "name": "d",
            "rules": [{"metric": "m", "filter": {"outcome": "failed"}, "window": "1h", "threshold": 5, "comparison": "gte", "then": "go"}],
            "default": "stay"
        }));
        let events: Vec<AnalyticsEvent> = (0..5)
            .map(|i| {
                ev(
                    "m",
                    json!({"outcome": if i < 2 { "failed" } else { "merged" }}),
                    NOW - 100,
                )
            })
            .collect();
        let mut by_metric = HashMap::new();
        by_metric.insert("m".to_string(), events);
        let out = evaluate_custom(&s, &by_metric, NOW);
        assert_eq!(out["outcome"], json!("stay"));
        assert!(out["reason"].as_str().unwrap().contains("count 2"));
    }

    // ─── DecisionResult response shape ───────────────────────────────────────

    #[test]
    fn decision_result_flattens_outcome_with_iso_timestamp() {
        let result = DecisionResult {
            decision: "feature.promote".into(),
            outcome: json!({"promote": true, "confidence": 0.9, "reason": "ok", "metrics": {}}),
            reason: "ok".into(),
            evaluated_at: 1_700_000_000,
        };
        let json = result.response_json();
        assert_eq!(json["decision"], json!("feature.promote"));
        assert_eq!(json["promote"], json!(true));
        assert_eq!(json["confidence"], json!(0.9));
        assert_eq!(json["reason"], json!("ok"));
        assert_eq!(json["evaluated_at"], json!("2023-11-14T22:13:20Z"));
    }
}
