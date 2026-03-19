//! Prometheus metrics for gyre-server.
//!
//! Exposed at `GET /metrics` in Prometheus text format.
//! Metrics are stored on `AppState` so they can be updated from middleware and handlers.

use anyhow::Result;
use prometheus::{CounterVec, Gauge, HistogramVec, Opts, Registry};

/// All Prometheus metrics for the server.
pub struct Metrics {
    pub registry: Registry,
    /// Total HTTP requests, labelled by method, path, status code.
    pub http_requests_total: CounterVec,
    /// HTTP request duration in seconds, labelled by method and path.
    pub http_request_duration_seconds: HistogramVec,
    /// Number of agents currently in Active status.
    pub active_agents: Gauge,
    /// Number of entries currently in the merge queue.
    pub merge_queue_depth: Gauge,
}

impl Metrics {
    pub fn new() -> Result<Self> {
        let registry = Registry::new();

        let http_requests_total = CounterVec::new(
            Opts::new(
                "gyre_http_requests_total",
                "Total HTTP requests by method, path, and status code",
            ),
            &["method", "path", "status"],
        )?;
        registry.register(Box::new(http_requests_total.clone()))?;

        let http_request_duration_seconds = HistogramVec::new(
            prometheus::HistogramOpts::new(
                "gyre_http_request_duration_seconds",
                "HTTP request duration in seconds",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]),
            &["method", "path"],
        )?;
        registry.register(Box::new(http_request_duration_seconds.clone()))?;

        let active_agents = Gauge::new("gyre_active_agents", "Number of active agents")?;
        registry.register(Box::new(active_agents.clone()))?;

        let merge_queue_depth = Gauge::new(
            "gyre_merge_queue_depth",
            "Number of entries in the merge queue",
        )?;
        registry.register(Box::new(merge_queue_depth.clone()))?;

        Ok(Self {
            registry,
            http_requests_total,
            http_request_duration_seconds,
            active_agents,
            merge_queue_depth,
        })
    }

    /// Render all metrics in Prometheus text format.
    pub fn render(&self) -> String {
        let encoder = prometheus::TextEncoder::new();
        let families = self.registry.gather();
        encoder.encode_to_string(&families).unwrap_or_default()
    }
}

// ── Handler ───────────────────────────────────────────────────────────────────

use axum::{extract::State, http::StatusCode};
use gyre_domain::AgentStatus;
use std::sync::Arc;

use crate::AppState;

/// GET /metrics — returns Prometheus text format.
pub async fn metrics_handler(State(state): State<Arc<AppState>>) -> Result<String, StatusCode> {
    // Refresh gauges from live state before rendering.
    if let Ok(agents) = state.agents.list().await {
        let active = agents
            .iter()
            .filter(|a| a.status == AgentStatus::Active)
            .count();
        state.metrics.active_agents.set(active as f64);
    }
    if let Ok(queue) = state.merge_queue.list_queue().await {
        state.metrics.merge_queue_depth.set(queue.len() as f64);
    }

    Ok(state.metrics.render())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_new_succeeds() {
        Metrics::new().expect("Metrics::new should not fail");
    }

    #[test]
    fn metrics_render_is_prometheus_format() {
        let m = Metrics::new().unwrap();
        let output = m.render();
        // Prometheus text format starts with "# HELP" or "# TYPE" lines.
        assert!(
            output.contains("# HELP") || output.contains("# TYPE") || output.is_empty(),
            "unexpected prometheus output: {output}"
        );
    }

    #[test]
    fn metrics_render_contains_request_counter() {
        let m = Metrics::new().unwrap();
        // CounterVec only appears in output after at least one observation.
        m.http_requests_total
            .with_label_values(&["GET", "/health", "200"])
            .inc();
        let output = m.render();
        assert!(
            output.contains("gyre_http_requests_total"),
            "missing gyre_http_requests_total in: {output}"
        );
    }

    #[test]
    fn metrics_render_contains_duration_histogram() {
        let m = Metrics::new().unwrap();
        // HistogramVec only appears in output after at least one observation.
        m.http_request_duration_seconds
            .with_label_values(&["GET", "/health"])
            .observe(0.001);
        let output = m.render();
        assert!(
            output.contains("gyre_http_request_duration_seconds"),
            "missing gyre_http_request_duration_seconds in: {output}"
        );
    }

    #[test]
    fn metrics_render_contains_active_agents() {
        let m = Metrics::new().unwrap();
        let output = m.render();
        assert!(
            output.contains("gyre_active_agents"),
            "missing gyre_active_agents in: {output}"
        );
    }

    #[test]
    fn metrics_render_contains_merge_queue_depth() {
        let m = Metrics::new().unwrap();
        let output = m.render();
        assert!(
            output.contains("gyre_merge_queue_depth"),
            "missing gyre_merge_queue_depth in: {output}"
        );
    }

    #[test]
    fn metrics_request_counter_increments() {
        let m = Metrics::new().unwrap();
        m.http_requests_total
            .with_label_values(&["GET", "/health", "200"])
            .inc();
        let output = m.render();
        assert!(
            output.contains("gyre_http_requests_total{"),
            "counter not incremented in: {output}"
        );
    }
}
