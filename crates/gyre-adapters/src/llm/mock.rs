//! Mock LLM adapter for tests — no external calls.
//!
//! `MockLlmAdapter` returns fixed or echo responses. `MockLlmPortFactory`
//! wraps it so `AppState.llm` can be wired in `mem::test_state()`.

use anyhow::Result;
use async_trait::async_trait;
use futures_util::Stream;
use gyre_ports::{
    ConversationContent, ConversationMessage, LlmPort, LlmPortFactory, ToolCallingResponse,
    ToolDefinition,
};
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;

/// In-memory LLM adapter that returns a fixed string or echoes the prompt.
///
/// Used in tests and when Vertex AI credentials are unavailable.
pub struct MockLlmAdapter {
    pub response: String,
    /// When set, every LLM call fails with this error — simulates an LLM
    /// connection failure so SSE `event: error` handling is testable
    /// (ui-layout.md §2: "If the LLM connection fails entirely, `event:
    /// error` fires instead").
    pub failure: Option<String>,
    /// When set, `stream_complete` succeeds but the stream errors mid-way
    /// after emitting the partial chunks — simulates a dropped LLM
    /// connection mid-generation so truncated-response handling is
    /// testable.
    pub stream_error_after: Option<usize>,
    /// Chunks emitted by `stream_complete` before the mid-stream error.
    pub partial_chunks: Vec<String>,
    /// The error emitted after the partial chunks in `stream_error_after`
    /// mode.
    pub stream_error_message: String,
    /// Scripted response sequence for `complete_with_tools`: each call
    /// returns the next entry (the last entry repeats once exhausted).
    /// When set, it takes precedence over `response` for tool-calling
    /// completions — used to test multi-turn agent loops (e.g. the explorer
    /// self-check refinement flow) where consecutive calls must return
    /// different outputs.
    script: Option<std::sync::Mutex<VecDeque<String>>>,
    /// Every system prompt received by `complete` / `predict_json` /
    /// `stream_complete`, in call order. Lets tests assert on the prompt
    /// the server actually built (e.g. that a `{{...}}` template variable
    /// was substituted before reaching the LLM) without mirroring the
    /// handler's substitution logic.
    pub captured_system_prompts: Arc<std::sync::Mutex<Vec<String>>>,
}

impl MockLlmAdapter {
    /// Create an adapter that always returns the given fixed string.
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
            failure: None,
            stream_error_after: None,
            partial_chunks: vec![],
            stream_error_message: String::new(),
            script: None,
            captured_system_prompts: Arc::new(std::sync::Mutex::new(vec![])),
        }
    }
    /// Create an adapter that echoes the user_prompt verbatim.
    ///
    /// Useful for asserting that prompt construction logic is correct.
    pub fn echo() -> Self {
        Self::new("__echo__")
    }

    /// Create an adapter that always returns a serialized JSON value.
    pub fn json_response(v: serde_json::Value) -> Self {
        Self::new(serde_json::to_string(&v).unwrap())
    }

    /// Create an adapter whose every LLM call fails — for testing SSE
    /// `event: error` behavior.
    pub fn failing(message: impl Into<String>) -> Self {
        Self {
            response: String::new(),
            failure: Some(message.into()),
            stream_error_after: None,
            partial_chunks: vec![],
            stream_error_message: String::new(),
            script: None,
            captured_system_prompts: Arc::new(std::sync::Mutex::new(vec![])),
        }
    }

    /// Create an adapter that streams `chunks` and then errors — for
    /// testing mid-stream failure handling (a truncated response must not
    /// masquerade as a `complete` event).
    pub fn stream_error_after(chunks: Vec<String>, message: impl Into<String>) -> Self {
        Self {
            response: String::new(),
            failure: None,
            stream_error_after: Some(chunks.len()),
            partial_chunks: chunks,
            stream_error_message: message.into(),
            script: None,
            captured_system_prompts: Arc::new(std::sync::Mutex::new(vec![])),
        }
    }

    /// Create an adapter whose `complete_with_tools` calls return the given
    /// responses in order (last repeats once exhausted). Other methods
    /// (`complete`, `stream_complete`, `predict_json`) keep the fixed
    /// `response` behavior.
    pub fn scripted_tool_responses(responses: Vec<String>) -> Self {
        Self {
            response: String::new(),
            failure: None,
            stream_error_after: None,
            partial_chunks: vec![],
            stream_error_message: String::new(),
            script: Some(std::sync::Mutex::new(VecDeque::from(responses))),
            captured_system_prompts: Arc::new(std::sync::Mutex::new(vec![])),
        }
    }

    /// Next scripted tool-calling response, or `None` when not scripted.
    fn next_scripted(&self) -> Option<String> {
        let mut script = self.script.as_ref()?.lock().ok()?;
        if script.len() > 1 {
            script.pop_front()
        } else {
            // Keep the last entry so the adapter never runs dry — callers
            // may loop more times than the script anticipates.
            script.front().cloned()
        }
    }

    fn resolve_text(&self, user_prompt: &str) -> String {
        if self.response == "__echo__" {
            user_prompt.to_string()
        } else {
            self.response.clone()
        }
    }

    /// Record a received system prompt for test inspection.
    fn capture_system_prompt(&self, system_prompt: &str) {
        if let Ok(mut captured) = self.captured_system_prompts.lock() {
            captured.push(system_prompt.to_string());
        }
    }
}

#[async_trait]
impl LlmPort for MockLlmAdapter {
    async fn complete(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        _max_tokens: Option<u32>,
    ) -> Result<String> {
        self.capture_system_prompt(system_prompt);
        if let Some(msg) = &self.failure {
            return Err(anyhow::anyhow!("{msg}"));
        }
        Ok(self.resolve_text(user_prompt))
    }

    async fn predict_json(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        _max_tokens: Option<u32>,
    ) -> Result<serde_json::Value> {
        self.capture_system_prompt(system_prompt);
        if let Some(msg) = &self.failure {
            return Err(anyhow::anyhow!("{msg}"));
        }
        if self.response == "__echo__" {
            Ok(serde_json::json!([{
                "type": "test",
                "description": user_prompt,
                "severity": "low",
                "affected_nodes": []
            }]))
        } else {
            serde_json::from_str(&self.response)
                .map_err(|e| anyhow::anyhow!("MockLlmAdapter: invalid JSON in response: {}", e))
        }
    }

    async fn stream_complete(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        _max_tokens: Option<u32>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
        self.capture_system_prompt(system_prompt);
        if let Some(msg) = &self.failure {
            return Err(anyhow::anyhow!("{msg}"));
        }
        if let Some(n) = self.stream_error_after {
            // Emit the partial chunks, then error — a mid-stream failure
            // the handler must surface as `event: error`, not `complete`.
            let msg = self.stream_error_message.clone();
            let chunks: Vec<Result<String>> = self
                .partial_chunks
                .iter()
                .take(n)
                .map(|c| Ok(c.clone()))
                .chain(std::iter::once(Err(anyhow::anyhow!("{msg}"))))
                .collect();
            return Ok(Box::pin(futures_util::stream::iter(chunks)));
        }
        let text = self.resolve_text(user_prompt);
        // Emit in (up to) 3 chunks to simulate streaming behaviour.
        let chars: Vec<char> = text.chars().collect();
        let chunk_size = chars.len().max(1).div_ceil(3);
        let chunks: Vec<Result<String>> = chars
            .chunks(chunk_size)
            .map(|c| Ok(c.iter().collect::<String>()))
            .collect();
        Ok(Box::pin(futures_util::stream::iter(chunks)))
    }

    async fn complete_with_tools(
        &self,
        _system_prompt: &str,
        messages: &[ConversationMessage],
        _tools: &[ToolDefinition],
        _max_tokens: Option<u32>,
    ) -> Result<ToolCallingResponse> {
        if let Some(text) = self.next_scripted() {
            return Ok(ToolCallingResponse {
                text,
                tool_calls: vec![],
                stop_reason: "end_turn".to_string(),
            });
        }
        let user_text = messages
            .iter()
            .filter(|m| m.role == "user")
            .filter_map(|m| match &m.content {
                ConversationContent::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .last()
            .unwrap_or("");
        let text = self.resolve_text(user_text);
        Ok(ToolCallingResponse {
            text,
            tool_calls: vec![],
            stop_reason: "end_turn".to_string(),
        })
    }
}

/// Factory that wraps a shared `MockLlmAdapter` and ignores the model name.
///
/// All calls to `for_model()` return the same underlying adapter.
pub struct MockLlmPortFactory {
    pub inner: Arc<MockLlmAdapter>,
}

impl MockLlmPortFactory {
    /// Create a factory whose adapter echoes the user_prompt.
    pub fn echo() -> Self {
        Self {
            inner: Arc::new(MockLlmAdapter::echo()),
        }
    }

    /// Create a factory whose adapter returns the given responses from
    /// `complete_with_tools` in order (last repeats once exhausted).
    /// See `MockLlmAdapter::scripted_tool_responses`.
    pub fn scripted_tool_responses(responses: Vec<String>) -> Self {
        Self {
            inner: Arc::new(MockLlmAdapter::scripted_tool_responses(responses)),
        }
    }
}

impl LlmPortFactory for MockLlmPortFactory {
    fn for_model(&self, _model_name: &str) -> Arc<dyn LlmPort> {
        self.inner.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    #[tokio::test]
    async fn mock_complete_returns_fixed_string() {
        let adapter = MockLlmAdapter::new("hello world");
        let result = adapter.complete("sys", "user input", None).await.unwrap();
        assert_eq!(result, "hello world");
    }

    #[tokio::test]
    async fn mock_complete_echo_returns_prompt() {
        let adapter = MockLlmAdapter::echo();
        let result = adapter
            .complete("system", "my prompt", Some(100))
            .await
            .unwrap();
        assert_eq!(result, "my prompt");
    }

    #[tokio::test]
    async fn mock_predict_json_returns_valid_json() {
        let expected = serde_json::json!({"key": "value"});
        let adapter = MockLlmAdapter::json_response(expected.clone());
        let result = adapter.predict_json("sys", "user", None).await.unwrap();
        assert_eq!(result, expected);
    }

    #[tokio::test]
    async fn mock_predict_json_echo_returns_structured_response() {
        let adapter = MockLlmAdapter::echo();
        let result = adapter
            .predict_json("sys", "my question", None)
            .await
            .unwrap();
        assert!(result.is_array());
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["description"], "my question");
    }

    #[tokio::test]
    async fn mock_stream_complete_emits_chunks_and_terminates() {
        let adapter = MockLlmAdapter::new("hello world");
        let stream = adapter.stream_complete("sys", "user", None).await.unwrap();
        let chunks: Vec<String> = stream.map(|r| r.unwrap()).collect::<Vec<_>>().await;
        // Chunks must reassemble to the original response.
        assert_eq!(chunks.join(""), "hello world");
        // Stream must terminate (we collected it).
        assert!(!chunks.is_empty());
    }

    #[tokio::test]
    async fn mock_failing_adapter_fails_every_call() {
        let adapter = MockLlmAdapter::failing("connection refused");
        assert!(adapter.complete("s", "u", None).await.is_err());
        assert!(adapter.predict_json("s", "u", None).await.is_err());
        assert!(adapter.stream_complete("s", "u", None).await.is_err());
    }

    #[tokio::test]
    async fn mock_stream_error_after_emits_partial_then_fails() {
        let adapter =
            MockLlmAdapter::stream_error_after(vec!["par".into()], "dropped mid-generation");
        let mut stream = adapter.stream_complete("s", "u", None).await.unwrap();
        assert_eq!(stream.next().await.unwrap().unwrap(), "par");
        assert!(stream.next().await.unwrap().is_err());
    }

    #[test]
    fn mock_factory_for_model_ignores_model_name() {
        let factory = MockLlmPortFactory::echo();
        // Different model names return the same adapter type.
        let _port1 = factory.for_model("gemini-2.0-flash-001");
        let _port2 = factory.for_model("gemini-1.5-pro-002");
        // No panic = success.
    }

    #[tokio::test]
    async fn mock_scripted_tool_responses_return_in_order() {
        // Each complete_with_tools call returns the next scripted response.
        let factory =
            MockLlmPortFactory::scripted_tool_responses(vec!["first".into(), "second".into()]);
        let port = factory.for_model("any");
        let r1 = port
            .complete_with_tools("sys", &[], &[], None)
            .await
            .unwrap();
        let r2 = port
            .complete_with_tools("sys", &[], &[], None)
            .await
            .unwrap();
        assert_eq!(r1.text, "first");
        assert_eq!(r2.text, "second");
        assert_eq!(r2.stop_reason, "end_turn");
    }

    #[tokio::test]
    async fn mock_scripted_tool_responses_repeat_last_when_exhausted() {
        // The adapter never runs dry — the last entry repeats.
        let factory = MockLlmPortFactory::scripted_tool_responses(vec!["only".to_string()]);
        let port = factory.for_model("any");
        for _ in 0..3 {
            let r = port
                .complete_with_tools("sys", &[], &[], None)
                .await
                .unwrap();
            assert_eq!(r.text, "only");
        }
    }

    #[tokio::test]
    async fn mock_scripted_tool_responses_do_not_affect_plain_complete() {
        // Only complete_with_tools is scripted; complete() keeps the fixed
        // response behavior.
        let mut adapter = MockLlmAdapter::scripted_tool_responses(vec!["scripted".into()]);
        adapter.response = "fixed".to_string();
        let r = adapter.complete("sys", "user", None).await.unwrap();
        assert_eq!(r, "fixed");
    }
}
