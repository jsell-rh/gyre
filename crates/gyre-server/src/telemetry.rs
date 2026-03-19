//! OpenTelemetry tracing + structured logging initialization.
//!
//! - Exports spans via OTLP/gRPC to the collector at `OTEL_EXPORTER_OTLP_ENDPOINT`
//!   (default: `http://localhost:4317`). The server works fine without a collector.
//! - JSON logging: set `GYRE_LOG_FORMAT=json` (default: human-readable).
//! - Log level: controlled by `RUST_LOG` (default: `info`).

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::Tracer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

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
    // Option<Layer<S>> type-parameter mismatches.
    match (tracer, json_logging) {
        (Some(t), true) => {
            let _ = tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_opentelemetry::layer().with_tracer(t))
                .with(tracing_subscriber::fmt::layer().json())
                .try_init();
        }
        (Some(t), false) => {
            let _ = tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_opentelemetry::layer().with_tracer(t))
                .with(tracing_subscriber::fmt::layer())
                .try_init();
        }
        (None, true) => {
            let _ = tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_subscriber::fmt::layer().json())
                .try_init();
        }
        (None, false) => {
            let _ = tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_subscriber::fmt::layer())
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
}
