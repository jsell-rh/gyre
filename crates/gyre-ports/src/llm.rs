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
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::Arc;

/// A tool definition passed to the LLM for tool-calling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// A single message in a multi-turn conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: String, // "user", "assistant", "tool_result"
    pub content: ConversationContent,
}

/// Content of a conversation message — either text or structured blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConversationContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

/// A content block within a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

/// The result of a tool-calling completion.
#[derive(Debug, Clone)]
pub struct ToolCallingResponse {
    /// Text content from the assistant.
    pub text: String,
    /// Tool calls requested by the assistant (empty if none).
    pub tool_calls: Vec<ToolCall>,
    /// Whether the model wants to stop ("end_turn") or continue ("tool_use").
    pub stop_reason: String,
}

/// A single tool call from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

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
    async fn stream_complete(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: Option<u32>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>>;

    /// Multi-turn completion with tool calling.
    ///
    /// Sends a conversation with tool definitions. The LLM may respond with
    /// text, tool calls, or both. The caller executes tool calls and feeds
    /// results back via subsequent calls.
    async fn complete_with_tools(
        &self,
        system_prompt: &str,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        max_tokens: Option<u32>,
    ) -> Result<ToolCallingResponse>;
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
