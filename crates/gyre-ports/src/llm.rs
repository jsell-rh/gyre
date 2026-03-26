//! LLM inference port — abstracted behind traits so adapters can swap
//! between providers (Vertex AI, mock for tests, etc.) without changing
//! domain or server logic.
//!
//! Lives in gyre-ports. `gyre-domain` MUST NOT depend on this directly.
//! All concrete implementations live in `gyre-adapters`.
//! `gyre-server` wires the implementation into `AppState`.

use anyhow::Result;
use async_trait::async_trait;
use futures_util::Stream;
use std::pin::Pin;
use std::sync::Arc;

/// Port for LLM inference.
///
/// Rules:
/// - Lives in gyre-ports. gyre-domain MUST NOT depend on this.
/// - All concrete implementations live in gyre-adapters.
/// - gyre-server wires the implementation into AppState.
#[async_trait]
pub trait LlmPort: Send + Sync {
    /// Generate a single non-streaming completion.
    ///
    /// Used where a full response is needed before proceeding
    /// (e.g., structured JSON prediction, internal pipeline steps).
    async fn complete(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: Option<u32>,
    ) -> Result<String>;

    /// Structured prediction: returns the LLM response parsed as JSON.
    ///
    /// Implementations append "Respond with valid JSON only, no markdown
    /// code fences." to the system prompt.
    async fn predict_json(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<serde_json::Value>;

    /// Token-level streaming completion.
    ///
    /// Returns a stream of token chunks (each item is a partial string
    /// emitted as the model generates). The caller collects these and
    /// forwards them as SSE events to the frontend.
    ///
    /// Each SSE endpoint calls this method, emitting one `partial` event
    /// per received chunk and a final `complete` event when the stream ends.
    async fn stream_complete(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: Option<u32>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>>;
}

/// Factory for creating per-call `LlmPort` handles with a specified model.
///
/// `AppState.llm` holds `Option<Arc<dyn LlmPortFactory>>`. Each endpoint
/// resolves the model name, calls `state.llm.for_model(&model)`, then calls
/// `complete()` / `stream_complete()` on the returned handle.
pub trait LlmPortFactory: Send + Sync {
    /// Create a per-call `LlmPort` handle using the specified model name.
    fn for_model(&self, model_name: &str) -> Arc<dyn LlmPort>;
}
