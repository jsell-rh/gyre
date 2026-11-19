//! OpenTelemetry tracing + structured logging initialization.
//!
//! - Exports spans via OTLP/gRPC to the collector at `OTEL_EXPORTER_OTLP_ENDPOINT`
//!   (default: `http://localhost:4317`). The server works fine without a collector.
//! - JSON logging: set `GYRE_LOG_FORMAT=json` (default: human-readable).
//! - Log level: controlled by `RUST_LOG` (default: `info`).

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::Tracer;
use std::collections::VecDeque;
use std::sync::OnceLock;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

// ── Server log capture (admin-panel.md §Server Logs; TASK-273) ──────────

/// One captured server log event.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ServerLogEntry {
    /// Unix epoch milliseconds of the event.
    pub timestamp_ms: u64,
    /// `TRACE` | `DEBUG` | `INFO` | `WARN` | `ERROR`.
    pub level: String,
    /// Module path that emitted the event.
    pub target: String,
    /// The formatted `message` field (empty for events without one).
    pub message: String,
}

/// Bounded ring buffer of recent server log events, served to the admin
/// Server Logs tab via `GET /api/v1/admin/server/logs`.
///
/// A process-wide singleton: the tracing subscriber is initialized before
/// `AppState` exists, so the buffer outlives (and is shared with) it.
pub struct ServerLogBuffer {
    entries: std::sync::RwLock<VecDeque<ServerLogEntry>>,
    capacity: usize,
}

impl ServerLogBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            entries: std::sync::RwLock::new(VecDeque::with_capacity(
                capacity.min(1024),
            )),
            capacity,
        }
    }

    fn push(&self, entry: ServerLogEntry) {
        let mut entries = self.entries.write().unwrap();
        if entries.len() == self.capacity {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    /// Entries matching the filters, in chronological order: level >=
    /// `min_level` (when given), emitted at/after `since_ms`, at most
    /// `limit` (the MOST RECENT `limit` matching entries).
    pub fn query(
        &self,
        min_level: Option<&str>,
        since_ms: Option<u64>,
        limit: usize,
    ) -> Vec<ServerLogEntry> {
        let entries = self.entries.read().unwrap();
        let mut matching: Vec<ServerLogEntry> = entries
            .iter()
            .filter(|e| {
                min_level
                    .map(|l| level_rank(&e.level) >= level_rank(l))
                    .unwrap_or(true)
                    && since_ms.map(|s| e.timestamp_ms >= s).unwrap_or(true)
            })
            .cloned()
            .collect();
        if matching.len() > limit {
            let skip = matching.len() - limit;
            matching.drain(..skip);
        }
        matching
    }

    /// Number of buffered entries (for tests).
    pub fn len(&self) -> usize {
        self.entries.read().unwrap().len()
    }

    /// Whether the buffer is empty (for tests).
    pub fn is_empty(&self) -> bool {
        self.entries.read().unwrap().is_empty()
    }
}

fn level_rank(level: &str) -> u8 {
    match level {
        "ERROR" => 5,
        "WARN" => 4,
        "INFO" => 3,
        "DEBUG" => 2,
        "TRACE" => 1,
        _ => 0,
    }
}

/// The process-wide server-log buffer.
pub fn server_log_buffer() -> &'static ServerLogBuffer {
    static BUFFER: OnceLock<ServerLogBuffer> = OnceLock::new();
    BUFFER.get_or_init(|| {
        let capacity = std::env::var("GYRE_SERVER_LOG_BUFFER_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5000);
        ServerLogBuffer::new(capacity)
    })
}

/// `tracing_subscriber` layer capturing formatted events into the
/// [`ServerLogBuffer`]. Installed ALONGSIDE the existing stdout/JSON and
/// OTel layers — it changes nothing about them.
struct ServerLogLayer;

impl<S> tracing_subscriber::Layer<S> for ServerLogLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        use tracing::field::Visit;

        struct MessageVisitor {
            message: String,
        }

        impl Visit for MessageVisitor {
            fn record_debug(
                &mut self,
                field: &tracing::field::Field,
                value: &dyn std::fmt::Debug,
            ) {
                if field.name() == "message" {
                    self.message = format!("{value:?}");
                }
            }

            fn record_str(
                &mut self,
                field: &tracing::field::Field,
                value: &str,
            ) {
                if field.name() == "message" {
                    self.message = value.to_string();
                }
            }
        }

        let mut visitor = MessageVisitor {
            message: String::new(),
        };
        event.record(&mut visitor);

        server_log_buffer().push(ServerLogEntry {
            timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            level: event.metadata().level().to_string(),
            target: event.metadata().target().to_string(),
            message: visitor.message,
        });
    }
}

/// Dropped when the server shuts down, flushing any buffered spans.
pub struct TelemetryGuard;

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        opentelemetry::global::shutdown_tracer_provider();
    }
}

/// Initialize OTel tracing + tracing-subscriber.
///
/// Returns a guard; drop it (or let it go out of scope) to flush spans on shutdown.
/// Safe to call even when no OTel collector is running.
pub fn init_telemetry() -> TelemetryGuard {
    init_impl()
}

pub(crate) fn init_impl() -> TelemetryGuard {
    init_with_otel(true)
}

fn init_with_otel(enable_otel: bool) -> TelemetryGuard {
    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:4317".to_string());

    let json_logging = std::env::var("GYRE_LOG_FORMAT")
        .map(|v| v == "json")
        .unwrap_or(false);

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // Attempt to build the OTel tracer (skipped in unit tests to avoid network timeouts).
    let tracer: Option<Tracer> = if enable_otel {
        build_tracer(&endpoint)
    } else {
        None
    };

    // Build the tracing-subscriber stack.  Four concrete branches avoid
    // Option<Layer<S>> type-parameter mismatches. Every branch installs the
    // server-log capture layer ALONGSIDE the existing stdout/JSON layers
    // (admin-panel.md §Server Logs; TASK-273) without changing them.
    match (tracer, json_logging) {
        (Some(t), true) => {
            let _ = tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_opentelemetry::layer().with_tracer(t))
                .with(tracing_subscriber::fmt::layer().json())
                .with(ServerLogLayer)
                .try_init();
        }
        (Some(t), false) => {
            let _ = tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_opentelemetry::layer().with_tracer(t))
                .with(tracing_subscriber::fmt::layer())
                .with(ServerLogLayer)
                .try_init();
        }
        (None, true) => {
            let _ = tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_subscriber::fmt::layer().json())
                .with(ServerLogLayer)
                .try_init();
        }
        (None, false) => {
            let _ = tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_subscriber::fmt::layer())
                .with(ServerLogLayer)
                .try_init();
        }
    }

    TelemetryGuard
}

fn build_tracer(endpoint: &str) -> Option<Tracer> {
    // Build OTLP span exporter via gRPC/tonic.
    let exporter = match opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()
    {
        Ok(e) => e,
        Err(e) => {
            eprintln!("OTel OTLP exporter build failed (no collector?): {e}");
            return None;
        }
    };

    // Create a TracerProvider with a batch processor (async, Tokio runtime).
    let provider = opentelemetry_sdk::trace::TracerProvider::builder()
        .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
        .build();

    // Set as the global provider so `opentelemetry::global::tracer()` works.
    let tracer = provider.tracer("gyre-server");
    opentelemetry::global::set_tracer_provider(provider);

    Some(tracer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_init_no_panic() {
        // Skip OTel to avoid network connection timeouts in tests.
        // The OTel path is tested separately when a collector is present.
        let _guard = init_with_otel(false);
    }

    #[test]
    fn telemetry_guard_drops_cleanly() {
        {
            let _guard = init_with_otel(false);
        }
        // Getting here means Drop impl didn't panic.
    }

    #[test]
    fn server_log_layer_captures_formatted_events() {
        // A dedicated subscriber with ONLY the capture layer (with_default,
        // not global init) so this test does not depend on — or disturb —
        // the process-global subscriber other tests install.
        let subscriber = tracing_subscriber::registry().with(ServerLogLayer);
        let marker = format!("capture-test-{}", std::process::id());
        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(target: "gyre_capture_test", "{marker} warn line");
            tracing::info!(target: "gyre_capture_test", answer = 42, "{marker} info line");
        });

        let buffer = server_log_buffer();
        let captured: Vec<ServerLogEntry> = buffer
            .query(None, None, 1000)
            .into_iter()
            .filter(|e| e.target == "gyre_capture_test" && e.message.contains(&marker))
            .collect();
        assert_eq!(captured.len(), 2, "both events captured");
        assert_eq!(captured[0].level, "WARN");
        assert!(captured[0].message.contains("warn line"));
        assert_eq!(captured[1].level, "INFO");
        assert!(captured[1].message.contains("info line"));
        assert!(captured[0].timestamp_ms > 0);

        // Level filter: WARN-and-above excludes the INFO line.
        let warn_only: Vec<_> = buffer
            .query(Some("WARN"), None, 1000)
            .into_iter()
            .filter(|e| e.target == "gyre_capture_test" && e.message.contains(&marker))
            .collect();
        assert_eq!(warn_only.len(), 1);
        assert_eq!(warn_only[0].level, "WARN");
    }

    #[test]
    fn server_log_buffer_is_bounded() {
        let buffer = ServerLogBuffer::new(3);
        for i in 0..5 {
            buffer.push(ServerLogEntry {
                timestamp_ms: i,
                level: "INFO".to_string(),
                target: "t".to_string(),
                message: format!("m{i}"),
            });
        }
        assert_eq!(buffer.len(), 3);
        let all = buffer.query(None, None, 10);
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].message, "m2", "oldest entries evicted first");
        assert_eq!(all[2].message, "m4");

        // `since` filter.
        let since = buffer.query(None, Some(3), 10);
        assert_eq!(since.len(), 2);

        // `limit` keeps the MOST RECENT entries.
        let limited = buffer.query(None, None, 1);
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].message, "m4");
    }
}
