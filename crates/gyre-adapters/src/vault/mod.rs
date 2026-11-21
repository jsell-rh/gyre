//! Vault-backed secret storage (TASK-273; admin-panel.md / platform-model.md
//! §7 Storage Backend: "Vault support as an option").
//!
//! Selected with `GYRE_SECRET_BACKEND=vault` (+ `GYRE_VAULT_ADDR` and auth
//! config). The [`SecretRepository`] port operations are served from Vault
//! KV v2 with Gyre as the intermediary: agents never receive Vault
//! credentials and never talk to Vault directly.
//!
//! - **Admin write path** (`create`/`delete`/`rotate` and unauthenticated
//!   reads): authenticated with the server's Vault token
//!   (`GYRE_VAULT_TOKEN`) or AppRole (`GYRE_VAULT_ROLE_ID` +
//!   `GYRE_VAULT_SECRET_ID`, logging in at first use).
//! - **Agent resolution** ([`SecretRepository::resolve_for_agent_as`]):
//!   per §7, authenticates to Vault's JWT auth backend with the agent's
//!   own JWT, so Vault policies scope the returned secrets. The agent
//!   never sees the server's token.
//!
//! Fail-closed: with Vault unreachable, every operation returns an error —
//! this repository has no database fallback by construction.
//!
//! Layout (under the configured KV v2 mount, default `secret`):
//!
//! - `gyre-secrets/{tenant}/{scope}/{scope_id}/{name}` — the secret: the
//!   base64-encoded value plus the secret's metadata (id, scope, type,
//!   expiry, …). Binary values are base64 at the Vault boundary; they never
//!   pass through a lossy UTF-8 conversion.
//! - `gyre-secrets/{tenant}/_idx/{id}` — id → (scope, scope_id, name)
//!   index entry so `get_value`/`find_by_id`/`delete`/`rotate` resolve an
//!   id without scanning.
//!
//! The HTTP client is latency-bounded (30 s total timeout).

use anyhow::{Context, Result};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use gyre_common::{Id, Secret, SecretScope};
use gyre_ports::SecretRepository;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::RwLock;

/// Vault client configuration (from `GYRE_VAULT_*` env vars).
#[derive(Debug, Clone)]
pub struct VaultConfig {
    /// Vault base address, e.g. `http://127.0.0.1:8200` (GYRE_VAULT_ADDR).
    pub addr: String,
    /// Static server token (GYRE_VAULT_TOKEN).
    pub token: Option<String>,
    /// AppRole login (GYRE_VAULT_ROLE_ID + GYRE_VAULT_SECRET_ID) — the
    /// client logs in and renews the token as needed.
    pub role_id: Option<String>,
    pub secret_id: Option<String>,
    /// KV v2 mount point (GYRE_VAULT_KV_MOUNT, default `secret`).
    pub kv_mount: String,
    /// JWT auth mount point (GYRE_VAULT_JWT_MOUNT, default `jwt`).
    pub jwt_mount: String,
    /// Role used for agent-JWT logins (GYRE_VAULT_JWT_ROLE, default
    /// `gyre-agent`).
    pub jwt_role: String,
    /// Vault Enterprise namespace (GYRE_VAULT_NAMESPACE), sent as
    /// `X-Vault-Namespace`.
    pub namespace: Option<String>,
}

impl VaultConfig {
    /// Build the configuration from the environment. Fails when
    /// `GYRE_VAULT_ADDR` is missing or neither a token nor a complete
    /// AppRole pair is configured — the backend must fail closed at wiring
    /// time rather than silently at request time.
    pub fn from_env() -> Result<Self> {
        let addr = std::env::var("GYRE_VAULT_ADDR")
            .context("GYRE_SECRET_BACKEND=vault requires GYRE_VAULT_ADDR")?;
        let token = std::env::var("GYRE_VAULT_TOKEN")
            .ok()
            .filter(|t| !t.is_empty());
        let role_id = std::env::var("GYRE_VAULT_ROLE_ID")
            .ok()
            .filter(|t| !t.is_empty());
        let secret_id = std::env::var("GYRE_VAULT_SECRET_ID")
            .ok()
            .filter(|t| !t.is_empty());
        anyhow::ensure!(
            token.is_some() || (role_id.is_some() && secret_id.is_some()),
            "GYRE_SECRET_BACKEND=vault requires GYRE_VAULT_TOKEN or GYRE_VAULT_ROLE_ID + \
             GYRE_VAULT_SECRET_ID"
        );
        Ok(Self {
            addr: addr.trim_end_matches('/').to_string(),
            token,
            role_id,
            secret_id,
            kv_mount: std::env::var("GYRE_VAULT_KV_MOUNT")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "secret".to_string()),
            jwt_mount: std::env::var("GYRE_VAULT_JWT_MOUNT")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "jwt".to_string()),
            jwt_role: std::env::var("GYRE_VAULT_JWT_ROLE")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "gyre-agent".to_string()),
            namespace: std::env::var("GYRE_VAULT_NAMESPACE")
                .ok()
                .filter(|v| !v.is_empty()),
        })
    }
}

/// The stored payload at the secret's data path.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct VaultSecretPayload {
    /// Base64-encoded plaintext value (binary-safe).
    value_b64: String,
    id: String,
    name: String,
    scope: String,
    scope_id: String,
    secret_type: String,
    created_by: String,
    created_at: u64,
    expires_at: Option<u64>,
    last_rotated_at: Option<u64>,
}

impl VaultSecretPayload {
    fn from_secret(secret: &Secret, value: &[u8]) -> Self {
        Self {
            value_b64: B64.encode(value),
            id: secret.id.to_string(),
            name: secret.name.clone(),
            scope: secret.scope.as_str().to_string(),
            scope_id: secret.scope_id.clone(),
            secret_type: secret.secret_type.as_str().to_string(),
            created_by: secret.created_by.clone(),
            created_at: secret.created_at,
            expires_at: secret.expires_at,
            last_rotated_at: secret.last_rotated_at,
        }
    }

    fn into_secret(self) -> Result<Secret> {
        Ok(Secret {
            id: Id::new(self.id),
            name: self.name,
            scope: SecretScope::from_str_opt(&self.scope)
                .with_context(|| format!("unknown secret scope {:?}", self.scope))?,
            scope_id: self.scope_id,
            secret_type: gyre_common::SecretType::from_str_opt(&self.secret_type)
                .with_context(|| format!("unknown secret type {:?}", self.secret_type))?,
            created_by: self.created_by,
            created_at: self.created_at,
            expires_at: self.expires_at,
            last_rotated_at: self.last_rotated_at,
            tenant_id: String::new(), // filled by the caller
        })
    }

    fn decode_value(&self) -> Result<Vec<u8>> {
        B64.decode(&self.value_b64)
            .context("stored Vault value is not valid base64")
    }
}

/// The id → location index entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct VaultIndexEntry {
    scope: String,
    scope_id: String,
    name: String,
}

/// Vault KV v2 client implementing the `SecretRepository` port.
pub struct VaultSecretRepository {
    config: VaultConfig,
    /// Latency-bounded HTTP client (30 s total timeout).
    http: reqwest::Client,
    /// Cached AppRole client token (none when a static token is used).
    approle_token: RwLock<Option<String>>,
}

/// Total outbound timeout for every Vault request (fail-closed under a hung
/// Vault, per the latency-bound requirement).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

impl VaultSecretRepository {
    pub fn new(config: VaultConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("building the Vault HTTP client")?;
        Ok(Self {
            config,
            http,
            approle_token: RwLock::new(None),
        })
    }

    /// The token for the admin (server) path: the static token, or an
    /// AppRole login (cached; re-logged-in when Vault rejects it).
    async fn server_token(&self) -> Result<String> {
        if let Some(t) = &self.config.token {
            return Ok(t.clone());
        }
        if let Some(cached) = self.approle_token.read().await.clone() {
            return Ok(cached);
        }
        let role_id = self
            .config
            .role_id
            .clone()
            .context("no Vault role_id configured")?;
        let secret_id = self
            .config
            .secret_id
            .clone()
            .context("no Vault secret_id configured")?;
        let url = format!("{}/v1/auth/approle/login", self.config.addr);
        let mut req = self.http.post(&url);
        // The namespace header must ride the LOGIN too: with
        // GYRE_VAULT_NAMESPACE set, a namespace-less login authenticates
        // against the root namespace while every KV operation targets the
        // configured one — both paths would fail (review task-273).
        if let Some(ns) = &self.config.namespace {
            req = req.header("X-Vault-Namespace", ns);
        }
        let resp = req
            .json(&serde_json::json!({ "role_id": role_id, "secret_id": secret_id }))
            .send()
            .await
            .context("Vault AppRole login request failed")?;
        anyhow::ensure!(
            resp.status().is_success(),
            "Vault AppRole login failed: HTTP {}",
            resp.status()
        );
        let body: serde_json::Value = resp.json().await.context("AppRole login response")?;
        let token = body
            .pointer("/auth/client_token")
            .and_then(|v| v.as_str())
            .context("AppRole login response missing client_token")?
            .to_string();
        *self.approle_token.write().await = Some(token.clone());
        Ok(token)
    }

    /// Invalidate a rejected AppRole token and retry once with a fresh
    /// login (token expiry mid-operation must not fail the operation).
    async fn server_token_retry<F, Fut, T>(&self, op: F) -> Result<T>
    where
        F: Fn(String) -> Fut + Clone,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let token = self.server_token().await?;
        match op(token.clone()).await {
            Err(e) if is_auth_rejected(&e) => {
                if self.config.token.is_none() {
                    *self.approle_token.write().await = None;
                }
                let fresh = self.server_token().await?;
                op(fresh).await
            }
            r => r,
        }
    }

    /// Authenticate with the agent's JWT via the JWT auth backend and
    /// return a scoped client token (platform-model.md §7).
    async fn agent_token(&self, agent_jwt: &str) -> Result<String> {
        let url = format!(
            "{}/v1/auth/{}/login",
            self.config.addr, self.config.jwt_mount
        );
        let mut req = self.http.post(&url);
        // Same namespace requirement as the AppRole login (review
        // task-273): a namespace-less JWT login would mint a token in the
        // root namespace that cannot read the configured namespace's KV
        // data.
        if let Some(ns) = &self.config.namespace {
            req = req.header("X-Vault-Namespace", ns);
        }
        let resp = req
            .json(&serde_json::json!({
                "jwt": agent_jwt,
                "role": self.config.jwt_role,
            }))
            .send()
            .await
            .context("Vault JWT login request failed")?;
        anyhow::ensure!(
            resp.status().is_success(),
            "Vault JWT login failed: HTTP {}",
            resp.status()
        );
        let body: serde_json::Value = resp.json().await.context("JWT login response")?;
        Ok(body
            .pointer("/auth/client_token")
            .and_then(|v| v.as_str())
            .context("JWT login response missing client_token")?
            .to_string())
    }

    fn data_path(&self, tenant_id: &str, scope: &str, scope_id: &str, name: &str) -> String {
        format!(
            "{}/data/gyre-secrets/{tenant_id}/{scope}/{scope_id}/{name}",
            self.config.kv_mount
        )
    }

    /// Validate a value before it becomes a Vault KV path segment: no
    /// separators (`/`, `\\`), no traversal segments (`..`, `.`), no
    /// control characters. The Admin API validates the same charset, but
    /// the port can be reached from other write paths — the adapter
    /// enforces its own path-safety boundary: reqwest/url collapses
    /// `../` segments, so a `../`-bearing segment would address outside
    /// the caller's tenant subtree whenever the server token's policy is
    /// broader than one tenant subtree, and a plain `/` in a segment
    /// breaks KV listing (only the first path component is returned).
    /// (Review task-273.)
    fn validate_path_segment(what: &str, value: &str) -> Result<()> {
        anyhow::ensure!(
            !value.is_empty() && value.len() <= 255,
            "{what} must be 1-255 characters"
        );
        anyhow::ensure!(
            !value.contains('/') && !value.contains('\\') && !value.contains("..") && value != ".",
            "{what} must not contain '/', '\\', or '.'/'..' traversal segments"
        );
        anyhow::ensure!(
            !value.chars().any(|c| c.is_control()),
            "{what} must not contain control characters"
        );
        Ok(())
    }

    /// Validate a secret name before it becomes a Vault KV path segment
    /// (see [`Self::validate_path_segment`]).
    fn validate_name(name: &str) -> Result<()> {
        Self::validate_path_segment("secret name", name)
    }

    /// Validate a secret id before it becomes a Vault KV path segment.
    /// The id reaches this adapter VERBATIM from the URL path of the
    /// delete/rotate routes (`Id::new(secret_id)`) — unlike the
    /// server-derived tenant scope, it is caller-controlled, so a crafted
    /// `../`-bearing id would escape the `_idx` subtree: read another
    /// tenant's index entry (cross-tenant metadata leak) and target an
    /// arbitrary metadata path for the index DELETE (review task-273).
    fn validate_id(id: &str) -> Result<()> {
        Self::validate_path_segment("secret id", id)
    }

    /// Validate a caller-supplied scope id before it becomes a Vault KV
    /// path segment: the request-body `scope_id` is interpolated
    /// unvalidated into the data/metadata paths otherwise (review
    /// task-273).
    fn validate_scope_id(scope_id: &str) -> Result<()> {
        Self::validate_path_segment("scope id", scope_id)
    }

    fn metadata_path(&self, tenant_id: &str, scope: &str, scope_id: &str) -> String {
        format!(
            "{}/metadata/gyre-secrets/{tenant_id}/{scope}/{scope_id}",
            self.config.kv_mount
        )
    }

    fn index_path(&self, tenant_id: &str, id: &str) -> String {
        format!(
            "{}/data/gyre-secrets/{tenant_id}/_idx/{id}",
            self.config.kv_mount
        )
    }

    fn index_metadata_path(&self, tenant_id: &str, id: &str) -> String {
        format!(
            "{}/metadata/gyre-secrets/{tenant_id}/_idx/{id}",
            self.config.kv_mount
        )
    }

    /// GET a KV v2 data path with `token`; `Ok(None)` on HTTP 404. A 403
    /// is a typed [`VaultPermissionDenied`] so callers can distinguish
    /// policy denial (scope the result) from transport failure (fail
    /// closed).
    async fn kv_read(&self, path: &str, token: &str) -> Result<Option<VaultSecretPayload>> {
        Ok(self
            .kv_read_versioned(path, token)
            .await?
            .map(|(payload, _)| payload))
    }

    /// [`Self::kv_read`] plus the version number of the read data (KV v2
    /// `/data/metadata/version`). Write paths that supersede a version
    /// (rotate, replace) use it to destroy the superseded version
    /// afterwards (review task-273).
    async fn kv_read_versioned(
        &self,
        path: &str,
        token: &str,
    ) -> Result<Option<(VaultSecretPayload, u64)>> {
        let url = format!("{}/v1/{path}", self.config.addr);
        let mut req = self.http.get(&url);
        req = Self::with_auth(req, token, self.config.namespace.as_deref());
        let resp = req.send().await.context("Vault read request failed")?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if resp.status() == reqwest::StatusCode::FORBIDDEN {
            return Err(permission_denied(path));
        }
        anyhow::ensure!(
            resp.status().is_success(),
            "Vault read of {path} failed: HTTP {}",
            resp.status()
        );
        let body: serde_json::Value = resp.json().await.context("Vault read response")?;
        let data = body
            .pointer("/data/data")
            .cloned()
            .context("Vault read response missing data")?;
        // A missing version on a 2xx read is an ERROR, not a silent 0: a
        // version of 0 would make the post-write destroy target nothing
        // ("versions 1..=0"), silently leaving the superseded value
        // readable (fail-open; review task-273).
        let version = body
            .pointer("/data/metadata/version")
            .and_then(|v| v.as_u64())
            .with_context(|| format!("Vault read of {path} missing version metadata"))?;
        let payload: VaultSecretPayload =
            serde_json::from_value(data).context("Vault secret payload is malformed")?;
        Ok(Some((payload, version)))
    }

    /// Read the id index entry with the server token. The id is
    /// validated as a path segment FIRST — it reaches this method
    /// verbatim from the delete/rotate URL paths, and every id-derived
    /// Vault path (`_idx` reads and metadata deletes) flows through here
    /// (review task-273).
    async fn read_index(&self, tenant_id: &str, id: &str) -> Result<Option<VaultIndexEntry>> {
        Self::validate_id(id)?;
        let path = self.index_path(tenant_id, id);
        // The namespace header must ride index reads too: with
        // GYRE_VAULT_NAMESPACE set, a namespace-less read 404s, which
        // silently fails OPEN (get_value/find_by_id return Ok(None) for a
        // secret that exists in the real namespace) — review task-273.
        let namespace = self.config.namespace.clone();
        self.server_token_retry(move |token| {
            let path = path.clone();
            let namespace = namespace.clone();
            async move {
                let url = format!("{}/v1/{}", self.config.addr, path);
                let mut req = self.http.get(&url);
                req = Self::with_auth(req, &token, namespace.as_deref());
                let resp = req.send().await.context("Vault index read failed")?;
                if resp.status() == reqwest::StatusCode::NOT_FOUND {
                    return Ok(None);
                }
                anyhow::ensure!(
                    resp.status().is_success(),
                    "Vault index read failed: HTTP {}",
                    resp.status()
                );
                let body: serde_json::Value = resp.json().await.context("index read response")?;
                let data = body
                    .pointer("/data/data")
                    .cloned()
                    .context("index entry missing data")?;
                Ok(Some(
                    serde_json::from_value(data).context("index entry is malformed")?,
                ))
            }
        })
        .await
    }

    /// LIST the keys under a KV v2 metadata path. Vault's HTTP API accepts
    /// the list operation as `GET <path>?list=true` on metadata paths
    /// (equivalent to the LIST verb). A 403 is a typed
    /// [`VaultPermissionDenied`]. A malformed 2xx response (missing
    /// `/data/keys`) is an ERROR, not an empty list — silently mapping it
    /// to `[]` would make list_by_scope return nothing with no failure
    /// (fail-open; review task-273).
    async fn kv_list(&self, path: &str, token: &str) -> Result<Vec<String>> {
        let url = format!("{}/v1/{path}?list=true", self.config.addr);
        let mut req = self.http.get(&url);
        req = Self::with_auth(req, token, self.config.namespace.as_deref());
        let resp = req.send().await.context("Vault list request failed")?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }
        if resp.status() == reqwest::StatusCode::FORBIDDEN {
            return Err(permission_denied(path));
        }
        anyhow::ensure!(
            resp.status().is_success(),
            "Vault list of {path} failed: HTTP {}",
            resp.status()
        );
        let body: serde_json::Value = resp.json().await.context("Vault list response")?;
        let keys = body
            .pointer("/data/keys")
            .and_then(|k| k.as_array())
            .with_context(|| format!("Vault list of {path} returned no key list"))?;
        Ok(keys
            .iter()
            .filter_map(|k| k.as_str().map(str::to_string))
            .collect())
    }

    fn with_auth(
        mut req: reqwest::RequestBuilder,
        token: &str,
        namespace: Option<&str>,
    ) -> reqwest::RequestBuilder {
        req = req.header("X-Vault-Token", token);
        if let Some(ns) = namespace {
            req = req.header("X-Vault-Namespace", ns);
        }
        req
    }

    /// KV data read with the server token, re-logging-in an expired
    /// AppRole token when Vault rejects it. Every server-token read goes
    /// through the retry wrapper — a cached AppRole token whose TTL
    /// expired must not permanently fail the operations that previously
    /// fetched it directly (review task-273: get_value / list_by_scope /
    /// rotate / resolve_for_agent 403'd forever until an unrelated
    /// create/delete refreshed the cache).
    async fn kv_read_server(&self, path: &str) -> Result<Option<VaultSecretPayload>> {
        Ok(self
            .kv_read_server_versioned(path)
            .await?
            .map(|(payload, _)| payload))
    }

    /// [`Self::kv_read_server`] plus the read version — the pre-write
    /// read of the rotate/replace paths, which destroy the superseded
    /// version after the write (review task-273).
    async fn kv_read_server_versioned(
        &self,
        path: &str,
    ) -> Result<Option<(VaultSecretPayload, u64)>> {
        let path = path.to_string();
        self.server_token_retry(|token| {
            let path = path.clone();
            async move { self.kv_read_versioned(&path, &token).await }
        })
        .await
    }

    /// KV metadata LIST with the server token (same retry contract as
    /// [`Self::kv_read_server`]).
    async fn kv_list_server(&self, path: &str) -> Result<Vec<String>> {
        let path = path.to_string();
        self.server_token_retry(|token| {
            let path = path.clone();
            async move { self.kv_list(&path, &token).await }
        })
        .await
    }

    /// Read the secret's payload by id (server token).
    async fn read_by_id(&self, id: &Id, tenant_id: &str) -> Result<Option<Secret>> {
        let Some(idx) = self.read_index(tenant_id, id.as_str()).await? else {
            return Ok(None);
        };
        let path = self.data_path(tenant_id, &idx.scope, &idx.scope_id, &idx.name);
        let payload = self.kv_read_server(&path).await?;
        match payload {
            None => Ok(None),
            Some(mut p) => {
                p.id = id.as_str().to_string();
                let mut secret = p.into_secret()?;
                secret.tenant_id = tenant_id.to_string();
                Ok(Some(secret))
            }
        }
    }

    /// POST a KV v2 data write of `payload` at `path` with the server
    /// token (a versioned append: the new version atomically supersedes
    /// the previous one). Shared by `create` and `replace` so both write
    /// paths enforce the identical success/namespace/retry contract.
    async fn kv_write<T: serde::Serialize>(&self, path: &str, payload: &T) -> Result<()> {
        let path = path.to_string();
        self.server_token_retry(|token| {
            let path = path.clone();
            async move {
                let url = format!("{}/v1/{}", self.config.addr, path);
                let mut req = self.http.post(&url);
                req = Self::with_auth(req, &token, self.config.namespace.as_deref());
                let resp = req
                    .json(&serde_json::json!({ "data": payload }))
                    .send()
                    .await
                    .with_context(|| format!("Vault write of {path} failed"))?;
                anyhow::ensure!(
                    resp.status().is_success(),
                    "Vault write of {path} failed: HTTP {}",
                    resp.status()
                );
                Ok(())
            }
        })
        .await
    }

    /// DELETE a KV v2 metadata path (destroys every version) with the
    /// server token; a 404 is success (already gone).
    async fn kv_delete_metadata(&self, path: &str) -> Result<()> {
        let path = path.to_string();
        self.server_token_retry(|token| {
            let path = path.clone();
            async move {
                let url = format!("{}/v1/{}", self.config.addr, path);
                let mut req = self.http.delete(&url);
                req = Self::with_auth(req, &token, self.config.namespace.as_deref());
                let resp = req.send().await.context("Vault delete request failed")?;
                anyhow::ensure!(
                    resp.status().is_success() || resp.status() == reqwest::StatusCode::NOT_FOUND,
                    "Vault delete of {path} failed: HTTP {}",
                    resp.status()
                );
                Ok(())
            }
        })
        .await
    }

    /// Destroy (permanently remove the contents of) specific versions of
    /// a KV v2 secret: `POST {mount}/destroy/{path}` with `{"versions":
    /// [...]}`. KV v2 writes are versioned appends — without destroying
    /// the superseded version, the OLD value stays readable at
    /// `GET ...?version=N` by any token with read on the path (the agent
    /// JWT's policy included) indefinitely, so a compromise-driven
    /// rotation leaves the compromised value retrievable (review
    /// task-273). Destroying versions `1..=prior` (rather than exactly
    /// the prior one) keeps the invariant across a retry after a failed
    /// destroy: the re-read current version is already the rotated one,
    /// and every version up to and including it is superseded. A destroy
    /// failure fails the operation — the rotation's security contract
    /// (old value not retrievable) is unmet; the operator grants the
    /// server token `update` on `<mount>/destroy/<path>`.
    async fn kv_destroy_versions(&self, data_path: &str, versions: &[u64]) -> Result<()> {
        // KV v2 destroy lives at `{mount}/destroy/{secret-path}` — the
        // data path's `data/` segment replaced by `destroy/`.
        let url_path = Self::destroy_url_path(&self.config.kv_mount, data_path);
        let versions = versions.to_vec();
        self.server_token_retry(move |token| {
            let url_path = url_path.clone();
            let versions = versions.clone();
            async move {
                let url = format!("{}/v1/{}", self.config.addr, url_path);
                let mut req = self.http.post(&url);
                req = Self::with_auth(req, &token, self.config.namespace.as_deref());
                let resp = req
                    .json(&serde_json::json!({
                        "versions": versions.iter().map(|v| v.to_string()).collect::<Vec<_>>()
                    }))
                    .send()
                    .await
                    .with_context(|| {
                        format!("Vault destroy of {url_path} versions {versions:?} failed")
                    })?;
                anyhow::ensure!(
                    resp.status().is_success(),
                    "Vault destroy of {url_path} versions {versions:?} failed: HTTP {}",
                    resp.status()
                );
                Ok(())
            }
        })
        .await
    }

    /// The KV v2 destroy URL path for a data path: `{mount}/destroy/…`
    /// (the data path's `data/` segment becomes `destroy/`).
    fn destroy_url_path(kv_mount: &str, data_path: &str) -> String {
        let prefix = format!("{kv_mount}/data/");
        let secret_path = data_path.strip_prefix(&prefix).unwrap_or(data_path);
        format!("{kv_mount}/destroy/{secret_path}")
    }
}

/// Vault explicitly rejected the request with HTTP 403: the authenticated
/// principal's policy denies this path. Distinguished from transport and
/// server failures so the agent-scoped resolution cascade can YIELD the
/// permitted subset instead of failing the whole resolution
/// (platform-model.md §7: "Vault returns scoped secrets" — a denial scopes,
/// it does not abort) while the server-token path still treats it as a
/// relogin signal.
#[derive(Debug)]
struct VaultPermissionDenied {
    path: String,
}

impl std::fmt::Display for VaultPermissionDenied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Vault denied access to {} (permission denied, HTTP 403)",
            self.path
        )
    }
}

impl std::error::Error for VaultPermissionDenied {}

fn permission_denied(path: &str) -> anyhow::Error {
    anyhow::Error::new(VaultPermissionDenied {
        path: path.to_string(),
    })
}

/// How the agent/server scope cascade treats a Vault policy denial
/// (HTTP 403) encountered while listing or reading a scope.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CascadeDenial {
    /// Fail the resolution. Server-token path: a 403 is an expired
    /// AppRole token (the retry wrapper re-logins) or a real
    /// misconfiguration — either way it must surface, never scope.
    Fail,
    /// Yield the permitted subset. Agent-JWT path (platform-model.md §7):
    /// Vault policies scope the secrets an agent receives — a role denied
    /// the tenant-level list still gets the workspace/repo/task scopes it
    /// may read. Fail-closed on everything that is not an explicit policy
    /// denial.
    YieldPermitted,
}

/// Whether an error is Vault rejecting our token (403/401 on read paths) —
/// signals the AppRole token needs re-login.
fn is_auth_rejected(e: &anyhow::Error) -> bool {
    if e.downcast_ref::<VaultPermissionDenied>().is_some() {
        return true;
    }
    e.chain()
        .any(|c| c.to_string().contains("HTTP 403") || c.to_string().contains("HTTP 401"))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[async_trait]
impl SecretRepository for VaultSecretRepository {
    async fn create(&self, secret: &Secret, value: &[u8]) -> Result<()> {
        let tenant_id = secret.tenant_id.clone();
        let scope = secret.scope.as_str().to_string();
        let scope_id = secret.scope_id.clone();
        let name = secret.name.clone();

        // Path-safety boundary (review task-273): the name and the
        // caller-supplied scope id become KV path segments, and the id is
        // validated inside read_index — reject separators/traversal
        // before anything else.
        Self::validate_name(&name)?;
        Self::validate_scope_id(&scope_id)?;

        // Port contract: fail if the same id, or the same
        // (scope, scope_id, name) within the tenant, already exists.
        if self
            .read_index(&tenant_id, secret.id.as_str())
            .await?
            .is_some()
        {
            anyhow::bail!("secret id {} already exists in Vault", secret.id);
        }
        let data_path = self.data_path(&tenant_id, &scope, &scope_id, &name);
        if self.kv_read_server(&data_path).await?.is_some() {
            anyhow::bail!(
                "secret '{}' already exists at scope {scope}:{scope_id}",
                secret.name
            );
        }

        let payload = VaultSecretPayload::from_secret(secret, value);
        let index = VaultIndexEntry {
            scope: scope.clone(),
            scope_id: scope_id.clone(),
            name: name.clone(),
        };
        let index_path = self.index_path(&tenant_id, secret.id.as_str());
        // KV v2 compare-and-swap (review task-273 repair): the two
        // pre-checks above are check-then-write — two concurrent creates
        // of the same (tenant, scope, scope_id, name) can both pass them
        // and both land as new KV versions, which violates the port's
        // duplicate constraint that the SQLite/PostgreSQL adapters
        // enforce absolutely via a UNIQUE index. `options.cas = 0` makes
        // the data write fail (HTTP 400, "check-and-set parameter did
        // not match the current version") unless the path holds zero
        // versions, so Vault itself enforces the constraint atomically;
        // that 400 maps to the port's duplicate error. The id index
        // check stays as-is (the index path is keyed by the id, and the
        // data-path cas is what guards the scoped-name uniqueness).
        let duplicate_err = format!(
            "secret '{}' already exists at scope {scope}:{scope_id}",
            name
        );
        self.server_token_retry(|token| {
            let payload = payload.clone();
            let data_path = data_path.clone();
            let duplicate_err = duplicate_err.clone();
            async move {
                let url = format!("{}/v1/{}", self.config.addr, data_path);
                let mut req = self.http.post(&url);
                req = Self::with_auth(req, &token, self.config.namespace.as_deref());
                let resp = req
                    .json(&serde_json::json!({
                        "data": payload,
                        "options": { "cas": 0 }
                    }))
                    .send()
                    .await
                    .context("Vault write request failed")?;
                let status = resp.status();
                if !status.is_success() {
                    let body = resp.text().await.unwrap_or_default();
                    if status.as_u16() == 400 && body.contains("check-and-set") {
                        anyhow::bail!("{duplicate_err}");
                    }
                    anyhow::bail!("Vault write of secret failed: HTTP {status}");
                }
                Ok(())
            }
        })
        .await?;
        self.server_token_retry(|token| {
            let index = index.clone();
            let index_path = index_path.clone();
            async move {
                let url = format!("{}/v1/{}", self.config.addr, index_path);
                let mut req = self.http.post(&url);
                req = Self::with_auth(req, &token, self.config.namespace.as_deref());
                let resp = req
                    .json(&serde_json::json!({ "data": index }))
                    .send()
                    .await
                    .context("Vault index write request failed")?;
                anyhow::ensure!(
                    resp.status().is_success(),
                    "Vault index write failed: HTTP {}",
                    resp.status()
                );
                Ok(())
            }
        })
        .await?;
        Ok(())
    }

    async fn get_value(&self, id: &Id, tenant_id: &str) -> Result<Option<Vec<u8>>> {
        let Some(idx) = self.read_index(tenant_id, id.as_str()).await? else {
            return Ok(None);
        };
        let path = self.data_path(tenant_id, &idx.scope, &idx.scope_id, &idx.name);
        match self.kv_read_server(&path).await? {
            None => Ok(None),
            Some(p) => Ok(Some(p.decode_value()?)),
        }
    }

    async fn find_by_id(&self, id: &Id, tenant_id: &str) -> Result<Option<Secret>> {
        self.read_by_id(id, tenant_id).await
    }

    async fn list_by_scope(
        &self,
        scope: SecretScope,
        scope_id: &str,
        tenant_id: &str,
    ) -> Result<Vec<Secret>> {
        // Path-safety boundary (review task-273): the request-body scope
        // id is a KV path segment — a `../`-bearing value would address
        // another scope's (or tenant's) subtree.
        Self::validate_scope_id(scope_id)?;
        let list_path = self.metadata_path(tenant_id, scope.as_str(), scope_id);
        let names = self.kv_list_server(&list_path).await?;
        let mut secrets = Vec::with_capacity(names.len());
        for name in names {
            let path = self.data_path(tenant_id, scope.as_str(), scope_id, &name);
            if let Some(p) = self.kv_read_server(&path).await? {
                let mut secret = p.into_secret()?;
                secret.tenant_id = tenant_id.to_string();
                secrets.push(secret);
            }
        }
        secrets.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(secrets)
    }

    async fn delete(&self, id: &Id, tenant_id: &str) -> Result<()> {
        let Some(idx) = self.read_index(tenant_id, id.as_str()).await? else {
            return Ok(());
        };
        let data_meta = format!(
            "{}/metadata/gyre-secrets/{tenant_id}/{}/{}/{}",
            self.config.kv_mount, idx.scope, idx.scope_id, idx.name
        );
        let index_meta = self.index_metadata_path(tenant_id, id.as_str());
        // The namespace header must ride the deletes too: with
        // GYRE_VAULT_NAMESPACE set, a namespace-less delete 404s (treated
        // as success) while the secret remains in the real namespace — a
        // silent non-enforcement of a destructive admin operation
        // (review task-273).
        let namespace = self.config.namespace.clone();
        self.server_token_retry(|token| {
            let data_meta = data_meta.clone();
            let namespace = namespace.clone();
            async move {
                let url = format!("{}/v1/{}", self.config.addr, data_meta);
                let mut req = self.http.delete(&url);
                req = Self::with_auth(req, &token, namespace.as_deref());
                let resp = req.send().await.context("Vault delete request failed")?;
                anyhow::ensure!(
                    resp.status().is_success() || resp.status() == reqwest::StatusCode::NOT_FOUND,
                    "Vault delete failed: HTTP {}",
                    resp.status()
                );
                Ok(())
            }
        })
        .await?;
        self.server_token_retry(|token| {
            let index_meta = index_meta.clone();
            let namespace = namespace.clone();
            async move {
                let url = format!("{}/v1/{}", self.config.addr, index_meta);
                let mut req = self.http.delete(&url);
                req = Self::with_auth(req, &token, namespace.as_deref());
                let resp = req.send().await.context("Vault index delete failed")?;
                anyhow::ensure!(
                    resp.status().is_success() || resp.status() == reqwest::StatusCode::NOT_FOUND,
                    "Vault index delete failed: HTTP {}",
                    resp.status()
                );
                Ok(())
            }
        })
        .await?;
        Ok(())
    }

    async fn rotate(&self, id: &Id, new_value: &[u8], tenant_id: &str) -> Result<()> {
        let Some(idx) = self.read_index(tenant_id, id.as_str()).await? else {
            anyhow::bail!("secret {id} not found in tenant {tenant_id}");
        };
        let path = self.data_path(tenant_id, &idx.scope, &idx.scope_id, &idx.name);
        let value_b64 = B64.encode(new_value);
        let rotated_at = now_secs();
        let not_found = format!("secret {id} not found in tenant {tenant_id}");
        let namespace = self.config.namespace.clone();
        // Read + write + destroy under the retry wrapper: an AppRole token
        // whose TTL expired between operations re-logins instead of 403ing
        // forever (review task-273). The read also captures the CURRENT
        // version so the superseded versions are destroyed after the
        // write — KV v2 keeps every prior version readable at
        // `?version=N`, so without the destroy a compromise-driven
        // rotation leaves the compromised value retrievable indefinitely
        // (review task-273).
        self.server_token_retry(move |token| {
            let path = path.clone();
            let value_b64 = value_b64.clone();
            let not_found = not_found.clone();
            let namespace = namespace.clone();
            async move {
                let Some((mut payload, prior_version)) =
                    self.kv_read_versioned(&path, &token).await?
                else {
                    anyhow::bail!("{not_found}");
                };
                payload.value_b64 = value_b64;
                payload.last_rotated_at = Some(rotated_at);

                let url = format!("{}/v1/{}", self.config.addr, path);
                let mut req = self.http.post(&url);
                req = Self::with_auth(req, &token, namespace.as_deref());
                let resp = req
                    .json(&serde_json::json!({ "data": payload }))
                    .send()
                    .await
                    .context("Vault rotate write failed")?;
                anyhow::ensure!(
                    resp.status().is_success(),
                    "Vault rotate write failed: HTTP {}",
                    resp.status()
                );
                // Every version up to and including the read one is now
                // superseded — destroy them all (robust across retries:
                // a failed destroy's retry re-reads the already-rotated
                // version and destroys everything up to it).
                let superseded: Vec<u64> = (1..=prior_version).collect();
                let destroy_url_path = Self::destroy_url_path(&self.config.kv_mount, &path);
                let url = format!("{}/v1/{}", self.config.addr, destroy_url_path);
                let mut req = self.http.post(&url);
                req = Self::with_auth(req, &token, namespace.as_deref());
                let resp = req
                    .json(&serde_json::json!({
                        "versions": superseded.iter().map(|v| v.to_string()).collect::<Vec<_>>()
                    }))
                    .send()
                    .await
                    .with_context(|| {
                        format!("Vault destroy of superseded versions {superseded:?} failed")
                    })?;
                anyhow::ensure!(
                    resp.status().is_success(),
                    "Vault destroy of superseded versions {superseded:?} failed: HTTP {}",
                    resp.status()
                );
                Ok(())
            }
        })
        .await
    }

    async fn resolve_for_agent(
        &self,
        tenant_id: &str,
        workspace_id: &str,
        repo_id: &str,
        task_id: Option<&str>,
    ) -> Result<Vec<(String, Vec<u8>)>> {
        // Server-token resolution: the same scope cascade the database
        // backends implement (used by internal callers without an agent
        // identity). Under the retry wrapper — an expired AppRole token
        // must re-login, not 403 the resolution forever (review
        // task-273). A denial here FAILS: the server token is the broad
        // admin identity, so a 403 is a relogin signal or a real
        // misconfiguration, never a per-agent policy boundary.
        let tenant = tenant_id.to_string();
        let ws = workspace_id.to_string();
        let repo = repo_id.to_string();
        let task = task_id.map(str::to_string);
        self.server_token_retry(move |token| {
            let tenant = tenant.clone();
            let ws = ws.clone();
            let repo = repo.clone();
            let task = task.clone();
            async move {
                self.resolve_cascade(
                    &token,
                    &tenant,
                    &ws,
                    &repo,
                    task.as_deref(),
                    CascadeDenial::Fail,
                )
                .await
            }
        })
        .await
    }

    async fn resolve_for_agent_as(
        &self,
        tenant_id: &str,
        workspace_id: &str,
        repo_id: &str,
        task_id: Option<&str>,
        agent_jwt: &str,
    ) -> Result<Vec<(String, Vec<u8>)>> {
        // platform-model.md §7: authenticate to Vault with the agent's JWT
        // (JWT auth backend) so Vault policies scope the returned secrets.
        // The agent never receives the server's token. A policy denial
        // (403) on a scope YIELDS the permitted subset — the §7 property
        // is that Vault's policies scope what the agent receives, not that
        // a restricted role fails the entire resolution (review
        // task-273).
        let token = self.agent_token(agent_jwt).await?;
        self.resolve_cascade(
            &token,
            tenant_id,
            workspace_id,
            repo_id,
            task_id,
            CascadeDenial::YieldPermitted,
        )
        .await
    }

    async fn replace(&self, secret: &Secret, value: &[u8]) -> Result<()> {
        let tenant_id = secret.tenant_id.clone();
        // Port contract: fail when the id is unknown.
        let Some(idx) = self.read_index(&tenant_id, secret.id.as_str()).await? else {
            anyhow::bail!(
                "secret {} not found in tenant {}",
                secret.id,
                secret.tenant_id
            );
        };
        Self::validate_name(&secret.name)?;
        Self::validate_scope_id(&secret.scope_id)?;

        // KV v2 writes are versioned appends at a path: posting the new
        // payload atomically supersedes the previous version — there is no
        // delete + create window in which the credential could be lost
        // (review task-273). When the (scope, scope_id, name) location is
        // unchanged — the rotation / re-enrollment cases — this is a
        // single write, followed by a DESTROY of the superseded versions
        // so the old value is not still readable at `?version=N`
        // (review task-273: same invariant the rotate path enforces).
        let payload = VaultSecretPayload::from_secret(secret, value);
        let new_path = self.data_path(
            &tenant_id,
            secret.scope.as_str(),
            &secret.scope_id,
            &secret.name,
        );
        let prior_version = self
            .kv_read_server_versioned(&new_path)
            .await?
            .map(|(_, v)| v);
        self.kv_write(&new_path, &payload).await?;
        if let Some(prior) = prior_version {
            let superseded: Vec<u64> = (1..=prior).collect();
            self.kv_destroy_versions(&new_path, &superseded).await?;
        }

        // Location changed (scope/name rewrite): repoint the id index and
        // remove the old data path. Order matters — the index is only
        // moved once the new payload is durable, and a crash before the
        // old-path delete leaves an orphaned KV entry (recoverable
        // housekeeping), never a lost credential.
        let location_changed = idx.scope != secret.scope.as_str()
            || idx.scope_id != secret.scope_id
            || idx.name != secret.name;
        if location_changed {
            let index = VaultIndexEntry {
                scope: secret.scope.as_str().to_string(),
                scope_id: secret.scope_id.clone(),
                name: secret.name.clone(),
            };
            let index_path = self.index_path(&tenant_id, secret.id.as_str());
            self.kv_write(&index_path, &index).await?;
            let old_data_meta = format!(
                "{}/metadata/gyre-secrets/{tenant_id}/{}/{}/{}",
                self.config.kv_mount, idx.scope, idx.scope_id, idx.name
            );
            self.kv_delete_metadata(&old_data_meta).await?;
        }
        Ok(())
    }
}

impl VaultSecretRepository {
    /// The scope cascade (tenant → workspace → repo → task, nearest scope
    /// wins) read with `token` — the server token for admin reads, the
    /// agent's JWT-derived token for agent resolution.
    async fn resolve_cascade(
        &self,
        token: &str,
        tenant_id: &str,
        workspace_id: &str,
        repo_id: &str,
        task_id: Option<&str>,
        denial: CascadeDenial,
    ) -> Result<Vec<(String, Vec<u8>)>> {
        let now = now_secs();
        let mut scope_targets: Vec<(String, String)> = vec![
            (
                SecretScope::Tenant.as_str().to_string(),
                tenant_id.to_string(),
            ),
            (
                SecretScope::Workspace.as_str().to_string(),
                workspace_id.to_string(),
            ),
            (SecretScope::Repo.as_str().to_string(), repo_id.to_string()),
        ];
        if let Some(tid) = task_id {
            scope_targets.push((SecretScope::Task.as_str().to_string(), tid.to_string()));
        }

        // Whether an error is an explicit Vault policy denial (HTTP 403)
        // — the only error class the YieldPermitted mode tolerates.
        let is_denial = |e: &anyhow::Error| {
            denial == CascadeDenial::YieldPermitted
                && e.downcast_ref::<VaultPermissionDenied>().is_some()
        };

        // Finer scopes overwrite coarser same-name entries (nearest scope
        // wins), mirroring the database adapters.
        let mut by_name: std::collections::HashMap<String, VaultSecretPayload> =
            std::collections::HashMap::new();
        for (scope, sid) in &scope_targets {
            let list_path = self.metadata_path(tenant_id, scope, sid);
            let names = match self.kv_list(&list_path, token).await {
                Ok(names) => names,
                // Policy-denied scope: the agent's role may not list
                // this subtree — the permitted scopes still resolve (§7
                // scoping, review task-273). Everything else fails
                // closed.
                Err(e) if is_denial(&e) => continue,
                Err(e) => return Err(e),
            };
            for name in names {
                let path = self.data_path(tenant_id, scope, sid, &name);
                match self.kv_read(&path, token).await {
                    Ok(Some(p)) => {
                        // Expired secrets are never delivered.
                        let expired = p.expires_at.map(|e| e <= now).unwrap_or(false);
                        if !expired {
                            by_name.insert(p.name.clone(), p);
                        }
                    }
                    Ok(None) => {}
                    // Policy-denied secret: skip it, keep the rest.
                    Err(e) if is_denial(&e) => continue,
                    Err(e) => return Err(e),
                }
            }
        }

        let mut resolved: Vec<(String, Vec<u8>)> = Vec::with_capacity(by_name.len());
        for (_, p) in by_name {
            resolved.push((p.name.clone(), p.decode_value()?));
        }
        resolved.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(resolved)
    }
}

#[cfg(test)]
mod tests;
