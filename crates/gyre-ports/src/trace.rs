//! Port trait for gate-time trace capture persistence (HSI §3a).

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::{GateTrace, Id};

/// Full input/output payload for a single span (stored separately from the trace).
///
/// Payloads are zstd-compressed and capped at 1MB per trace. This keeps the
/// main `TraceSpan` struct lightweight while allowing drill-down into full payloads.
pub struct SpanPayload {
    /// Full request body (decompressed). None if no input was captured.
    pub input: Option<Vec<u8>>,
    /// Full response body (decompressed). None if no output was captured.
    pub output: Option<Vec<u8>>,
}

/// Repository for gate-time OTel trace capture (HSI §3a).
///
/// Traces are stored per-MR, capped at the most recent gate run.
/// The store() method replaces any existing trace for the same MR.
#[async_trait]
pub trait TraceRepository: Send + Sync {
    /// Store a gate trace (replaces any existing trace for the same MR).
    async fn store(&self, trace: &GateTrace) -> Result<()>;

    /// Get the most recent trace for an MR.
    async fn get_by_mr(&self, mr_id: &Id) -> Result<Option<GateTrace>>;

    /// Get a specific span's full payload (input/output bodies).
    ///
    /// Returns None if the span has no stored payload blob.
    async fn get_span_payload(
        &self,
        gate_run_id: &Id,
        span_id: &str,
    ) -> Result<Option<SpanPayload>>;

    /// Promote a trace to permanent storage (called on MR merge for attestation).
    ///
    /// The trace is preserved even after the MR is merged — it becomes part of
    /// the merge attestation record for provenance.
    async fn promote_to_attestation(&self, mr_id: &Id) -> Result<()>;

    /// Delete traces for an MR (called on MR close without merge).
    async fn delete_by_mr(&self, mr_id: &Id) -> Result<()>;
}
