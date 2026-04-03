//! Vertex AI LLM adapter — direct REST API implementation.
//!
//! Uses the Vertex AI REST API via `reqwest` and Google Application Default
//! Credentials (ADC) via `google-cloud-auth`.
//!
//! Claude models use the `rawPredict` / `streamRawPredict` endpoints with the
//! Anthropic Messages API format. Gemini models use `generateContent` with
//! Google's format.
//!
//! Note: rig-core ≥ 0.33.0 requires Rust 1.88 (let-chain syntax). Our MSRV
//! is 1.87, so we implement the adapter without rig and call the REST API
//! directly. The `LlmPort` / `LlmPortFactory` abstractions are unchanged.
//!
//! # Environment variables
//! - `GYRE_VERTEX_PROJECT` (required): GCP project ID.
//! - `GYRE_VERTEX_LOCATION` (optional, default: "us-central1"): region.
//! - Auth: uses Google Application Default Credentials (ADC).
//!   Set `GOOGLE_APPLICATION_CREDENTIALS` to a service account key file or an
//!   ADC user credentials file (produced by `gcloud auth application-default login`).
//!   If the env var is unset, falls back to
//!   `~/.config/gcloud/application_default_credentials.json`.
//!   Also works on GCP with Workload Identity / metadata server.

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::Stream;
use gyre_ports::{
    ContentBlock, ConversationContent, ConversationMessage, LlmPort, LlmPortFactory, ToolCall,
    ToolCallingResponse, ToolDefinition,
};
use serde::Deserialize;
use serde_json::Value;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Parsed Google service account key file (subset of fields we need).
#[derive(Debug, Deserialize)]
struct ServiceAccountKey {
    client_email: String,
    private_key: String,
}

/// Parsed Google ADC user credentials (from `gcloud auth application-default login`).
#[derive(Debug, Deserialize)]
struct UserCredentials {
    client_id: String,
    client_secret: String,
    refresh_token: String,
}

// ── Token cache ──────────────────────────────────────────────────────────────

/// Cached ADC access token with expiry tracking.
#[derive(Default)]
struct TokenCache {
    token: Option<String>,
    /// Expiry as Unix epoch seconds. We refresh 60 s before actual expiry.
    expires_at: u64,
}

impl TokenCache {
    fn is_valid(&self) -> bool {
        if let Some(ref _t) = self.token {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            now < self.expires_at.saturating_sub(60)
        } else {
            false
        }
    }
}

// ── Factory ──────────────────────────────────────────────────────────────────

/// Factory for creating per-model Vertex AI adapters.
///
/// Reads `GYRE_VERTEX_PROJECT` and `GYRE_VERTEX_LOCATION` at construction time.
/// Each call to `for_model()` creates a `RigVertexAiAdapter` with the given
/// model name. Auth tokens are cached and refreshed automatically.
pub struct RigVertexAiFactory {
    project: String,
    location: String,
    client: reqwest::Client,
    /// Shared token cache across all adapters spawned by this factory.
    token_cache: Arc<RwLock<TokenCache>>,
}

impl RigVertexAiFactory {
    /// Construct from environment variables.
    ///
    /// Returns `Err` if `GYRE_VERTEX_PROJECT` is not set.
    pub fn from_env() -> Result<Self> {
        let project = std::env::var("GYRE_VERTEX_PROJECT")
            .context("GYRE_VERTEX_PROJECT is not set — cannot initialise Vertex AI adapter")?;
        let location =
            std::env::var("GYRE_VERTEX_LOCATION").unwrap_or_else(|_| "us-central1".to_string());
        Ok(Self {
            project,
            location,
            client: reqwest::Client::new(),
            token_cache: Arc::new(RwLock::new(TokenCache::default())),
        })
    }
}

impl LlmPortFactory for RigVertexAiFactory {
    fn for_model(&self, model_name: &str) -> Arc<dyn LlmPort> {
        Arc::new(RigVertexAiAdapter {
            project: self.project.clone(),
            location: self.location.clone(),
            model: model_name.to_string(),
            client: self.client.clone(),
            token_cache: Arc::clone(&self.token_cache),
        })
    }
}

// ── Adapter ──────────────────────────────────────────────────────────────────

/// Per-call Vertex AI completion adapter.
///
/// Created by `RigVertexAiFactory::for_model`. Shares an auth token cache
/// with the factory and any sibling adapters.
pub struct RigVertexAiAdapter {
    project: String,
    location: String,
    model: String,
    client: reqwest::Client,
    token_cache: Arc<RwLock<TokenCache>>,
}

impl RigVertexAiAdapter {
    /// Returns true if this adapter targets a Claude model.
    fn is_claude(&self) -> bool {
        self.model.starts_with("claude")
    }

    /// Return the completion endpoint URL for this adapter's model.
    ///
    /// Claude models use `rawPredict` (Anthropic Messages API).
    /// Gemini models use `generateContent` (Google format).
    fn endpoint_url(&self) -> String {
        let (publisher, method) = if self.is_claude() {
            ("anthropic", "rawPredict")
        } else {
            ("google", "generateContent")
        };
        format!(
            "https://{location}-aiplatform.googleapis.com/v1/\
             projects/{project}/locations/{location}/\
             publishers/{publisher}/models/{model}:{method}",
            location = self.location,
            project = self.project,
            publisher = publisher,
            model = self.model,
        )
    }

    /// Return the streaming endpoint URL for this adapter's model.
    ///
    /// Claude models use `streamRawPredict` (Anthropic SSE streaming).
    /// Gemini models use `streamGenerateContent` (Google SSE streaming).
    fn streaming_endpoint_url(&self) -> String {
        let (publisher, method) = if self.is_claude() {
            ("anthropic", "streamRawPredict")
        } else {
            ("google", "streamGenerateContent")
        };
        format!(
            "https://{location}-aiplatform.googleapis.com/v1/\
             projects/{project}/locations/{location}/\
             publishers/{publisher}/models/{model}:{method}",
            location = self.location,
            project = self.project,
            publisher = publisher,
            model = self.model,
        )
    }

    /// Fetch a valid ADC bearer token, refreshing from cache if necessary.
    async fn bearer_token(&self) -> Result<String> {
        // Fast path: return cached token if still valid.
        {
            let cache = self.token_cache.read().await;
            if cache.is_valid() {
                return Ok(cache.token.clone().unwrap());
            }
        }

        // Slow path: fetch a new token.
        let (token_value, expires_at) = fetch_adc_token().await?;

        let mut cache = self.token_cache.write().await;
        cache.token = Some(token_value.clone());
        cache.expires_at = expires_at;
        Ok(token_value)
    }

    /// POST `body` to `url` and return the parsed JSON response.
    async fn post_json(&self, url: &str, body: Value) -> Result<Value> {
        let token = self.bearer_token().await?;

        let resp = self
            .client
            .post(url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .context("Vertex AI HTTP request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Vertex AI returned {status}: {text}");
        }

        let json: Value = resp
            .json()
            .await
            .context("Failed to parse Vertex AI response")?;
        tracing::debug!(
            "Vertex AI raw response: {}",
            serde_json::to_string(&json).unwrap_or_default()
        );
        Ok(json)
    }

    /// Build the Anthropic Messages API request body for Claude on Vertex AI.
    ///
    /// For `rawPredict` (non-streaming), the `stream` field is omitted — the
    /// endpoint is always synchronous. For `streamRawPredict`, `stream: true`
    /// is included to signal Anthropic SSE mode.
    fn claude_request_body(
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: Option<u32>,
        stream: bool,
    ) -> Value {
        let mut body = serde_json::json!({
            "anthropic_version": "vertex-2023-10-16",
            "messages": [
                {"role": "user", "content": user_prompt}
            ],
            "max_tokens": max_tokens.unwrap_or(4096),
        });
        // Only include `stream` for streaming calls (streamRawPredict).
        // rawPredict is always synchronous and does not require this field.
        if stream {
            body["stream"] = true.into();
        }
        if !system_prompt.is_empty() {
            body["system"] = system_prompt.into();
        }
        body
    }

    /// Build the Anthropic Messages API request body with tool definitions
    /// and a multi-turn conversation history.
    fn claude_request_body_with_tools(
        system_prompt: &str,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        max_tokens: Option<u32>,
    ) -> Value {
        // Convert messages to Anthropic format
        let api_messages: Vec<Value> = messages
            .iter()
            .map(|msg| {
                let content = match &msg.content {
                    ConversationContent::Text(text) => Value::String(text.clone()),
                    ConversationContent::Blocks(blocks) => {
                        let api_blocks: Vec<Value> = blocks
                            .iter()
                            .map(|block| match block {
                                ContentBlock::Text { text } => {
                                    serde_json::json!({"type": "text", "text": text})
                                }
                                ContentBlock::ToolUse { id, name, input } => {
                                    serde_json::json!({"type": "tool_use", "id": id, "name": name, "input": input})
                                }
                                ContentBlock::ToolResult {
                                    tool_use_id,
                                    content,
                                } => {
                                    serde_json::json!({"type": "tool_result", "tool_use_id": tool_use_id, "content": content})
                                }
                            })
                            .collect();
                        Value::Array(api_blocks)
                    }
                };
                serde_json::json!({"role": msg.role, "content": content})
            })
            .collect();

        // Convert tool definitions to Anthropic format
        let api_tools: Vec<Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            })
            .collect();

        let mut body = serde_json::json!({
            "anthropic_version": "vertex-2023-10-16",
            "messages": api_messages,
            "max_tokens": max_tokens.unwrap_or(4096),
        });
        if !system_prompt.is_empty() {
            body["system"] = system_prompt.into();
        }
        if !api_tools.is_empty() {
            body["tools"] = Value::Array(api_tools);
        }
        body
    }

    /// Parse a Claude response that may contain tool_use blocks.
    fn parse_tool_calling_response(response: &Value) -> Result<ToolCallingResponse> {
        let content = response
            .get("content")
            .and_then(|c| c.as_array())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Expected content array in Claude response: {}",
                    serde_json::to_string(response).unwrap_or_default()
                )
            })?;

        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();

        for block in content {
            match block.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                        text_parts.push(text.to_string());
                    }
                }
                Some("tool_use") => {
                    let id = block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = block
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let input = block.get("input").cloned().unwrap_or(Value::Object(Default::default()));
                    tool_calls.push(ToolCall { id, name, input });
                }
                _ => {}
            }
        }

        let stop_reason = response
            .get("stop_reason")
            .and_then(|s| s.as_str())
            .unwrap_or("end_turn")
            .to_string();

        Ok(ToolCallingResponse {
            text: text_parts.join(""),
            tool_calls,
            stop_reason,
        })
    }

    /// Build the Google generateContent request body for Gemini on Vertex AI.
    fn gemini_request_body(
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: Option<u32>,
    ) -> Value {
        let mut generation_config = serde_json::json!({});
        if let Some(max) = max_tokens {
            generation_config["maxOutputTokens"] = max.into();
        }
        serde_json::json!({
            "systemInstruction": {
                "parts": [{"text": system_prompt}]
            },
            "contents": [
                {"role": "user", "parts": [{"text": user_prompt}]}
            ],
            "generationConfig": generation_config,
        })
    }

    /// Extract text from a Claude (Anthropic Messages API) response.
    ///
    /// Expected shape: `{"content": [{"type": "text", "text": "..."}], ...}`
    fn extract_claude_text(response: &Value) -> Result<String> {
        response
            .pointer("/content/0/text")
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Unexpected Claude Vertex AI response shape: {}",
                    serde_json::to_string(response).unwrap_or_default()
                )
            })
    }

    /// Extract text from a Gemini (generateContent) response.
    ///
    /// Expected shape: `{"candidates": [{"content": {"parts": [{"text": "..."}]}}]}`
    fn extract_gemini_text(response: &Value) -> Result<String> {
        response
            .pointer("/candidates/0/content/parts/0/text")
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Unexpected Gemini Vertex AI response shape: {}",
                    serde_json::to_string(response).unwrap_or_default()
                )
            })
    }

    /// Stream a Claude completion via `streamRawPredict`, parsing Anthropic SSE events.
    ///
    /// SSE event format:
    /// ```text
    /// data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"..."}}
    /// ```
    async fn stream_claude(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: Option<u32>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
        use futures_util::StreamExt;

        let token = self.bearer_token().await?;
        let url = self.streaming_endpoint_url();
        let body = Self::claude_request_body(system_prompt, user_prompt, max_tokens, true);

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .context("Vertex AI streamRawPredict request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Vertex AI streamRawPredict returned {status}: {text}");
        }

        // Parse SSE events from the byte stream.
        let byte_stream = resp.bytes_stream();
        let text_stream = byte_stream
            .map(|chunk_result| -> Result<String> {
                let chunk = chunk_result.context("Error reading SSE stream chunk")?;
                let raw = std::str::from_utf8(&chunk).context("SSE chunk is not valid UTF-8")?;

                // Collect text deltas from all `data:` lines in this chunk.
                let mut texts = Vec::new();
                for line in raw.lines() {
                    let Some(json_str) = line.strip_prefix("data: ") else {
                        continue;
                    };
                    if json_str == "[DONE]" {
                        break;
                    }
                    let Ok(event) = serde_json::from_str::<Value>(json_str) else {
                        continue;
                    };
                    if event.get("type").and_then(Value::as_str) == Some("content_block_delta") {
                        if let Some(text) = event.pointer("/delta/text").and_then(Value::as_str) {
                            texts.push(text.to_string());
                        }
                    }
                }
                Ok(texts.join(""))
            })
            // Drop empty chunks (non-delta events) to avoid noisy empty strings.
            .filter(|result| {
                let keep = match result {
                    Ok(s) => !s.is_empty(),
                    Err(_) => true,
                };
                std::future::ready(keep)
            });

        Ok(Box::pin(text_stream))
    }
}

#[async_trait]
impl LlmPort for RigVertexAiAdapter {
    async fn complete(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: Option<u32>,
    ) -> Result<String> {
        if self.is_claude() {
            let body = Self::claude_request_body(system_prompt, user_prompt, max_tokens, false);
            let response = self.post_json(&self.endpoint_url(), body).await?;
            Self::extract_claude_text(&response)
        } else {
            let body = Self::gemini_request_body(system_prompt, user_prompt, max_tokens);
            let response = self.post_json(&self.endpoint_url(), body).await?;
            Self::extract_gemini_text(&response)
        }
    }

    async fn predict_json(&self, system_prompt: &str, user_prompt: &str) -> Result<Value> {
        let json_system = format!(
            "{}\nRespond with valid JSON only, no markdown code fences.",
            system_prompt
        );
        let text = self.complete(&json_system, user_prompt, None).await?;
        serde_json::from_str(&text).map_err(|e| {
            anyhow::anyhow!(
                "Failed to parse Vertex AI response as JSON: {}: {:?}",
                e,
                text
            )
        })
    }

    async fn stream_complete(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: Option<u32>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
        if self.is_claude() {
            return self
                .stream_claude(system_prompt, user_prompt, max_tokens)
                .await;
        }

        // Gemini: fall back to non-streaming complete() and emit as a single chunk.
        let text = self
            .complete(system_prompt, user_prompt, max_tokens)
            .await?;
        let chunks: Vec<Result<String>> = vec![Ok(text)];
        Ok(Box::pin(futures_util::stream::iter(chunks)))
    }

    async fn complete_with_tools(
        &self,
        system_prompt: &str,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        max_tokens: Option<u32>,
    ) -> Result<ToolCallingResponse> {
        if !self.is_claude() {
            // Gemini: fall back to text-only completion
            let user_text = messages
                .iter()
                .filter(|m| m.role == "user")
                .filter_map(|m| match &m.content {
                    ConversationContent::Text(t) => Some(t.as_str()),
                    _ => None,
                })
                .last()
                .unwrap_or("");
            let text = self.complete(system_prompt, user_text, max_tokens).await?;
            return Ok(ToolCallingResponse {
                text,
                tool_calls: vec![],
                stop_reason: "end_turn".to_string(),
            });
        }

        let body = Self::claude_request_body_with_tools(system_prompt, messages, tools, max_tokens);
        let response = self.post_json(&self.endpoint_url(), body).await?;
        Self::parse_tool_calling_response(&response)
    }
}

// ── ADC token fetching ───────────────────────────────────────────────────────

/// Fetch an ADC access token and its expiry (Unix epoch seconds).
///
/// Priority:
/// 1. GCE metadata server (GYRE_VERTEX_METADATA_TOKEN=1 or auto-detect)
/// 2. Service account key file at GOOGLE_APPLICATION_CREDENTIALS
async fn fetch_adc_token() -> Result<(String, u64)> {
    // Try metadata server first (running on GCP).
    if let Ok(token) = fetch_metadata_token().await {
        return Ok(token);
    }

    // Fall back to service account key file.
    fetch_service_account_token().await
}

/// Fetch a token from the GCE instance metadata server.
async fn fetch_metadata_token() -> Result<(String, u64)> {
    let client = reqwest::Client::new();
    let resp = client
        .get(
            "http://metadata.google.internal/computeMetadata/v1/instance/\
             service-accounts/default/token",
        )
        .header("Metadata-Flavor", "Google")
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
        .context("Metadata server not reachable")?;

    if !resp.status().is_success() {
        anyhow::bail!("Metadata server returned {}", resp.status());
    }

    let json: Value = resp.json().await?;
    let token = json["access_token"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No access_token in metadata response"))?
        .to_string();
    let expires_in = json["expires_in"].as_u64().unwrap_or(3600);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Ok((token, now + expires_in))
}

/// Resolve the credentials file path.
///
/// Priority:
/// 1. `GOOGLE_APPLICATION_CREDENTIALS` env var
/// 2. `~/.config/gcloud/application_default_credentials.json` (ADC default)
fn resolve_credentials_path() -> Result<String> {
    if let Ok(path) = std::env::var("GOOGLE_APPLICATION_CREDENTIALS") {
        return Ok(path);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let default_path = format!("{home}/.config/gcloud/application_default_credentials.json");
    Ok(default_path)
}

/// Fetch a token using a credentials file.
///
/// Supports both service account key files and ADC user credentials
/// (produced by `gcloud auth application-default login`).
async fn fetch_service_account_token() -> Result<(String, u64)> {
    let creds_path = resolve_credentials_path()
        .context("Cannot resolve credentials path: GOOGLE_APPLICATION_CREDENTIALS is not set and metadata server is unavailable")?;

    let creds_json = tokio::fs::read_to_string(&creds_path)
        .await
        .with_context(|| format!("Cannot read credentials file: {creds_path}"))?;

    let parsed: serde_json::Value =
        serde_json::from_str(&creds_json).context("Invalid credentials JSON")?;

    if parsed.get("client_email").is_some() {
        // Service account key file.
        let creds: ServiceAccountKey =
            serde_json::from_value(parsed).context("Failed to parse service account key fields")?;
        exchange_service_account_jwt(&creds).await
    } else if parsed.get("refresh_token").is_some() {
        // ADC user credentials (gcloud auth application-default login).
        let creds: UserCredentials =
            serde_json::from_value(parsed).context("Failed to parse ADC user credential fields")?;
        exchange_refresh_token(&creds).await
    } else {
        anyhow::bail!(
            "Unrecognized credentials format in {creds_path}: \
             expected 'client_email' (service account) or 'refresh_token' (user ADC)"
        )
    }
}

/// Exchange an OAuth2 refresh token for an access token.
async fn exchange_refresh_token(creds: &UserCredentials) -> Result<(String, u64)> {
    let client = reqwest::Client::new();
    let resp = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", creds.client_id.as_str()),
            ("client_secret", creds.client_secret.as_str()),
            ("refresh_token", creds.refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .context("Google token endpoint request failed (refresh_token)")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Google token endpoint returned {status}: {text}");
    }

    let json: Value = resp.json().await?;
    let token = json["access_token"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No access_token in refresh token response"))?
        .to_string();
    let expires_in = json["expires_in"].as_u64().unwrap_or(3600);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    Ok((token, now + expires_in))
}

/// Sign a JWT with the service account private key and exchange it for an
/// OAuth2 access token via the Google token endpoint.
async fn exchange_service_account_jwt(creds: &ServiceAccountKey) -> Result<(String, u64)> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let exp = now + 3600;

    let client_email = &creds.client_email;
    let private_key = &creds.private_key;

    // Build JWT header + claims.
    let header = serde_json::json!({
        "alg": "RS256",
        "typ": "JWT"
    });
    let claims = serde_json::json!({
        "iss": client_email,
        "sub": client_email,
        "aud": "https://oauth2.googleapis.com/token",
        "scope": "https://www.googleapis.com/auth/cloud-platform",
        "iat": now,
        "exp": exp,
    });

    let header_b64 = base64url_encode(serde_json::to_string(&header)?.as_bytes());
    let claims_b64 = base64url_encode(serde_json::to_string(&claims)?.as_bytes());
    let signing_input = format!("{}.{}", header_b64, claims_b64);

    let signature = rs256_sign(private_key, signing_input.as_bytes())
        .context("Failed to sign JWT with service account key")?;
    let jwt = format!("{}.{}", signing_input, base64url_encode(&signature));

    // Exchange JWT for access token.
    let client = reqwest::Client::new();
    let resp = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", &jwt),
        ])
        .send()
        .await
        .context("Google token endpoint request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Google token endpoint returned {status}: {text}");
    }

    let json: Value = resp.json().await?;
    let token = json["access_token"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No access_token in OAuth2 response"))?
        .to_string();
    let expires_in = json["expires_in"].as_u64().unwrap_or(3600);
    let now2 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Ok((token, now2 + expires_in))
}

fn base64url_encode(data: &[u8]) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    URL_SAFE_NO_PAD.encode(data)
}

/// Sign `data` with an RSA private key (PEM format) using RS256.
fn rs256_sign(pem_key: &str, data: &[u8]) -> Result<Vec<u8>> {
    use ring::signature::{RsaKeyPair, RSA_PKCS1_SHA256};

    // Strip PEM headers and decode base64.
    let pem_body: String = pem_key
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect();
    let der = base64_decode_standard(&pem_body).context("Failed to decode RSA private key PEM")?;

    let key_pair = RsaKeyPair::from_pkcs8(&der)
        .map_err(|e| anyhow::anyhow!("Failed to parse RSA private key (PKCS8): {:?}", e))?;

    let rng = ring::rand::SystemRandom::new();
    let mut signature = vec![0u8; key_pair.public().modulus_len()];
    key_pair
        .sign(&RSA_PKCS1_SHA256, &rng, data, &mut signature)
        .map_err(|_| anyhow::anyhow!("RSA signing failed"))?;

    Ok(signature)
}

fn base64_decode_standard(s: &str) -> Result<Vec<u8>> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    STANDARD
        .decode(s.trim())
        .map_err(|e| anyhow::anyhow!("Base64 decode error: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_adapter(model: &str) -> RigVertexAiAdapter {
        RigVertexAiAdapter {
            project: "my-project".to_string(),
            location: "us-central1".to_string(),
            model: model.to_string(),
            client: reqwest::Client::new(),
            token_cache: Arc::new(RwLock::new(TokenCache::default())),
        }
    }

    #[test]
    fn from_env_fails_without_project() {
        let original = std::env::var("GYRE_VERTEX_PROJECT").ok();
        std::env::remove_var("GYRE_VERTEX_PROJECT");

        let result = RigVertexAiFactory::from_env();
        assert!(
            result.is_err(),
            "Expected error when GYRE_VERTEX_PROJECT is unset"
        );

        if let Some(val) = original {
            std::env::set_var("GYRE_VERTEX_PROJECT", val);
        }
    }

    #[test]
    fn endpoint_url_gemini_uses_generate_content() {
        let adapter = make_adapter("gemini-2.0-flash-001");
        let url = adapter.endpoint_url();
        assert!(url.contains("my-project"));
        assert!(url.contains("us-central1"));
        assert!(url.contains("gemini-2.0-flash-001"));
        assert!(url.contains("publishers/google/"));
        assert!(
            url.contains(":generateContent"),
            "Gemini must use generateContent, got: {url}"
        );
    }

    #[test]
    fn endpoint_url_claude_uses_raw_predict() {
        let adapter = make_adapter("claude-opus-4-6@default");
        let url = adapter.endpoint_url();
        assert!(url.contains("my-project"));
        assert!(url.contains("us-central1"));
        assert!(url.contains("claude-opus-4-6@default"));
        assert!(
            url.contains("publishers/anthropic/"),
            "Claude must use anthropic publisher, got: {url}"
        );
        assert!(
            url.contains(":rawPredict"),
            "Claude must use rawPredict endpoint, got: {url}"
        );
        assert!(
            !url.contains("generateContent"),
            "Claude must NOT use generateContent, got: {url}"
        );
    }

    #[test]
    fn streaming_endpoint_url_claude_uses_stream_raw_predict() {
        let adapter = make_adapter("claude-sonnet-4-6@default");
        let url = adapter.streaming_endpoint_url();
        assert!(url.contains("publishers/anthropic/"));
        assert!(
            url.contains(":streamRawPredict"),
            "Claude streaming must use streamRawPredict, got: {url}"
        );
    }

    #[test]
    fn streaming_endpoint_url_gemini_uses_stream_generate_content() {
        let adapter = make_adapter("gemini-2.0-flash-001");
        let url = adapter.streaming_endpoint_url();
        assert!(url.contains("publishers/google/"));
        assert!(
            url.contains(":streamGenerateContent"),
            "Gemini streaming must use streamGenerateContent, got: {url}"
        );
    }

    #[test]
    fn claude_request_body_format() {
        let body = RigVertexAiAdapter::claude_request_body(
            "You are a helpful assistant.",
            "Hello!",
            Some(1024),
            false,
        );
        assert_eq!(body["anthropic_version"], "vertex-2023-10-16");
        assert_eq!(body["max_tokens"], 1024);
        // Non-streaming rawPredict calls must NOT include the stream field.
        assert!(
            body.get("stream").is_none() || body["stream"].is_null(),
            "Non-streaming body must omit stream field, got: {body}"
        );
        assert_eq!(body["system"], "You are a helpful assistant.");
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "Hello!");
    }

    #[test]
    fn claude_request_body_no_system_omits_field() {
        let body = RigVertexAiAdapter::claude_request_body("", "Hello!", None, false);
        assert!(
            body.get("system").is_none() || body["system"].is_null(),
            "Empty system prompt should not produce a system field, got: {body}"
        );
        assert_eq!(
            body["max_tokens"], 4096,
            "Default max_tokens should be 4096"
        );
    }

    #[test]
    fn claude_request_body_stream_true() {
        let body = RigVertexAiAdapter::claude_request_body("sys", "user", None, true);
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn extract_claude_text_success() {
        let response = serde_json::json!({
            "id": "msg_01",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello, world!"}],
            "stop_reason": "end_turn"
        });
        let text = RigVertexAiAdapter::extract_claude_text(&response).unwrap();
        assert_eq!(text, "Hello, world!");
    }

    #[test]
    fn extract_claude_text_missing_content_returns_error() {
        let response = serde_json::json!({"candidates": [{"content": {"parts": [{"text": "gemini response"}]}}]});
        let result = RigVertexAiAdapter::extract_claude_text(&response);
        assert!(result.is_err(), "Should fail on Gemini-shaped response");
    }

    #[test]
    fn extract_gemini_text_success() {
        let response = serde_json::json!({
            "candidates": [{"content": {"parts": [{"text": "Gemini says hi"}]}}]
        });
        let text = RigVertexAiAdapter::extract_gemini_text(&response).unwrap();
        assert_eq!(text, "Gemini says hi");
    }

    #[test]
    fn extract_gemini_text_missing_returns_error() {
        let response =
            serde_json::json!({"content": [{"type": "text", "text": "claude response"}]});
        let result = RigVertexAiAdapter::extract_gemini_text(&response);
        assert!(result.is_err(), "Should fail on Claude-shaped response");
    }

    #[test]
    fn detect_service_account_credentials() {
        let json = serde_json::json!({
            "type": "service_account",
            "client_email": "test@project.iam.gserviceaccount.com",
            "private_key": "-----BEGIN RSA PRIVATE KEY-----\nfake\n-----END RSA PRIVATE KEY-----\n",
        });
        assert!(json.get("client_email").is_some());
        assert!(json.get("refresh_token").is_none());
        let creds: Result<ServiceAccountKey, _> = serde_json::from_value(json);
        assert!(creds.is_ok());
    }

    #[test]
    fn detect_user_adc_credentials() {
        let json = serde_json::json!({
            "type": "authorized_user",
            "client_id": "123456789.apps.googleusercontent.com",
            "client_secret": "GOCSPX-fake-secret",
            "refresh_token": "1//fake-refresh-token",
        });
        assert!(json.get("client_email").is_none());
        assert!(json.get("refresh_token").is_some());
        let creds: Result<UserCredentials, _> = serde_json::from_value(json);
        assert!(creds.is_ok());
        let creds = creds.unwrap();
        assert_eq!(creds.client_id, "123456789.apps.googleusercontent.com");
        assert_eq!(creds.refresh_token, "1//fake-refresh-token");
    }

    #[test]
    fn resolve_credentials_path_uses_env_var() {
        std::env::set_var("GOOGLE_APPLICATION_CREDENTIALS", "/tmp/test-creds.json");
        let path = resolve_credentials_path().unwrap();
        assert_eq!(path, "/tmp/test-creds.json");
        std::env::remove_var("GOOGLE_APPLICATION_CREDENTIALS");
    }

    #[test]
    fn resolve_credentials_path_falls_back_to_gcloud_default() {
        let saved = std::env::var("GOOGLE_APPLICATION_CREDENTIALS").ok();
        std::env::remove_var("GOOGLE_APPLICATION_CREDENTIALS");
        let path = resolve_credentials_path().unwrap();
        assert!(
            path.ends_with("/.config/gcloud/application_default_credentials.json"),
            "Expected gcloud default path, got: {path}"
        );
        if let Some(v) = saved {
            std::env::set_var("GOOGLE_APPLICATION_CREDENTIALS", v);
        }
    }

    #[test]
    fn unrecognized_credentials_format_detected() {
        // A JSON blob with neither client_email nor refresh_token.
        let json = serde_json::json!({"some_field": "some_value"});
        assert!(json.get("client_email").is_none());
        assert!(json.get("refresh_token").is_none());
    }
}
