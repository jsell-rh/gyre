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
        let resp = self
            .http
            .post(&url)
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
        let resp = self
            .http
            .post(&url)
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

    /// GET a KV v2 data path with `token`; `Ok(None)` on HTTP 404.
    async fn kv_read(&self, path: &str, token: &str) -> Result<Option<VaultSecretPayload>> {
        let url = format!("{}/v1/{path}", self.config.addr);
        let mut req = self.http.get(&url);
        req = Self::with_auth(req, token, self.config.namespace.as_deref());
        let resp = req.send().await.context("Vault read request failed")?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
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
        Ok(Some(
            serde_json::from_value(data).context("Vault secret payload is malformed")?,
        ))
    }

    /// Read the id index entry with the server token.
    async fn read_index(&self, tenant_id: &str, id: &str) -> Result<Option<VaultIndexEntry>> {
        let path = self.index_path(tenant_id, id);
        self.server_token_retry(move |token| {
            let url = format!("{}/v1/{}", self.config.addr, path);
            async move {
                let mut req = self.http.get(&url);
                req = Self::with_auth(req, &token, None);
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
    /// (equivalent to the LIST verb).
    async fn kv_list(&self, path: &str, token: &str) -> Result<Vec<String>> {
        let url = format!("{}/v1/{path}?list=true", self.config.addr);
        let mut req = self.http.get(&url);
        req = Self::with_auth(req, token, self.config.namespace.as_deref());
        let resp = req.send().await.context("Vault list request failed")?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }
        anyhow::ensure!(
            resp.status().is_success(),
            "Vault list of {path} failed: HTTP {}",
            resp.status()
        );
        let body: serde_json::Value = resp.json().await.context("Vault list response")?;
        Ok(body
            .pointer("/data/keys")
            .and_then(|k| k.as_array())
            .map(|keys| {
                keys.iter()
                    .filter_map(|k| k.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default())
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

    /// Read the secret's payload by id (server token).
    async fn read_by_id(&self, id: &Id, tenant_id: &str) -> Result<Option<Secret>> {
        let Some(idx) = self.read_index(tenant_id, id.as_str()).await? else {
            return Ok(None);
        };
        let path = self.data_path(tenant_id, &idx.scope, &idx.scope_id, &idx.name);
        let token = self.server_token().await?;
        let payload = self.kv_read(&path, &token).await?;
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
}

/// Whether an error is Vault rejecting our token (403 on read paths) —
/// signals the AppRole token needs re-login.
fn is_auth_rejected(e: &anyhow::Error) -> bool {
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
        let token = self.server_token().await?;
        if self.kv_read(&data_path, &token).await?.is_some() {
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
        self.server_token_retry(|token| {
            let payload = payload.clone();
            let data_path = data_path.clone();
            async move {
                let url = format!("{}/v1/{}", self.config.addr, data_path);
                let mut req = self.http.post(&url);
                req = Self::with_auth(req, &token, self.config.namespace.as_deref());
                let resp = req
                    .json(&serde_json::json!({ "data": payload }))
                    .send()
                    .await
                    .context("Vault write request failed")?;
                anyhow::ensure!(
                    resp.status().is_success(),
                    "Vault write of secret failed: HTTP {}",
                    resp.status()
                );
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
        let token = self.server_token().await?;
        match self.kv_read(&path, &token).await? {
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
        let list_path = self.metadata_path(tenant_id, scope.as_str(), scope_id);
        let token = self.server_token().await?;
        let names = self.kv_list(&list_path, &token).await?;
        let mut secrets = Vec::with_capacity(names.len());
        for name in names {
            let path = self.data_path(tenant_id, scope.as_str(), scope_id, &name);
            if let Some(p) = self.kv_read(&path, &token).await? {
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
        self.server_token_retry(|token| {
            let data_meta = data_meta.clone();
            async move {
                let url = format!("{}/v1/{}", self.config.addr, data_meta);
                let mut req = self.http.delete(&url);
                req = Self::with_auth(req, &token, None);
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
            async move {
                let url = format!("{}/v1/{}", self.config.addr, index_meta);
                let mut req = self.http.delete(&url);
                req = Self::with_auth(req, &token, None);
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
        let token = self.server_token().await?;
        let Some(mut payload) = self.kv_read(&path, &token).await? else {
            anyhow::bail!("secret {id} not found in tenant {tenant_id}");
        };
        payload.value_b64 = B64.encode(new_value);
        payload.last_rotated_at = Some(now_secs());

        let url = format!("{}/v1/{path}", self.config.addr);
        let mut req = self.http.post(&url);
        req = Self::with_auth(req, &token, self.config.namespace.as_deref());
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
        Ok(())
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
        // identity).
        let token = self.server_token().await?;
        self.resolve_cascade(&token, tenant_id, workspace_id, repo_id, task_id)
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
        // The agent never receives the server's token.
        let token = self.agent_token(agent_jwt).await?;
        self.resolve_cascade(&token, tenant_id, workspace_id, repo_id, task_id)
            .await
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

        // Finer scopes overwrite coarser same-name entries (nearest scope
        // wins), mirroring the database adapters.
        let mut by_name: std::collections::HashMap<String, VaultSecretPayload> =
            std::collections::HashMap::new();
        for (scope, sid) in &scope_targets {
            let list_path = self.metadata_path(tenant_id, scope, sid);
            for name in self.kv_list(&list_path, token).await? {
                let path = self.data_path(tenant_id, scope, sid, &name);
                if let Some(p) = self.kv_read(&path, token).await? {
                    // Expired secrets are never delivered.
                    let expired = p.expires_at.map(|e| e <= now).unwrap_or(false);
                    if !expired {
                        by_name.insert(p.name.clone(), p);
                    }
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
