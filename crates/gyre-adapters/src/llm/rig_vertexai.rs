//! Vertex AI LLM adapter — direct REST API implementation.
//!
//! Uses the Vertex AI `generateContent` REST endpoint via `reqwest` and
//! Google Application Default Credentials (ADC) via `google-cloud-auth`.
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
use gyre_ports::{LlmPort, LlmPortFactory};
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
    /// Return the generateContent endpoint URL for this adapter's model.
    ///
    /// Claude models (claude-*) are published by Anthropic; all others (e.g.
    /// Gemini) are published by Google. The publisher segment in the Vertex AI
    /// REST path must match the model's actual publisher.
    fn endpoint_url(&self) -> String {
        let publisher = if self.model.starts_with("claude") {
            "anthropic"
        } else {
            "google"
        };
        format!(
            "https://{location}-aiplatform.googleapis.com/v1/\
             projects/{project}/locations/{location}/\
             publishers/{publisher}/models/{model}:generateContent",
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

    /// Send a generateContent request and return the full response JSON.
    async fn generate_content(&self, body: Value) -> Result<Value> {
        let token = self.bearer_token().await?;
        let url = self.endpoint_url();

        let resp = self
            .client
            .post(&url)
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
        Ok(json)
    }

    /// Extract the text content from a generateContent response.
    fn extract_text(response: &Value) -> Result<String> {
        let text = response
            .pointer("/candidates/0/content/parts/0/text")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Unexpected Vertex AI response shape: {}",
                    serde_json::to_string(response).unwrap_or_default()
                )
            })?;
        Ok(text.to_string())
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
        let mut generation_config = serde_json::json!({});
        if let Some(max) = max_tokens {
            generation_config["maxOutputTokens"] = max.into();
        }

        let body = serde_json::json!({
            "systemInstruction": {
                "parts": [{"text": system_prompt}]
            },
            "contents": [
                {"role": "user", "parts": [{"text": user_prompt}]}
            ],
            "generationConfig": generation_config,
        });

        let response = self.generate_content(body).await?;
        Self::extract_text(&response)
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
        // The Vertex AI generateContent REST endpoint does not support streaming
        // without a separate streamGenerateContent call. We fall back to complete()
        // and emit the full response as a single chunk to satisfy the trait contract.
        let text = self
            .complete(system_prompt, user_prompt, max_tokens)
            .await?;
        let chunks: Vec<Result<String>> = vec![Ok(text)];
        Ok(Box::pin(futures_util::stream::iter(chunks)))
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
    fn endpoint_url_format_gemini() {
        let cache = Arc::new(RwLock::new(TokenCache::default()));
        let adapter = RigVertexAiAdapter {
            project: "my-project".to_string(),
            location: "us-central1".to_string(),
            model: "gemini-2.0-flash-001".to_string(),
            client: reqwest::Client::new(),
            token_cache: cache,
        };
        let url = adapter.endpoint_url();
        assert!(url.contains("my-project"));
        assert!(url.contains("us-central1"));
        assert!(url.contains("gemini-2.0-flash-001"));
        assert!(url.contains("publishers/google/"));
        assert!(url.contains("generateContent"));
    }

    #[test]
    fn endpoint_url_format_claude() {
        let cache = Arc::new(RwLock::new(TokenCache::default()));
        let adapter = RigVertexAiAdapter {
            project: "my-project".to_string(),
            location: "us-central1".to_string(),
            model: "claude-opus-4-6@default".to_string(),
            client: reqwest::Client::new(),
            token_cache: cache,
        };
        let url = adapter.endpoint_url();
        assert!(url.contains("my-project"));
        assert!(url.contains("us-central1"));
        assert!(url.contains("claude-opus-4-6@default"));
        assert!(
            url.contains("publishers/anthropic/"),
            "Claude must use anthropic publisher, got: {url}"
        );
        assert!(url.contains("generateContent"));
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
