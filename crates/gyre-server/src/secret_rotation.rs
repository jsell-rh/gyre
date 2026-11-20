//! Secret expiry check background job (platform-model.md §7, task-098/100).
//!
//! Runs every 5 minutes (`secret_rotation_check` job). Scans tenant-,
//! workspace-, and repo-scoped secrets for expiry within the next hour.
//! For each expiring secret with a rotation handler (§7 Rotated type, e.g.
//! OAuth refresh tokens — see [`SecretRotationHandler`]) the job attempts
//! automatic rotation; secrets without a handler (or whose rotation
//! failed) get a priority-7 `SecretExpiring` notification for each
//! Admin/Owner workspace member responsible for the secret's scope:
//!
//! - repo-scoped secret  → Admin/Owner members of the repo's workspace
//! - workspace-scoped    → Admin/Owner members of that workspace
//! - tenant-scoped       → Admin/Owner members of every workspace in the tenant
//!
//! Re-creation is suppressed while an active `SecretExpiring` notification
//! for the same (user, workspace, secret) exists, and for 24 h after a user
//! dismisses one (same suppression shape as the trust-suggestion job).

use std::sync::Arc;

use anyhow::Context as _;
use gyre_common::{Id, Notification, NotificationType, Secret};
use tracing::{debug, info, warn};

use crate::AppState;

/// Window before expiry that triggers a notification (seconds).
const EXPIRY_WINDOW_SECS: u64 = 3600;

/// Minimum remaining lifetime that still justifies an automatic refresh
/// (seconds). Larger than the 5-minute job interval: a credential whose
/// natural lifetime is at or below the window (a Google access token is
/// exactly 3600 s) must not be re-refreshed on EVERY job cycle — the
/// refresh fires when the credential is genuinely near expiry, then the
/// new grant takes it out of the refresh threshold until it nears expiry
/// again (review task-273: perpetual refresh loop).
const REFRESH_THRESHOLD_MIN_SECS: u64 = 600;

/// Whether an expiring secret should be refreshed NOW.
///
/// The credential must be within [`EXPIRY_WINDOW_SECS`] of expiry (the
/// job's scan window — see [`expiring`]) AND meaningfully near expiry:
/// already expired, or with a remaining lifetime at or below a threshold
/// derived from the granted lifetime (`expires_at - last_rotated_at`, set
/// by the previous refresh). Without the threshold, a credential whose
/// natural lifetime is shorter than or equal to the window — Google
/// access tokens: `expires_in` 3600 against a 3600 s window — would be
/// refreshed on every 5-minute cycle forever: each refresh sets
/// `expires_at = now + 3600`, which never leaves the window.
///
/// The threshold is one fifth of the granted lifetime, clamped to
/// [REFRESH_THRESHOLD_MIN_SECS, EXPIRY_WINDOW_SECS]:
///
/// - Google SA (granted 3600 s): threshold 720 s — refreshed with ~12
///   minutes left, i.e. roughly every 48 minutes, not every cycle.
/// - Long-lived grants (days): threshold clamps to the window — the
///   pre-existing behavior (refresh within the last hour).
/// - Never-refreshed secrets (`last_rotated_at` unknown): threshold is
///   the full window, so the first refresh happens promptly and starts
///   the cadence.
fn should_refresh(secret: &Secret, now: u64) -> bool {
    let Some(expires_at) = secret.expires_at else {
        return false;
    };
    if expires_at <= now {
        // Already expired: refresh immediately (an admin must also know
        // either way, but a handler refresh revives it first).
        return true;
    }
    let remaining = expires_at - now;
    if remaining > EXPIRY_WINDOW_SECS {
        return false;
    }
    let threshold = match secret.last_rotated_at {
        Some(rotated_at) if rotated_at < expires_at => {
            let granted = expires_at - rotated_at;
            (granted / 5).clamp(REFRESH_THRESHOLD_MIN_SECS, EXPIRY_WINDOW_SECS)
        }
        _ => EXPIRY_WINDOW_SECS,
    };
    remaining <= threshold
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── Rotation handlers (platform-model.md §7 Rotated type) ───────────────────

/// Result of a successful handler rotation: the new plaintext value and the
/// new expiry (Unix epoch seconds).
pub struct RotatedSecret {
    pub value: Vec<u8>,
    pub expires_at: Option<u64>,
}

/// A rotation handler performs a real credential refresh for an expiring
/// secret (platform-model.md §7: Rotated secrets are refreshed by a
/// "Background job refreshes before expiry"). Handlers match on the
/// credential-document SHAPE — the decrypted value parsed as JSON — not
/// the secret name alone (TASK-273: every expiring credential is
/// dispatched through this registry).
///
/// Handlers receive the decrypted plaintext only as opaque bytes; they MUST
/// never log or serialize it.
#[async_trait::async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
pub trait SecretRotationHandler: Send + Sync {
    /// Handler name for run-log attribution ("refreshed via <name>").
    fn name(&self) -> &'static str;

    /// Whether this handler can rotate `secret` whose decrypted value is
    /// `value` (credential-document shape matching).
    fn handles(&self, secret: &Secret, value: &[u8]) -> bool;

    /// Refresh the credential and return the new value + expiry.
    async fn rotate(
        &self,
        http: &reqwest::Client,
        secret: &Secret,
        current_value: &[u8],
    ) -> anyhow::Result<RotatedSecret>;
}

/// OAuth 2.0 refresh-token grant handler for any secret whose value is an
/// OAuth credential document (TASK-273: credential-document shape, not
/// name suffix — an API key or other credential carrying this document is
/// refreshed the same way). Typically created by the Claude Max enrollment
/// flow as the scoped `claude_max_refresh_token` secret.
///
/// The secret's plaintext value is a JSON credential document:
///
/// ```json
/// { "token_endpoint": "https://issuer.example/token",
///   "client_id": "...", "client_secret": "...",
///   "refresh_token": "..." }
/// ```
///
/// The handler performs a standard `refresh_token` grant (RFC 6749 §6)
/// against `token_endpoint` and stores the updated document — new access
/// token, and the rotated refresh token when the provider issues one. The
/// secret's `expires_at` is extended by the provider's `expires_in`.
pub struct OAuthRefreshHandler;

/// The credential document carried as an `_refresh_token` secret's value.
#[derive(serde::Serialize, serde::Deserialize)]
struct OAuthCredentialDoc {
    token_endpoint: String,
    client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client_secret: Option<String>,
    refresh_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    access_token: Option<String>,
}

/// Whether `value` parses as an OAuth credential document.
fn parses_as_oauth_doc(value: &[u8]) -> bool {
    serde_json::from_slice::<OAuthCredentialDoc>(value).is_ok()
}

#[async_trait::async_trait]
impl SecretRotationHandler for OAuthRefreshHandler {
    fn name(&self) -> &'static str {
        "oauth_refresh_token_grant"
    }

    fn handles(&self, _secret: &Secret, value: &[u8]) -> bool {
        // Credential-document shape matching (TASK-273): the value itself
        // is an OAuth credential document, regardless of the secret name.
        parses_as_oauth_doc(value)
    }

    async fn rotate(
        &self,
        http: &reqwest::Client,
        _secret: &Secret,
        current_value: &[u8],
    ) -> anyhow::Result<RotatedSecret> {
        // Parse the document as a whole JSON object and only READ the
        // grant fields through the typed view: an admin-supplied OAuth
        // credential document can carry fields Gyre does not use (scope,
        // issuer, provider hints, …) that MUST survive the refresh — the
        // rotated document is re-serialized from the ORIGINAL value with
        // only the rotated tokens overwritten, never from a lossy typed
        // copy (review task-273: same treatment the GCP SA handler got).
        let mut doc: serde_json::Value = serde_json::from_slice(current_value).context(
            "value is not an OAuth credential document (JSON with token_endpoint, client_id, refresh_token)",
        )?;
        anyhow::ensure!(
            doc.is_object(),
            "OAuth credential document must be a JSON object"
        );
        let typed: OAuthCredentialDoc = serde_json::from_value(doc.clone()).context(
            "value is not an OAuth credential document (JSON with token_endpoint, client_id, refresh_token)",
        )?;

        let mut form: Vec<(&str, &str)> = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", typed.refresh_token.as_str()),
            ("client_id", typed.client_id.as_str()),
        ];
        if let Some(cs) = typed.client_secret.as_deref() {
            form.push(("client_secret", cs));
        }

        // `http` is the AppState client, built with a 30 s total timeout, so
        // a hung provider cannot stall the job loop.
        let resp = http
            .post(&typed.token_endpoint)
            .form(&form)
            .send()
            .await
            .context("token endpoint request failed")?;
        let status = resp.status();
        let body = resp.bytes().await.context("reading token response")?;
        anyhow::ensure!(status.is_success(), "token endpoint returned HTTP {status}");

        let tokens: serde_json::Value =
            serde_json::from_slice(&body).context("token response is not JSON")?;
        let access_token = tokens
            .get("access_token")
            .and_then(|v| v.as_str())
            .context("token response missing access_token")?
            .to_string();
        // expires_in is RECOMMENDED, not guaranteed — but fabricating a
        // lifetime the provider did not state would set a wrong expires_at
        // (and, for a value below the real lifetime, a perpetual
        // refresh/notification loop). A response without it fails the
        // rotation honestly: the admin is notified and the stored
        // credential is left untouched (review task-273).
        let expires_in = tokens
            .get("expires_in")
            .and_then(|v| v.as_u64())
            .context("token response missing expires_in (cannot determine new expiry)")?;

        // Overwrite ONLY the rotated tokens on the original document —
        // every other field the admin supplied (scope, issuer, provider
        // hints, …) is preserved verbatim, so the stored value remains a
        // complete credential document. RFC 6749 §6: the provider MAY
        // issue a new refresh token; when absent the ORIGINAL field is
        // left exactly as stored (review task-273).
        let rotated_refresh = tokens
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if let Some(new_refresh) = rotated_refresh {
            doc["refresh_token"] = serde_json::Value::String(new_refresh);
        }
        doc["access_token"] = serde_json::Value::String(access_token);
        let value = serde_json::to_vec(&doc).context("serializing rotated credential")?;
        Ok(RotatedSecret {
            value,
            expires_at: Some(now_secs().saturating_add(expires_in)),
        })
    }
}

static OAUTH_REFRESH: OAuthRefreshHandler = OAuthRefreshHandler;

// ── GCP service-account handler (admin-panel.md: Vertex AI for Claude Code
//    is the primary use case; TASK-273) ─────────────────────────────────

/// Google Cloud service-account credential handler: a secret whose value
/// is a GCP service-account JSON key (the Vertex AI primary use case) is
/// refreshed by exchanging a real RS256-signed JWT-bearer assertion at the
/// SA's `token_uri` (Google OAuth 2.0 JWT profile, RFC 7523) for a
/// short-lived access token. The refreshed document keeps the SA key and
/// gains the new `access_token` + expiry, so the next refresh re-signs from
/// the same key.
pub struct GcpServiceAccountHandler;

#[derive(serde::Serialize, serde::Deserialize)]
struct GcpSaDoc {
    /// "service_account" — shape discriminator.
    r#type: String,
    client_email: String,
    /// PEM PKCS#8 RSA private key ("-----BEGIN PRIVATE KEY-----…").
    private_key: String,
    /// Google token endpoint, e.g. https://oauth2.googleapis.com/token.
    token_uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    access_token: Option<String>,
}

fn parses_as_gcp_sa(value: &[u8]) -> bool {
    match serde_json::from_slice::<GcpSaDoc>(value) {
        Ok(d) => d.r#type == "service_account",
        Err(_) => false,
    }
}

/// Strip PEM armor and base64-decode to DER (PKCS#8).
fn pem_to_der(pem: &str) -> anyhow::Result<Vec<u8>> {
    let b64: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .context("service-account private_key is not valid base64")
}

fn b64url(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

#[async_trait::async_trait]
impl SecretRotationHandler for GcpServiceAccountHandler {
    fn name(&self) -> &'static str {
        "gcp_service_account_jwt_bearer"
    }

    fn handles(&self, _secret: &Secret, value: &[u8]) -> bool {
        parses_as_gcp_sa(value)
    }

    async fn rotate(
        &self,
        http: &reqwest::Client,
        _secret: &Secret,
        current_value: &[u8],
    ) -> anyhow::Result<RotatedSecret> {
        // Parse the document as a whole JSON object and only READ the
        // signing fields through the typed view: the admin-supplied SA key
        // carries fields Gyre does not use (project_id, private_key_id,
        // client_id, auth_uri, auth_provider_x509_cert_url,
        // client_x509_cert_url, …) that MUST survive the refresh — the
        // rotated document is re-serialized from the ORIGINAL value with
        // only `access_token` overwritten, never from a lossy typed copy.
        let mut doc: serde_json::Value = serde_json::from_slice(current_value).context(
            "value is not a GCP service-account key (JSON with type, client_email, private_key, token_uri)",
        )?;
        anyhow::ensure!(doc.is_object(), "service-account key must be a JSON object");
        let typed: GcpSaDoc = serde_json::from_value(doc.clone())
            .context("value is not a GCP service-account key (JSON with type, client_email, private_key, token_uri)")?;
        anyhow::ensure!(
            typed.r#type == "service_account",
            "credential document is not a service account"
        );

        // RFC 7523 JWT-bearer assertion: RS256 over the SHA-256 digest of
        // the header.payload, signed with the SA's private key (real
        // signing — ring parses the PKCS#8 PEM and signs on-device).
        let now = now_secs();
        let header = serde_json::json!({ "alg": "RS256", "typ": "JWT" });
        let claims = serde_json::json!({
            "iss": typed.client_email,
            "scope": "https://www.googleapis.com/auth/cloud-platform",
            "aud": typed.token_uri,
            "iat": now,
            "exp": now + 3600,
        });
        let signing_input = format!(
            "{}.{}",
            b64url(header.to_string().as_bytes()),
            b64url(claims.to_string().as_bytes())
        );

        let der = pem_to_der(&typed.private_key)?;
        let key_pair = ring::signature::RsaKeyPair::from_pkcs8(&der)
            .map_err(|e| anyhow::anyhow!("service-account private_key is unusable: {e:?}"))?;
        let mut sig = vec![0u8; key_pair.public().modulus_len()];
        let rng = ring::rand::SystemRandom::new();
        key_pair
            .sign(
                &ring::signature::RSA_PKCS1_SHA256,
                &rng,
                signing_input.as_bytes(),
                &mut sig,
            )
            .map_err(|e| anyhow::anyhow!("RS256 signing failed: {e:?}"))?;
        let assertion = format!("{signing_input}.{}", b64url(&sig));

        // JWT-bearer grant at the SA's token_uri. `http` is the AppState
        // client (30 s total timeout), so a hung Google endpoint cannot
        // stall the job loop.
        let resp = http
            .post(&typed.token_uri)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", assertion.as_str()),
            ])
            .send()
            .await
            .context("token endpoint request failed")?;
        let status = resp.status();
        let body = resp.bytes().await.context("reading token response")?;
        anyhow::ensure!(status.is_success(), "token endpoint returned HTTP {status}");

        let tokens: serde_json::Value =
            serde_json::from_slice(&body).context("token response is not JSON")?;
        let access_token = tokens
            .get("access_token")
            .and_then(|v| v.as_str())
            .context("token response missing access_token")?
            .to_string();
        // No fabricated lifetime: Google always states expires_in, and a
        // response without it cannot set an honest expires_at (review
        // task-273).
        let expires_in = tokens
            .get("expires_in")
            .and_then(|v| v.as_u64())
            .context("token response missing expires_in (cannot determine new expiry)")?;

        // Overwrite ONLY the access token on the original document — every
        // other field the admin supplied (Google-issued SA key fields
        // included) is preserved verbatim, so the stored value remains a
        // complete, re-refreshable SA key.
        doc["access_token"] = serde_json::Value::String(access_token);
        let value = serde_json::to_vec(&doc).context("serializing rotated credential")?;
        Ok(RotatedSecret {
            value,
            expires_at: Some(now_secs().saturating_add(expires_in)),
        })
    }
}

static GCP_SA: GcpServiceAccountHandler = GcpServiceAccountHandler;

/// Handlers consulted (in order) by the rotation job. The GCP SA handler
/// runs first: a service-account key (type: service_account) is the more
/// specific document shape; OAuth credential docs have neither
/// `client_email` nor `private_key` and never collide.
static ROTATION_HANDLERS: &[&dyn SecretRotationHandler] = &[&GCP_SA, &OAUTH_REFRESH];

/// The handler that can rotate `secret` (whose decrypted value is
/// `value`), if any.
fn handler_for(secret: &Secret, value: &[u8]) -> Option<&'static dyn SecretRotationHandler> {
    ROTATION_HANDLERS
        .iter()
        .copied()
        .find(|h| h.handles(secret, value))
}

/// Attempt automatic rotation of `secret` via its handler. On success the
/// stored value is replaced, `last_rotated_at` is set, `expires_at` is
/// extended past the expiry window, and a `secret.rotated` audit event is
/// logged (name, never value). Errors propagate to the caller, which falls
/// back to admin notification.
async fn attempt_rotation(
    state: &Arc<AppState>,
    handler: &dyn SecretRotationHandler,
    secret: &Secret,
    current_value: &[u8],
) -> anyhow::Result<()> {
    let rotated = handler
        .rotate(&state.http_client, secret, current_value)
        .await?;

    // Replace the value + metadata with ONE port operation. The port's
    // `rotate()` only touches value + `last_rotated_at`, so the extended
    // `expires_at` needs a row rewrite — but a delete + create pair would
    // lose the credential if the process died between the two steps (the
    // SA key or refresh token is unrecoverable), and a concurrent
    // resolve_for_agent in the window would see it missing. `replace` is
    // a single atomic rewrite with no such window (review task-273). The
    // new value is re-supplied from the handler, never read back out of
    // storage.
    let mut updated = secret.clone();
    updated.expires_at = rotated.expires_at;
    updated.last_rotated_at = Some(now_secs());
    state.secrets.replace(&updated, &rotated.value).await?;

    // §7 audit event: secret name (never value), scope, rotating actor.
    info!(
        event = "secret.rotated",
        secret_name = %secret.name,
        scope = secret.scope.as_str(),
        scope_id = %secret.scope_id,
        tenant_id = %secret.tenant_id,
        rotated_by = "secret_rotation_check",
        expires_at = rotated.expires_at,
        "secret.rotated: rotation handler refreshed expiring secret"
    );
    Ok(())
}

/// Per-secret outcome of the rotation dispatch (TASK-273: no expiring
/// credential is silently skipped — every outcome is distinguishable in
/// the run's durable logs).
enum RotateOutcome {
    /// A handler refreshed the credential (admin notification suppressed).
    Rotated,
    /// A handler exists but the credential is not yet near enough to
    /// expiry to justify a refresh (see [`should_refresh`]): the refresh
    /// will fire before expiry; no notification.
    Deferred,
    /// No handler (no-refresh-mechanism) or the refresh failed — the
    /// caller notifies the responsible admins.
    Notify,
}

/// Attempt handler rotation for one expiring secret. Returns the
/// [`RotateOutcome`] for the caller's notification decision; the
/// per-secret outcome (refreshed / deferred / no-refresh-mechanism /
/// failed: reason) is recorded in the run-scoped log sink (TASK-273: no
/// expiring credential is silently skipped).
async fn try_rotate(
    state: &Arc<AppState>,
    secret: &Secret,
    log: &crate::jobs::JobRunLog,
    now: u64,
) -> RotateOutcome {
    let value = match state.secrets.get_value(&secret.id, &secret.tenant_id).await {
        Ok(Some(v)) => v,
        Ok(None) => {
            log.log(format!(
                "secret '{}' ({}:{}): FAILED — value vanished before rotation",
                secret.name,
                secret.scope.as_str(),
                secret.scope_id
            ))
            .await;
            return RotateOutcome::Notify;
        }
        Err(e) => {
            log.log(format!(
                "secret '{}' ({}:{}): FAILED — value read failed: {e:#}",
                secret.name,
                secret.scope.as_str(),
                secret.scope_id
            ))
            .await;
            return RotateOutcome::Notify;
        }
    };

    let Some(handler) = handler_for(secret, &value) else {
        log.log(format!(
            "secret '{}' ({}:{}): no-refresh-mechanism — admin notified",
            secret.name,
            secret.scope.as_str(),
            secret.scope_id
        ))
        .await;
        return RotateOutcome::Notify;
    };
    if !should_refresh(secret, now) {
        // Within the scan window but not yet meaningfully near expiry: a
        // short-lived token (e.g. a 3600 s Google access token) is fresh
        // for most of its granted lifetime. Refreshing now would re-fire
        // on every job cycle; the deferred refresh lands before expiry,
        // so no admin notification is due either (review task-273:
        // perpetual refresh loop).
        log.log(format!(
            "secret '{}' ({}:{}): refresh deferred — credential still fresh \
             (expires_at {}, refresh fires near expiry)",
            secret.name,
            secret.scope.as_str(),
            secret.scope_id,
            secret.expires_at.unwrap_or(0)
        ))
        .await;
        return RotateOutcome::Deferred;
    }
    match attempt_rotation(state, handler, secret, &value).await {
        Ok(()) => {
            log.log(format!(
                "secret '{}' ({}:{}): refreshed via {}",
                secret.name,
                secret.scope.as_str(),
                secret.scope_id,
                handler.name()
            ))
            .await;
            RotateOutcome::Rotated
        }
        Err(e) => {
            warn!(
                secret_name = %secret.name,
                scope = secret.scope.as_str(),
                tenant_id = %secret.tenant_id,
                "secret_rotation: automatic rotation failed, falling back to admin notification: {e:#}"
            );
            log.log(format!(
                "secret '{}' ({}:{}): FAILED — refresh via {} failed: {e:#}",
                secret.name,
                secret.scope.as_str(),
                secret.scope_id,
                handler.name()
            ))
            .await;
            RotateOutcome::Notify
        }
    }
}

// ── Agent teardown revocation (platform-model.md §7) ────────────────────────

/// Revoke an agent's task-scoped secrets on teardown (agent completion or
/// admin kill): delete every Task-scoped secret for `task_id` from the
/// secrets table and log a `secret.revoked` audit event per secret (name,
/// never value).
///
/// The tenant is resolved from the agent's workspace record; an
/// unresolvable workspace skips revocation with a warn — a scope is never
/// fabricated.
pub async fn revoke_task_secrets(
    state: &AppState,
    workspace_id: &Id,
    task_id: &str,
    agent_id: &str,
) {
    let tenant_id = match state.workspaces.find_by_id(workspace_id).await {
        Ok(Some(ws)) => ws.tenant_id.to_string(),
        Ok(None) => {
            warn!(
                agent_id,
                task_id, "secret teardown: workspace unresolvable; skipping task-secret revocation"
            );
            return;
        }
        Err(e) => {
            warn!(
                agent_id,
                task_id, "secret teardown: workspace lookup failed: {e:#}"
            );
            return;
        }
    };

    let secrets = match state
        .secrets
        .list_by_scope(gyre_common::SecretScope::Task, task_id, &tenant_id)
        .await
    {
        Ok(s) => s,
        Err(e) => {
            warn!(
                agent_id,
                task_id, "secret teardown: task-scope list failed: {e:#}"
            );
            return;
        }
    };

    for secret in secrets {
        match state.secrets.delete(&secret.id, &tenant_id).await {
            Ok(()) => {
                // §7 audit event: secret name (never value), scope, agent_id.
                info!(
                    event = "secret.revoked",
                    agent_id,
                    secret_name = %secret.name,
                    scope = "task",
                    task_id,
                    tenant_id = %tenant_id,
                    "secret.revoked: task-scoped secret deleted on agent teardown"
                );
            }
            Err(e) => {
                warn!(
                    agent_id,
                    secret_name = %secret.name,
                    "secret teardown: delete failed: {e:#}"
                );
            }
        }
    }
}

/// Revoke the task-scoped ephemeral secrets of `agent`'s current task, if
/// any — the shared teardown step for every path that ends an agent run
/// (REST and MCP completion, admin kill, heartbeat abort, compose teardown,
/// repo-archive stop, process/container/SSH exit monitors). Agents without a
/// current task (e.g. orchestrators) have nothing to revoke.
pub async fn revoke_agent_task_secrets(state: &AppState, agent: &gyre_domain::Agent) {
    if let Some(task_id) = agent.current_task_id.as_ref() {
        revoke_task_secrets(
            state,
            &agent.workspace_id,
            &task_id.to_string(),
            &agent.id.to_string(),
        )
        .await;
    }
}

/// Run one pass of the secret expiry check across all tenants. Per-secret
/// outcomes (refreshed / no-refresh-mechanism / failed: reason) are written
/// to the run-scoped log sink, so the durable job-run record carries them.
/// A scope-list failure fails the whole run (fail-closed) instead of
/// degrading to an empty list — no expiring credential may be silently
/// skipped.
pub async fn run_once(state: &Arc<AppState>, log: &crate::jobs::JobRunLog) -> anyhow::Result<()> {
    let now = now_secs();
    let tenants = state.tenants.list().await?;
    debug!(tenants = tenants.len(), "secret_rotation: scanning tenants");

    for tenant in &tenants {
        let tenant_id = tenant.id.to_string();

        // Tenant-scoped secrets notify the admins of every workspace in the
        // tenant (the secret applies to all of them).
        let workspaces = state.workspaces.list_by_tenant(&tenant.id).await?;
        let tenant_secrets = state
            .secrets
            .list_by_scope(gyre_common::SecretScope::Tenant, &tenant_id, &tenant_id)
            .await
            .context("tenant-scope secret list failed")?;
        for secret in expiring(&tenant_secrets, now) {
            // Rotated secrets with a handler are refreshed automatically;
            // deferred refreshes will fire before expiry; only
            // no-handler / failed-rotation secrets notify admins.
            if !matches!(
                try_rotate(state, secret, log, now).await,
                RotateOutcome::Notify
            ) {
                continue;
            }
            for ws in &workspaces {
                notify_workspace_admins(state, ws.id.clone(), secret, now).await;
            }
        }

        for ws in &workspaces {
            let ws_id = ws.id.to_string();
            // Workspace-scoped secrets. A scope-list failure fails the run
            // (recorded as a failed run with the reason): degrading to an
            // empty list would complete the run "green" while silently
            // skipping every expiring credential in the scope — the exact
            // silent skip the run record exists to prevent (review
            // task-273).
            let ws_secrets = state
                .secrets
                .list_by_scope(gyre_common::SecretScope::Workspace, &ws_id, &tenant_id)
                .await
                .with_context(|| format!("workspace-scope secret list failed for {ws_id}"))?;
            for secret in expiring(&ws_secrets, now) {
                // Rotated secrets with a handler are refreshed
                // automatically; deferred refreshes will fire before
                // expiry; only no-handler / failed-rotation secrets
                // notify admins.
                if !matches!(
                    try_rotate(state, secret, log, now).await,
                    RotateOutcome::Notify
                ) {
                    continue;
                }
                notify_workspace_admins(state, ws.id.clone(), secret, now).await;
            }

            // Repo-scoped secrets.
            let repos = state.repos.list_by_workspace(&ws.id).await?;
            for repo in &repos {
                let repo_id = repo.id.to_string();
                let repo_secrets = state
                    .secrets
                    .list_by_scope(gyre_common::SecretScope::Repo, &repo_id, &tenant_id)
                    .await
                    .with_context(|| format!("repo-scope secret list failed for {repo_id}"))?;
                for secret in expiring(&repo_secrets, now) {
                    // Rotated secrets with a handler are refreshed
                    // automatically; deferred refreshes will fire before
                    // expiry; only no-handler / failed-rotation secrets
                    // notify admins.
                    if !matches!(
                        try_rotate(state, secret, log, now).await,
                        RotateOutcome::Notify
                    ) {
                        continue;
                    }
                    notify_workspace_admins(state, ws.id.clone(), secret, now).await;
                }
            }
        }
    }
    Ok(())
}

/// Filter to secrets whose expiry is within [`EXPIRY_WINDOW_SECS`] of `now`
/// (already-expired secrets included — an admin must know either way).
fn expiring(secrets: &[gyre_common::Secret], now: u64) -> Vec<&gyre_common::Secret> {
    secrets
        .iter()
        .filter(|s| {
            s.expires_at
                .map(|e| e <= now + EXPIRY_WINDOW_SECS)
                .unwrap_or(false)
        })
        .collect()
}

/// Create a SecretExpiring notification for each Admin/Owner member of the
/// workspace, suppressing duplicates (active notification exists or a recent
/// dismissal).
///
/// `workspace_id` is the workspace that OWNS the secret (resolved by the
/// caller from the secret's own scope), and every notification is created
/// for a member of that same workspace — not a caller-propagated scope.
async fn notify_workspace_admins(
    state: &Arc<AppState>,
    workspace_id: Id,
    secret: &gyre_common::Secret,
    now: u64,
) {
    // caller-scope:ok — workspace_id is the secret's own governing workspace;
    // members and notifications are resolved from it directly by design.
    let members = match state
        .workspace_memberships
        .list_by_workspace(&workspace_id)
        .await
    {
        Ok(m) => m,
        Err(e) => {
            warn!(
                workspace_id = %workspace_id,
                "secret_rotation: member list failed: {e:#}"
            );
            return;
        }
    };

    for member in &members {
        if !matches!(
            member.role,
            gyre_domain::WorkspaceRole::Admin | gyre_domain::WorkspaceRole::Owner
        ) {
            continue;
        }

        // Suppress while an active SecretExpiring for this secret exists, or
        // for 24 h after the user dismissed one for THIS secret (a dismissal
        // for a different secret must not suppress this one).
        let existing = state
            .notifications
            .list_for_user(
                &member.user_id,
                Some(&workspace_id),
                None,
                None,
                Some(&[NotificationType::SecretExpiring.as_str()]),
                None,
                &[],
                50,
                0,
            )
            .await
            .unwrap_or_default();
        let dismissal_cutoff = now as i64 - 86_400;
        let suppressed = existing.iter().any(|n| {
            n.source.entity_id == secret.id.as_str()
                && (n.is_active()
                    || n.dismissed_at
                        .map(|d| d >= dismissal_cutoff)
                        .unwrap_or(false))
        });
        if suppressed {
            continue;
        }

        let status = if secret.is_expired(now) {
            "expired"
        } else {
            "expiring"
        };
        let notif = Notification::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            workspace_id.clone(),
            member.user_id.clone(),
            NotificationType::SecretExpiring,
            format!(
                "Secret '{}' ({} scope) is {}",
                secret.name,
                secret.scope.as_str(),
                status
            ),
            secret.tenant_id.clone(),
            now as i64,
        );
        let mut notif = notif;
        notif.body = Some(
            serde_json::json!({
                "secret_id": secret.id.to_string(),
                "name": secret.name,
                "scope": secret.scope.as_str(),
                "scope_id": secret.scope_id,
                "expires_at": secret.expires_at,
                "status": status,
            })
            .to_string(),
        );
        // Source the notification to the secret itself; a repo-scoped
        // secret carries its repo so the source is scoped like the entity.
        notif.source.entity_type = "secret".to_string();
        notif.source.entity_id = secret.id.to_string();
        if secret.scope == gyre_common::SecretScope::Repo {
            notif.source.repo_id = Some(Id::new(secret.scope_id.clone()));
        }

        if let Err(e) = state.create_notification(&notif).await {
            warn!(
                workspace_id = %workspace_id,
                user_id = %member.user_id,
                "secret_rotation: notification create failed: {e:#}"
            );
        } else {
            // Channel fan-out per user-management.md §Delivery Channels.
            crate::notification_dispatcher::dispatch_to_channels(state, &notif).await;
            info!(
                workspace_id = %workspace_id,
                user_id = %member.user_id,
                secret = %secret.name,
                status,
                "secret_rotation: expiry notification created"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_domain::{Workspace, WorkspaceMembership, WorkspaceRole};

    /// Seed: tenant "default", workspace with an Admin member, a Developer
    /// member, and secrets with controlled expiry. The tenant must be
    /// created explicitly — the mem tenant store starts empty, and
    /// `run_once` walks `tenants.list()`, so without it the scan finds
    /// nothing (the seeded workspace would be orphaned from the scan).
    async fn seed(now: u64) -> Arc<AppState> {
        let state = crate::mem::test_state();

        state
            .tenants
            .create(&gyre_domain::Tenant::new(
                Id::new("default"),
                "Default",
                "default",
                now,
            ))
            .await
            .unwrap();

        let ws = Workspace::new(
            Id::new("ws-rot"),
            Id::new("default"),
            "Rotation WS",
            "rotation-ws",
            now,
        );
        state.workspaces.create(&ws).await.unwrap();

        // Admin member + developer member.
        for (uid, role) in [
            ("user-admin", WorkspaceRole::Admin),
            ("user-dev", WorkspaceRole::Developer),
        ] {
            let m = WorkspaceMembership::new(
                Id::new(format!("mem-{uid}")),
                Id::new(uid),
                ws.id.clone(),
                role,
                Id::new("inviter"),
                now,
            );
            state.workspace_memberships.create(&m).await.unwrap();
        }

        let mk = |id: &str,
                  name: &str,
                  scope: gyre_common::SecretScope,
                  scope_id: &str,
                  expires_at: Option<u64>| gyre_common::Secret {
            id: Id::new(id),
            name: name.to_string(),
            scope,
            scope_id: scope_id.to_string(),
            secret_type: gyre_common::SecretType::Rotated,
            created_by: "system".to_string(),
            created_at: now,
            expires_at,
            last_rotated_at: None,
            tenant_id: "default".to_string(),
        };
        // Expiring in 30 minutes → notified.
        state
            .secrets
            .create(
                &mk(
                    "sec-exp",
                    "OAUTH_TOKEN",
                    gyre_common::SecretScope::Workspace,
                    "ws-rot",
                    Some(now + 1800),
                ),
                b"v",
            )
            .await
            .unwrap();
        // No expiry → never notified.
        state
            .secrets
            .create(
                &mk(
                    "sec-static",
                    "DATABASE_URL",
                    gyre_common::SecretScope::Workspace,
                    "ws-rot",
                    None,
                ),
                b"v",
            )
            .await
            .unwrap();
        // Expires in 2 hours → outside the window, not notified.
        state
            .secrets
            .create(
                &mk(
                    "sec-later",
                    "LATER_KEY",
                    gyre_common::SecretScope::Workspace,
                    "ws-rot",
                    Some(now + 7200),
                ),
                b"v",
            )
            .await
            .unwrap();
        // Already expired → notified (status "expired").
        state
            .secrets
            .create(
                &mk(
                    "sec-past",
                    "PAST_KEY",
                    gyre_common::SecretScope::Workspace,
                    "ws-rot",
                    Some(now - 60),
                ),
                b"v",
            )
            .await
            .unwrap();
        state
    }

    #[tokio::test]
    async fn expiry_notification_fans_out_to_configured_channels() {
        // Emitter-path guard: the secret-expiry scan's notification creation
        // must go through the channel dispatcher — a configured email
        // channel queues an outbox entry for the notified Admin.
        let now = now_secs();
        let state = seed(now).await;
        state
            .user_channel_prefs
            .upsert(
                &Id::new("user-admin"),
                &gyre_domain::NotificationChannels {
                    in_app: true,
                    email: gyre_domain::EmailConfig {
                        enabled: true,
                        digest: gyre_domain::DigestFrequency::Immediate,
                        min_priority: gyre_domain::NotificationPriority::Low,
                        type_overrides: Default::default(),
                    },
                    webhook: None,
                    slack: None,
                },
            )
            .await
            .unwrap();

        run_once(&state, &crate::jobs::JobRunLog::new())
            .await
            .unwrap();

        let admin_notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-admin"),
                None,
                None,
                None,
                None,
                None,
                &[],
                50,
                0,
            )
            .await
            .unwrap();
        assert!(
            admin_notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::SecretExpiring),
            "expected in-app SecretExpiring notification"
        );
        let outbox = state
            .kv_store
            .kv_list(crate::notification_dispatcher::EMAIL_OUTBOX_NS)
            .await
            .unwrap();
        assert!(
            outbox.iter().any(|(k, _)| k.starts_with("user-admin:")),
            "secret-expiry notification must reach the configured email channel"
        );
    }

    #[tokio::test]
    async fn detects_expiring_secrets_and_notifies_admins_only() {
        // Fixed "now" so the seeded expiries are deterministic. run_once uses
        // real wall-clock now, so seed relative to real now instead.
        let now = now_secs();
        let state = seed(now).await;
        run_once(&state, &crate::jobs::JobRunLog::new())
            .await
            .unwrap();

        let admin_notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-admin"),
                None,
                None,
                None,
                None,
                None,
                &[],
                50,
                0,
            )
            .await
            .unwrap();
        let secret_notifs: Vec<&Notification> = admin_notifs
            .iter()
            .filter(|n| n.notification_type == NotificationType::SecretExpiring)
            .collect();
        assert_eq!(
            secret_notifs.len(),
            2,
            "expiring + expired secrets must each notify, got: {admin_notifs:#?}"
        );
        let names: Vec<String> = secret_notifs
            .iter()
            .map(|n| {
                let body: serde_json::Value =
                    serde_json::from_str(n.body.as_deref().unwrap()).unwrap();
                body["name"].as_str().unwrap().to_string()
            })
            .collect();
        assert!(names.contains(&"OAUTH_TOKEN".to_string()));
        assert!(names.contains(&"PAST_KEY".to_string()));

        // Developer member gets nothing.
        let dev_notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-dev"),
                None,
                None,
                None,
                None,
                None,
                &[],
                50,
                0,
            )
            .await
            .unwrap();
        assert!(dev_notifs
            .iter()
            .all(|n| n.notification_type != NotificationType::SecretExpiring));
    }

    #[tokio::test]
    async fn does_not_duplicate_notifications_on_rerun() {
        let now = now_secs();
        let state = seed(now).await;
        run_once(&state, &crate::jobs::JobRunLog::new())
            .await
            .unwrap();
        run_once(&state, &crate::jobs::JobRunLog::new())
            .await
            .unwrap();

        let admin_notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-admin"),
                None,
                None,
                None,
                Some(&["SecretExpiring"]),
                None,
                &[],
                50,
                0,
            )
            .await
            .unwrap();
        assert_eq!(admin_notifs.len(), 2, "second pass must not re-notify");
    }

    #[tokio::test]
    async fn healthy_secrets_never_notify() {
        let now = now_secs();
        let state = crate::mem::test_state();
        let ws = Workspace::new(
            Id::new("ws-healthy"),
            Id::new("default"),
            "Healthy WS",
            "healthy-ws",
            now,
        );
        state.workspaces.create(&ws).await.unwrap();
        let m = WorkspaceMembership::new(
            Id::new("mem-h"),
            Id::new("user-h-admin"),
            ws.id,
            WorkspaceRole::Admin,
            Id::new("inviter"),
            now,
        );
        state.workspace_memberships.create(&m).await.unwrap();
        state
            .secrets
            .create(
                &gyre_common::Secret {
                    id: Id::new("sec-healthy"),
                    name: "FINE_KEY".to_string(),
                    scope: gyre_common::SecretScope::Workspace,
                    scope_id: "ws-healthy".to_string(),
                    secret_type: gyre_common::SecretType::Static,
                    created_by: "system".to_string(),
                    created_at: now,
                    expires_at: Some(now + 30 * 86400),
                    last_rotated_at: None,
                    tenant_id: "default".to_string(),
                },
                b"v",
            )
            .await
            .unwrap();

        run_once(&state, &crate::jobs::JobRunLog::new())
            .await
            .unwrap();
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-h-admin"),
                None,
                None,
                None,
                Some(&["SecretExpiring"]),
                None,
                &[],
                50,
                0,
            )
            .await
            .unwrap();
        assert!(notifs.is_empty(), "healthy secret must not notify");
    }

    // ── Rotation handler (task-100) ───────────────────────────────────────────

    /// Minimal seed for handler tests: tenant + workspace + Admin member + a
    /// single expiring `oauth_refresh_token` secret with the given value.
    async fn seed_oauth(now: u64, value: &[u8]) -> Arc<AppState> {
        let state = crate::mem::test_state();
        state
            .tenants
            .create(&gyre_domain::Tenant::new(
                Id::new("default"),
                "Default",
                "default",
                now,
            ))
            .await
            .unwrap();
        let ws = Workspace::new(
            Id::new("ws-rot"),
            Id::new("default"),
            "Rotation WS",
            "rotation-ws",
            now,
        );
        state.workspaces.create(&ws).await.unwrap();
        let m = WorkspaceMembership::new(
            Id::new("mem-oauth-admin"),
            Id::new("user-admin"),
            ws.id,
            WorkspaceRole::Admin,
            Id::new("inviter"),
            now,
        );
        state.workspace_memberships.create(&m).await.unwrap();
        state
            .secrets
            .create(
                &gyre_common::Secret {
                    id: Id::new("sec-oauth"),
                    name: "oauth_refresh_token".to_string(),
                    scope: gyre_common::SecretScope::Workspace,
                    scope_id: "ws-rot".to_string(),
                    secret_type: gyre_common::SecretType::Rotated,
                    created_by: "system".to_string(),
                    created_at: now,
                    // Expiring in 30 minutes → inside the check window.
                    expires_at: Some(now + 1800),
                    last_rotated_at: None,
                    tenant_id: "default".to_string(),
                },
                value,
            )
            .await
            .unwrap();
        state
    }

    /// Spawn a local OAuth token endpoint returning `response` with
    /// `status`, capturing the received form body. Returns the endpoint URL.
    async fn spawn_token_endpoint(
        status: axum::http::StatusCode,
        response: serde_json::Value,
    ) -> (String, Arc<tokio::sync::Mutex<String>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(tokio::sync::Mutex::new(String::new()));
        let seen_clone = Arc::clone(&seen);
        let app = axum::Router::new().route(
            "/token",
            axum::routing::post(move |body: String| {
                let seen = Arc::clone(&seen_clone);
                async move {
                    *seen.lock().await = body;
                    (status, axum::Json(response))
                }
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://127.0.0.1:{port}/token"), seen)
    }

    #[tokio::test]
    async fn rotation_handler_refreshes_expiring_oauth_secret() {
        let now = now_secs();
        let (endpoint, seen) = spawn_token_endpoint(
            axum::http::StatusCode::OK,
            serde_json::json!({
                "access_token": "new-access-token",
                "expires_in": 7200,
                "refresh_token": "new-refresh-token"
            }),
        )
        .await;
        let cred = serde_json::json!({
            "token_endpoint": endpoint,
            "client_id": "gyre-test",
            "client_secret": "test-client-secret",
            "refresh_token": "old-refresh-token",
            // review task-273: fields Gyre does not read must survive the
            // rotation — the rotated document is re-serialized from the
            // ORIGINAL value, never from a lossy typed copy.
            "scope": "openid profile email",
            "issuer": "https://sso.example.com",
            "provider_hint": "claude-max"
        });
        let state = seed_oauth(now, cred.to_string().as_bytes()).await;

        run_once(&state, &crate::jobs::JobRunLog::new())
            .await
            .unwrap();

        // The handler performed a real refresh_token grant with the stored
        // credential (old refresh token + client id posted to the endpoint).
        let posted = seen.lock().await.clone();
        assert!(
            posted.contains("grant_type=refresh_token"),
            "grant_type must be refresh_token, posted: {posted}"
        );
        assert!(
            posted.contains("refresh_token=old-refresh-token"),
            "the stored refresh token must be used, posted: {posted}"
        );
        assert!(
            posted.contains("client_id=gyre-test"),
            "client_id must be posted, posted: {posted}"
        );

        // The stored value is the rotated credential document: new tokens,
        // untouched config preserved.
        let value = state
            .secrets
            .get_value(&Id::new("sec-oauth"), "default")
            .await
            .unwrap()
            .expect("rotated secret must still exist");
        let doc: serde_json::Value = serde_json::from_slice(&value).unwrap();
        assert_eq!(doc["refresh_token"], "new-refresh-token");
        assert_eq!(doc["access_token"], "new-access-token");
        assert_eq!(doc["client_id"], "gyre-test");
        // review task-273: every extra field the admin supplied survives
        // the rotation verbatim (a typed re-serialization dropped them).
        assert_eq!(doc["scope"], "openid profile email");
        assert_eq!(doc["issuer"], "https://sso.example.com");
        assert_eq!(doc["provider_hint"], "claude-max");
        assert_eq!(doc["token_endpoint"], endpoint);

        // Expiry extended beyond the 1-hour window and last_rotated_at set.
        let stored = state
            .secrets
            .find_by_id(&Id::new("sec-oauth"), "default")
            .await
            .unwrap()
            .unwrap();
        assert!(
            stored.expires_at.unwrap() > now + 3600,
            "rotation must extend expiry past the check window, got {:?}",
            stored.expires_at
        );
        assert!(stored.last_rotated_at.is_some());

        // Successful rotation suppresses the admin notification.
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-admin"),
                None,
                None,
                None,
                Some(&["SecretExpiring"]),
                None,
                &[],
                50,
                0,
            )
            .await
            .unwrap();
        assert!(
            notifs.is_empty(),
            "a handler-rotated secret must not notify admins"
        );
    }

    #[tokio::test]
    async fn oauth_refresh_without_rotated_refresh_token_preserves_original_document() {
        // review task-273: RFC 6749 §6 — the provider MAY issue a new
        // refresh token. When it does not, the ORIGINAL refresh_token
        // field (and every other admin-supplied field) must survive the
        // rotation verbatim; nothing may be dropped or rewritten.
        let (endpoint, _seen) = spawn_token_endpoint(
            axum::http::StatusCode::OK,
            // No refresh_token in the response — the old one remains
            // valid per RFC 6749 §6.
            serde_json::json!({ "access_token": "at-2", "expires_in": 3600 }),
        )
        .await;
        let cred = serde_json::json!({
            "token_endpoint": endpoint,
            "client_id": "gyre-test",
            "refresh_token": "original-refresh-token",
            "scope": "openid"
        });
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();
        let secret = gyre_common::Secret {
            id: Id::new("sec-direct"),
            name: "PROD_API_CREDENTIALS".to_string(),
            scope: gyre_common::SecretScope::Workspace,
            scope_id: "ws-rot".to_string(),
            secret_type: gyre_common::SecretType::Rotated,
            created_by: "system".to_string(),
            created_at: 0,
            expires_at: None,
            last_rotated_at: None,
            tenant_id: "default".to_string(),
        };
        let rotated = OAUTH_REFRESH
            .rotate(&http, &secret, cred.to_string().as_bytes())
            .await
            .unwrap();
        let doc: serde_json::Value = serde_json::from_slice(&rotated.value).unwrap();
        assert_eq!(
            doc["refresh_token"], "original-refresh-token",
            "the original refresh token must be preserved when the provider issues none"
        );
        assert_eq!(doc["access_token"], "at-2");
        assert_eq!(doc["scope"], "openid");
        assert_eq!(doc["token_endpoint"], endpoint);
        assert!(rotated.expires_at.is_some());
    }

    #[tokio::test]
    async fn rotation_handler_failure_falls_back_to_admin_notification() {
        let now = now_secs();
        let (endpoint, _seen) = spawn_token_endpoint(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({"error": "invalid_grant"}),
        )
        .await;
        let cred = serde_json::json!({
            "token_endpoint": endpoint,
            "client_id": "gyre-test",
            "refresh_token": "old-refresh-token"
        });
        let original = cred.to_string();
        let state = seed_oauth(now, original.as_bytes()).await;

        run_once(&state, &crate::jobs::JobRunLog::new())
            .await
            .unwrap();

        // Value untouched: the failed grant must not corrupt the credential.
        let value = state
            .secrets
            .get_value(&Id::new("sec-oauth"), "default")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(value, original.as_bytes());

        // The admin is notified — a failed rotation is exactly what an
        // admin must be warned about.
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-admin"),
                None,
                None,
                None,
                Some(&["SecretExpiring"]),
                None,
                &[],
                50,
                0,
            )
            .await
            .unwrap();
        assert_eq!(notifs.len(), 1, "failed rotation must notify admins");
    }

    #[tokio::test]
    async fn malformed_handler_value_falls_back_to_admin_notification() {
        let now = now_secs();
        let state = seed_oauth(now, b"not-a-json-credential").await;

        run_once(&state, &crate::jobs::JobRunLog::new())
            .await
            .unwrap();

        let value = state
            .secrets
            .get_value(&Id::new("sec-oauth"), "default")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(value, b"not-a-json-credential");
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-admin"),
                None,
                None,
                None,
                Some(&["SecretExpiring"]),
                None,
                &[],
                50,
                0,
            )
            .await
            .unwrap();
        assert_eq!(
            notifs.len(),
            1,
            "unparseable credential must fall back to admin notification"
        );
    }

    // ── Task-scoped teardown revocation (task-100) ───────────────────────────

    #[tokio::test]
    async fn revoke_task_secrets_deletes_only_that_task_scope() {
        let now = now_secs();
        let state = crate::mem::test_state();
        state
            .tenants
            .create(&gyre_domain::Tenant::new(
                Id::new("default"),
                "Default",
                "default",
                now,
            ))
            .await
            .unwrap();
        let ws = Workspace::new(
            Id::new("ws-teardown"),
            Id::new("default"),
            "Teardown WS",
            "teardown-ws",
            now,
        );
        state.workspaces.create(&ws).await.unwrap();

        let mk = |id: &str, name: &str, scope: gyre_common::SecretScope, scope_id: &str| {
            gyre_common::Secret {
                id: Id::new(id),
                name: name.to_string(),
                scope,
                scope_id: scope_id.to_string(),
                secret_type: gyre_common::SecretType::Ephemeral,
                created_by: "system".to_string(),
                created_at: now,
                expires_at: None,
                last_rotated_at: None,
                tenant_id: "default".to_string(),
            }
        };
        // Task-scoped secret for the completing agent's task (revoked), a
        // task-scoped secret for a different task (kept), and a
        // workspace-scoped secret (kept).
        state
            .secrets
            .create(
                &mk(
                    "sec-t1",
                    "EPHEMERAL_DB",
                    gyre_common::SecretScope::Task,
                    "task-1",
                ),
                b"v1",
            )
            .await
            .unwrap();
        state
            .secrets
            .create(
                &mk(
                    "sec-t2",
                    "EPHEMERAL_DB",
                    gyre_common::SecretScope::Task,
                    "task-2",
                ),
                b"v2",
            )
            .await
            .unwrap();
        state
            .secrets
            .create(
                &mk(
                    "sec-ws",
                    "SHARED_KEY",
                    gyre_common::SecretScope::Workspace,
                    "ws-teardown",
                ),
                b"v3",
            )
            .await
            .unwrap();

        revoke_task_secrets(&state, &Id::new("ws-teardown"), "task-1", "agent-1").await;

        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-t1"), "default")
                .await
                .unwrap()
                .is_none(),
            "the completed task's secret must be revoked"
        );
        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-t2"), "default")
                .await
                .unwrap()
                .is_some(),
            "another task's secret must survive"
        );
        assert!(
            state
                .secrets
                .find_by_id(&Id::new("sec-ws"), "default")
                .await
                .unwrap()
                .is_some(),
            "workspace-scoped secrets must survive task teardown"
        );
    }

    #[tokio::test]
    async fn revoke_task_secrets_skips_unresolvable_workspace() {
        // No workspace record exists for this id: revocation must skip
        // (warn) rather than fabricate a tenant scope or panic.
        let state = crate::mem::test_state();
        revoke_task_secrets(&state, &Id::new("ws-ghost"), "task-1", "agent-1").await;
    }

    // ── TASK-273: shape-based dispatch + GCP service-account refresh ─────────

    /// Seed a tenant/workspace/admin member and ONE expiring secret with the
    /// given id, NAME (arbitrary — dispatch is shape-based, not name-based)
    /// and plaintext value.
    async fn seed_named_secret(now: u64, id: &str, name: &str, value: &[u8]) -> Arc<AppState> {
        let state = crate::mem::test_state();
        state
            .tenants
            .create(&gyre_domain::Tenant::new(
                Id::new("default"),
                "Default",
                "default",
                now,
            ))
            .await
            .unwrap();
        let ws = Workspace::new(
            Id::new("ws-rot"),
            Id::new("default"),
            "Rotation WS",
            "rotation-ws",
            now,
        );
        state.workspaces.create(&ws).await.unwrap();
        let m = WorkspaceMembership::new(
            Id::new("mem-shape-admin"),
            Id::new("user-admin"),
            ws.id,
            WorkspaceRole::Admin,
            Id::new("inviter"),
            now,
        );
        state.workspace_memberships.create(&m).await.unwrap();
        state
            .secrets
            .create(
                &gyre_common::Secret {
                    id: Id::new(id),
                    name: name.to_string(),
                    scope: gyre_common::SecretScope::Workspace,
                    scope_id: "ws-rot".to_string(),
                    secret_type: gyre_common::SecretType::Rotated,
                    created_by: "system".to_string(),
                    created_at: now,
                    // Expiring in 30 minutes → inside the check window.
                    expires_at: Some(now + 1800),
                    last_rotated_at: None,
                    tenant_id: "default".to_string(),
                },
                value,
            )
            .await
            .unwrap();
        state
    }

    /// Minimal percent-decoding for reading captured `application/x-www-
    /// form-urlencoded` bodies in tests (reqwest percent-encodes `:` etc.).
    fn form_get(body: &str, key: &str) -> Option<String> {
        fn percent_decode(s: &str) -> Option<String> {
            let bytes = s.as_bytes();
            let mut out = Vec::with_capacity(bytes.len());
            let mut i = 0;
            while i < bytes.len() {
                match bytes[i] {
                    b'%' => {
                        let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
                        out.push(u8::from_str_radix(hex, 16).ok()?);
                        i += 3;
                    }
                    b'+' => {
                        out.push(b' ');
                        i += 1;
                    }
                    b => {
                        out.push(b);
                        i += 1;
                    }
                }
            }
            String::from_utf8(out).ok()
        }
        for pair in body.split('&') {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            if k == key {
                return percent_decode(v);
            }
        }
        None
    }

    #[tokio::test]
    async fn oauth_document_under_arbitrary_name_is_refreshed_by_shape() {
        // TASK-273: dispatch matches the credential-document SHAPE, not the
        // secret name — an OAuth credential document stored under a name
        // with no `_refresh_token` suffix is refreshed by the RFC 6749
        // handler all the same.
        let now = now_secs();
        let (endpoint, seen) = spawn_token_endpoint(
            axum::http::StatusCode::OK,
            serde_json::json!({
                "access_token": "rotated-access",
                "expires_in": 7200,
                "refresh_token": "rotated-refresh"
            }),
        )
        .await;
        let cred = serde_json::json!({
            "token_endpoint": endpoint,
            "client_id": "gyre-test",
            "refresh_token": "original-refresh"
        });
        let state = seed_named_secret(
            now,
            "sec-api-cred",
            // Deliberately NOT a *_refresh_token name: an API-credential
            // secret carrying an OAuth document.
            "PROD_API_CREDENTIALS",
            cred.to_string().as_bytes(),
        )
        .await;

        let log = crate::jobs::JobRunLog::new();
        run_once(&state, &log).await.unwrap();

        // A real refresh_token grant was performed against the document's
        // token endpoint using the stored refresh token.
        let posted = seen.lock().await.clone();
        assert!(
            posted.contains("grant_type=refresh_token"),
            "shape-matched OAuth doc must be refreshed via the RFC 6749 grant, posted: {posted}"
        );
        assert!(posted.contains("refresh_token=original-refresh"));

        // The stored value is the rotated document and the expiry was
        // extended past the check window.
        let value = state
            .secrets
            .get_value(&Id::new("sec-api-cred"), "default")
            .await
            .unwrap()
            .expect("secret must survive rotation");
        let doc: serde_json::Value = serde_json::from_slice(&value).unwrap();
        assert_eq!(doc["refresh_token"], "rotated-refresh");
        assert_eq!(doc["access_token"], "rotated-access");
        let stored = state
            .secrets
            .find_by_id(&Id::new("sec-api-cred"), "default")
            .await
            .unwrap()
            .unwrap();
        assert!(stored.expires_at.unwrap() > now + 3600);

        // The durable run log records the per-secret refreshed outcome.
        let lines = log.take().await;
        assert!(
            lines
                .iter()
                .any(|l| l.contains("refreshed via oauth_refresh_token_grant")),
            "run log must record the refresh outcome, got: {lines:?}"
        );

        // Successful rotation suppresses the admin notification.
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-admin"),
                None,
                None,
                None,
                Some(&["SecretExpiring"]),
                None,
                &[],
                50,
                0,
            )
            .await
            .unwrap();
        assert!(notifs.is_empty());
    }

    #[tokio::test]
    async fn no_refresh_mechanism_outcome_recorded_in_run_logs() {
        // TASK-273: an expiring API key with no refresh mechanism must BOTH
        // notify admins AND record a no-refresh-mechanism outcome in the
        // run's durable log — no expiring credential is silently skipped.
        let now = now_secs();
        let state =
            seed_named_secret(now, "sec-api-key", "THIRD_PARTY_API_KEY", b"sk-live-plain").await;

        let log = crate::jobs::JobRunLog::new();
        run_once(&state, &log).await.unwrap();

        let lines = log.take().await;
        assert!(
            lines
                .iter()
                .any(|l| l.contains("THIRD_PARTY_API_KEY") && l.contains("no-refresh-mechanism")),
            "run log must account for the no-refresh-mechanism secret, got: {lines:?}"
        );

        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-admin"),
                None,
                None,
                None,
                Some(&["SecretExpiring"]),
                None,
                &[],
                50,
                0,
            )
            .await
            .unwrap();
        assert_eq!(notifs.len(), 1, "admins must still be notified");
    }

    /// Throwaway 2048-bit RSA test key, as base64 of its PKCS#8 DER
    /// (generated solely for this test suite — never a real credential).
    /// The test rebuilds the PEM armor at runtime so the handler's real
    /// `pem_to_der` path is exercised.
    const TEST_SA_PRIVATE_KEY_DER_B64: &str = concat!(
        "MIIEpAIBAAKCAQEAoptEfzjxiQ1VJoEzYLvdYM9dJzSwd25dMxqbBYzjfRGgNUZPIeHe2hx66w+l",
        "cmGpY1zvzuPGCXKcvmFAiYxDo594gyGaF6ZgR5tX/d1V+DDPmCnq2fxdPimHfXTixR0hIbmkNDqc",
        "vAxbhKJFcx0aSWYkNY2fyHVyHCLaw4+IT+QztX3Zgh+45AsoL3LGAAlEbrF6hjJFHku4cEpE13Hh",
        "A3XxEruQ4RtdnUH96uS3HKN0Zt3XF7ENbn8/JokSf3qfie5jLDCxN8521unaPP5wsNi/ADYKo8Ur",
        "v1hG+0NZC57bgNBuZoUSmm3enK5H6qKdUJSkArkNjwf5hSlQpJCu9wIDAQABAoIBAEACKtaBzeKL",
        "UweTOGxNx3lCZ4L1ZLm+wgVJ5dgre6lR9OTDxoap6RWmvGQnhpMeCpXIkcW5N82kRjtTARqkEkQo",
        "lEHaSlQg9Cl75wlRxXOrkyxs47vncbT0mBc0mcfKDa6+6vp8UBJeMvBlCD2mKSDXik1icJFyWfbl",
        "aYQtzhTUOGqVlR+1PyuVu7wYW+HVE/wSl38q209uHtHlEeZgIU1kK+8ZG2ZL7+ZafB+I6yWzxULU",
        "pvg3+9kuWCd2gGV916NcRkv7g5h/Bcy23p8wUdmZYn6YfOopEfx/2OjuDYbJmQC4Z6PG8I8q1AhX",
        "HCzY02tOona1ld48tFxWjfnETsECgYEA4cRuNQ88tln9OnGdKo6CRGxp8tdO7z8/zqXvOsLW/rGK",
        "NNYgserjDs7zRKzjhhb8JcwVw9QngHCrQfsXyx9R9SDaJ8p88bsH9/lxxid8DqF3EYKFqkosEXMR",
        "EBAWn7p91tueKaABQLqxTiFXdpRLzFl6Zr5WCShrzs5K5R5xFzECgYEAuGGcUTg3gfIPmj+/qD4Y",
        "dPPxzCLY/Zxtb7v8474YBPoLtvb+Mp7m5UJiyJ+ACyYX7Sb/eAQPCaLFOtCZOzCfxez1uzXc9Bmy",
        "DP0O2+DOuOOq1LpKWbHqRgEU2Ze6/7lU8EyqwNBzx8+MRBS9waOciM1TOIzYitzUdycXecAr7qcC",
        "gYA5+/lSJ1ewfexL1mJHS9KpQC7179aWG+iHapbGwMP3JiIY7+X1zB4E76ILQVwuVDjufDwaz6JV",
        "mpphl9vEj4a9LnLZRWhRrXeeeLJ/8iQqnc228Y7i7anj0WnQdlFZNMydIkq85htsRIbFSkrsaaoO",
        "Ee0d9kSilX8a3mDd97OUAQKBgQCBwmAqYK/fyaTlXzvYgWrg09U1XsjrIq151wjQljV9OyQHbafT",
        "pDTG34N5wZZxQivQgIOFyQV0OJMpIyjTmRK4Tl1ZlTycpYDC100fvQR+5eqRr1En8qf3AEfWIow9",
        "8AYw3yPEcm8pDY2fVCF/J0/Ajw+kWqsCfwsfl+lTi8TfPQKBgQCqMx9I4uSjw6xEIoYGBCgjRorY",
        "14Njhox5UoLFZr2KXONFZm+Cm+l+bqPT+yu/p26SAWcTD6cpzn6joRwCIusf5MtEoo33Pgh/7tdg",
        "y/vtQVC3yPyMHfhkwsMZAj1c1ez3NbOZJm9GK+BS9YWoOAHWGC0I1PsE/1v6DgfQ1Zr2gw==",
    );

    /// Public half (SPKI DER, base64) of [`TEST_SA_PRIVATE_KEY_DER_B64`] —
    /// the stub token endpoint verifies the RS256 assertion against it.
    const TEST_SA_PUBLIC_SPKI_B64: &str = concat!(
        "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAoptEfzjxiQ1VJoEzYLvdYM9dJzSwd25d",
        "MxqbBYzjfRGgNUZPIeHe2hx66w+lcmGpY1zvzuPGCXKcvmFAiYxDo594gyGaF6ZgR5tX/d1V+DDP",
        "mCnq2fxdPimHfXTixR0hIbmkNDqcvAxbhKJFcx0aSWYkNY2fyHVyHCLaw4+IT+QztX3Zgh+45Aso",
        "L3LGAAlEbrF6hjJFHku4cEpE13HhA3XxEruQ4RtdnUH96uS3HKN0Zt3XF7ENbn8/JokSf3qfie5j",
        "LDCxN8521unaPP5wsNi/ADYKo8Urv1hG+0NZC57bgNBuZoUSmm3enK5H6qKdUJSkArkNjwf5hSlQ",
        "pJCu9wIDAQAB",
    );

    /// Rebuild the PEM-armored PKCS#8 form of the test SA key (the shape a
    /// real Google-issued SA JSON carries in `private_key`). The embedded
    /// DER constant is the PKCS#1 RSAPrivateKey encoding of the key whose
    /// public half is [`TEST_SA_PUBLIC_SPKI_B64`]; Google's armor wraps the
    /// PKCS#8 PrivateKeyInfo form, so the PKCS#1 bytes are re-wrapped here
    /// exactly as `openssl pkcs8 -topk8` would — a bare PKCS#1 body under
    /// PKCS#8 armor would be rejected by `RsaKeyPair::from_pkcs8` before
    /// any grant is signed.
    fn test_sa_private_key_pem() -> String {
        use base64::Engine as _;
        let pkcs1 = base64::engine::general_purpose::STANDARD
            .decode(TEST_SA_PRIVATE_KEY_DER_B64.trim())
            .expect("test key DER is valid base64");
        let pkcs8 = pkcs8_wrap_pkcs1(&pkcs1);
        let b64 = base64::engine::general_purpose::STANDARD.encode(&pkcs8);
        let mut pem = String::from("-----BEGIN PRIVATE KEY-----\n");
        for chunk in b64.as_bytes().chunks(64) {
            pem.push_str(std::str::from_utf8(chunk).unwrap());
            pem.push('\n');
        }
        pem.push_str("-----END PRIVATE KEY-----\n");
        pem
    }

    /// Minimal DER TLV encoder: tag, DER length, content.
    fn der_tlv(tag: u8, content: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        let len = content.len();
        if len < 0x80 {
            out.push(len as u8);
        } else {
            let bytes = len.to_be_bytes();
            let skip = bytes.iter().take_while(|b| **b == 0).count();
            out.push(0x80 | (bytes.len() - skip) as u8);
            out.extend_from_slice(&bytes[skip..]);
        }
        out.extend_from_slice(content);
        out
    }

    /// Wrap a PKCS#1 RSAPrivateKey DER in a PKCS#8 PrivateKeyInfo
    /// (version 0 + rsaEncryption AlgorithmIdentifier + OCTET STRING) —
    /// the format `RsaKeyPair::from_pkcs8` and real Google SA keys use.
    fn pkcs8_wrap_pkcs1(pkcs1: &[u8]) -> Vec<u8> {
        const RSA_ENCRYPTION_ALGID: &[u8] = &[
            0x06, 0x09, // OID, 9 bytes: 1.2.840.113549.1.1.1 (rsaEncryption)
            0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05, 0x00, // + NULL params
        ];
        let mut content = der_tlv(0x02, &[0]); // version: 0
        content.extend(der_tlv(0x30, RSA_ENCRYPTION_ALGID));
        content.extend(der_tlv(0x04, pkcs1)); // privateKey OCTET STRING
        der_tlv(0x30, &content)
    }

    /// A full, Google-shaped service-account key document pointing at
    /// `token_uri` — every extra field a real SA JSON carries is included
    /// so the preservation assertions are meaningful.
    fn test_sa_doc(token_uri: &str) -> serde_json::Value {
        serde_json::json!({
            "type": "service_account",
            "project_id": "gyre-test-project",
            "private_key_id": "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6a7b8c9d0",
            "private_key": test_sa_private_key_pem(),
            "client_email": "sa-test@gyre-test-project.iam.gserviceaccount.com",
            "client_id": "102938475610293847561",
            "auth_uri": "https://accounts.google.com/o/oauth2/auth",
            "token_uri": token_uri,
            "auth_provider_x509_cert_url": "https://www.googleapis.com/oauth2/v1/certs",
            "client_x509_cert_url":
                "https://www.googleapis.com/robot/v1/metadata/x509/sa-test%40gyre-test-project.iam.gserviceaccount.com"
        })
    }

    /// Stub Google token endpoint for the JWT-bearer grant: it enforces the
    /// RFC 7523 request shape — `grant_type` MUST be the jwt-bearer URI and
    /// the `assertion` MUST be an RS256 JWT whose signature verifies against
    /// the test key's public half with the expected iss/aud/scope claims —
    /// returning 400 with the failure reason when it does not. The received
    /// form body is captured for test-side assertions.
    async fn spawn_sa_token_endpoint() -> (String, Arc<tokio::sync::Mutex<String>>) {
        use base64::Engine as _;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(tokio::sync::Mutex::new(String::new()));
        let seen_clone = Arc::clone(&seen);
        let app = axum::Router::new().route(
            "/token",
            axum::routing::post(move |body: String| {
                let seen = Arc::clone(&seen_clone);
                async move {
                    *seen.lock().await = body.clone();

                    let bad_request = |reason: &str| {
                        (
                            axum::http::StatusCode::BAD_REQUEST,
                            axum::Json(serde_json::json!({ "error": reason })),
                        )
                    };

                    let grant_type = form_get(&body, "grant_type").unwrap_or_default();
                    if grant_type != "urn:ietf:params:oauth:grant-type:jwt-bearer" {
                        return bad_request("unexpected grant_type");
                    }
                    let assertion = form_get(&body, "assertion").unwrap_or_default();
                    let mut parts = assertion.split('.');
                    let (h, c, s) = (
                        parts.next().unwrap_or_default(),
                        parts.next().unwrap_or_default(),
                        parts.next().unwrap_or_default(),
                    );
                    if parts.next().is_some() || h.is_empty() || c.is_empty() || s.is_empty() {
                        return bad_request("assertion is not a three-part JWT");
                    }

                    // Real RS256 verification against the test key's public
                    // half — a forged or malformed signature fails here.
                    let sig = base64::engine::general_purpose::URL_SAFE_NO_PAD
                        .decode(s)
                        .unwrap_or_default();
                    let spki = base64::engine::general_purpose::STANDARD
                        .decode(TEST_SA_PUBLIC_SPKI_B64.trim())
                        .expect("test public key is valid base64");
                    let pubkey = ring::signature::UnparsedPublicKey::new(
                        &ring::signature::RSA_PKCS1_2048_8192_SHA256,
                        spki,
                    );
                    let signing_input = format!("{h}.{c}");
                    if pubkey.verify(signing_input.as_bytes(), &sig).is_err() {
                        return bad_request("RS256 signature verification failed");
                    }

                    // Claim checks: iss = SA email, aud = this endpoint,
                    // cloud-platform scope, exp > iat.
                    let claims_raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
                        .decode(c)
                        .unwrap_or_default();
                    let claims: serde_json::Value = match serde_json::from_slice(&claims_raw) {
                        Ok(v) => v,
                        Err(_) => return bad_request("claims are not JSON"),
                    };
                    if claims["iss"] != "sa-test@gyre-test-project.iam.gserviceaccount.com" {
                        return bad_request("unexpected iss");
                    }
                    if claims["aud"] != format!("http://127.0.0.1:{port}/token") {
                        return bad_request("unexpected aud");
                    }
                    if claims["scope"] != "https://www.googleapis.com/auth/cloud-platform" {
                        return bad_request("unexpected scope");
                    }
                    let exp = claims["exp"].as_u64().unwrap_or(0);
                    let iat = claims["iat"].as_u64().unwrap_or(1);
                    if exp <= iat {
                        return bad_request("exp must be after iat");
                    }

                    (
                        axum::http::StatusCode::OK,
                        axum::Json(serde_json::json!({
                            "access_token": "sa-short-lived-access-token",
                            "expires_in": 5400,
                            "token_type": "Bearer"
                        })),
                    )
                }
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://127.0.0.1:{port}/token"), seen)
    }

    #[tokio::test]
    async fn gcp_sa_refresh_performs_real_rs256_jwt_bearer_grant_and_preserves_document() {
        let now = now_secs();
        let (endpoint, seen) = spawn_sa_token_endpoint().await;
        let sa = test_sa_doc(&endpoint);
        // A non-`*_refresh_token` name: the GCP SA handler is selected by
        // credential-document shape (type=service_account), like the OAuth
        // handler above.
        let state =
            seed_named_secret(now, "sec-gcp-sa", "GCP_SA_JSON", sa.to_string().as_bytes()).await;

        let log = crate::jobs::JobRunLog::new();
        run_once(&state, &log).await.unwrap();

        // The endpoint saw a jwt-bearer grant carrying an assertion that
        // verified (a 400 would have failed the rotation below).
        let posted = seen.lock().await.clone();
        let grant_type = form_get(&posted, "grant_type").unwrap_or_default();
        assert_eq!(
            grant_type, "urn:ietf:params:oauth:grant-type:jwt-bearer",
            "the SA refresh must be a JWT-bearer grant, posted: {posted}"
        );
        assert!(form_get(&posted, "assertion").is_some());

        // The stored value is the refreshed document: the new access token
        // AND every Google-issued field of the original SA key preserved
        // verbatim (no lossy re-serialization).
        let value = state
            .secrets
            .get_value(&Id::new("sec-gcp-sa"), "default")
            .await
            .unwrap()
            .expect("secret must survive rotation");
        let doc: serde_json::Value = serde_json::from_slice(&value).unwrap();
        assert_eq!(doc["access_token"], "sa-short-lived-access-token");
        assert_eq!(doc["type"], "service_account");
        assert_eq!(doc["project_id"], "gyre-test-project");
        assert_eq!(
            doc["private_key_id"],
            "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6a7b8c9d0"
        );
        assert_eq!(doc["client_id"], "102938475610293847561");
        assert_eq!(doc["auth_uri"], "https://accounts.google.com/o/oauth2/auth");
        assert_eq!(
            doc["auth_provider_x509_cert_url"],
            "https://www.googleapis.com/oauth2/v1/certs"
        );
        assert!(doc["client_x509_cert_url"].is_string());
        assert_eq!(doc["private_key"], sa["private_key"]);
        assert_eq!(doc["client_email"], sa["client_email"]);
        assert_eq!(doc["token_uri"], endpoint);

        // Expiry extended past the check window; last_rotated_at set.
        let stored = state
            .secrets
            .find_by_id(&Id::new("sec-gcp-sa"), "default")
            .await
            .unwrap()
            .unwrap();
        assert!(
            stored.expires_at.unwrap() > now + 3600,
            "SA refresh must extend expiry past the check window, got {:?}",
            stored.expires_at
        );
        assert!(stored.last_rotated_at.is_some());

        // The durable run log records the per-secret refreshed outcome via
        // the SA handler.
        let lines = log.take().await;
        assert!(
            lines
                .iter()
                .any(|l| l.contains("refreshed via gcp_service_account_jwt_bearer")),
            "run log must record the SA refresh outcome, got: {lines:?}"
        );

        // Successful rotation suppresses the admin notification.
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-admin"),
                None,
                None,
                None,
                Some(&["SecretExpiring"]),
                None,
                &[],
                50,
                0,
            )
            .await
            .unwrap();
        assert!(
            notifs.is_empty(),
            "a handler-rotated SA secret must not notify admins"
        );
    }

    #[tokio::test]
    async fn gcp_sa_refresh_failure_leaves_document_untouched_and_notifies() {
        // A bad private key: the handler cannot sign, so rotation fails and
        // the admin is notified with the stored document intact.
        let now = now_secs();
        // Point at an endpoint that would succeed — the failure must come
        // from signing, before any HTTP call.
        let (endpoint, _seen) = spawn_sa_token_endpoint().await;
        let mut sa = test_sa_doc(&endpoint);
        sa["private_key"] = serde_json::json!(
            "-----BEGIN PRIVATE KEY-----\nnot-a-key\n-----END PRIVATE KEY-----\n"
        );
        let original = sa.to_string();
        let state =
            seed_named_secret(now, "sec-gcp-sa-bad", "GCP_SA_JSON", original.as_bytes()).await;

        let log = crate::jobs::JobRunLog::new();
        run_once(&state, &log).await.unwrap();

        let value = state
            .secrets
            .get_value(&Id::new("sec-gcp-sa-bad"), "default")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            value,
            original.as_bytes(),
            "failed SA refresh must not corrupt the credential"
        );

        let lines = log.take().await;
        assert!(
            lines
                .iter()
                .any(|l| l.contains("FAILED") && l.contains("gcp_service_account_jwt_bearer")),
            "run log must record the failed refresh, got: {lines:?}"
        );
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-admin"),
                None,
                None,
                None,
                Some(&["SecretExpiring"]),
                None,
                &[],
                50,
                0,
            )
            .await
            .unwrap();
        assert_eq!(notifs.len(), 1, "failed SA refresh must notify admins");
    }

    // ── Refresh cadence (review task-273: perpetual refresh loop) ──────────

    fn cadence_secret(expires_at: u64, last_rotated_at: Option<u64>) -> gyre_common::Secret {
        gyre_common::Secret {
            id: Id::new("sec-cadence"),
            name: "gcp_sa_json".to_string(),
            scope: gyre_common::SecretScope::Workspace,
            scope_id: "ws-rot".to_string(),
            secret_type: gyre_common::SecretType::Rotated,
            created_by: "system".to_string(),
            created_at: 1,
            expires_at: Some(expires_at),
            last_rotated_at,
            tenant_id: "default".to_string(),
        }
    }

    /// A credential whose granted lifetime equals the scan window (a
    /// Google access token: expires_in 3600 against a 3600 s window) must
    /// NOT be re-refreshed on every job cycle: right after a refresh the
    /// remaining lifetime (3600 s) is above the refresh threshold, so the
    /// next refresh defers until the credential nears expiry again.
    #[test]
    fn should_refresh_does_not_loop_on_window_sized_grants() {
        let now = 1_000_000u64;
        // Just refreshed: granted 3600 s at `now`.
        let fresh = cadence_secret(now + 3600, Some(now));
        assert!(
            !should_refresh(&fresh, now + 300),
            "a freshly granted 3600 s token (3300 s left) must not re-refresh \
             every 5-minute cycle"
        );
        // Still not refreshable at half life (1800 s left; threshold is
        // 3600/5 = 720 s).
        assert!(!should_refresh(&fresh, now + 1800));
        // Refreshable inside the threshold (600 s left ≤ 720 s).
        assert!(
            should_refresh(&fresh, now + 3000),
            "refresh must fire once the remaining lifetime is at/below the \
             granted-lifetime threshold"
        );
    }

    #[test]
    fn should_refresh_first_refresh_and_expiry_edges() {
        let now = 1_000_000u64;
        // Never rotated: the first refresh uses the full window.
        let never_rotated = cadence_secret(now + 1800, None);
        assert!(should_refresh(&never_rotated, now));
        // Already expired: always refreshable.
        let expired = cadence_secret(now - 10, Some(now - 3610));
        assert!(should_refresh(&expired, now));
        // Outside the scan window entirely: not refreshable.
        let far = cadence_secret(now + 7200, Some(now));
        assert!(!should_refresh(&far, now));
        // Long-lived grant (30 days): threshold clamps to the window —
        // refreshed within the last hour of life only (pre-existing
        // behavior preserved).
        let long_lived = cadence_secret(now + 30 * 86_400, Some(now));
        assert!(!should_refresh(&long_lived, now + 29 * 86_400));
        assert!(should_refresh(&long_lived, now + 30 * 86_400 - 1800));
    }

    /// A token response without expires_in cannot set an honest expiry —
    /// the rotation must FAIL (notification + untouched stored value)
    /// rather than fabricate a 3600 s lifetime (review task-273).
    #[tokio::test]
    async fn missing_expires_in_fails_rotation_instead_of_fabricating_a_lifetime() {
        let now = now_secs();
        let (endpoint, _seen) = spawn_token_endpoint(
            axum::http::StatusCode::OK,
            serde_json::json!({
                "access_token": "new-access-token",
                "refresh_token": "new-refresh-token"
            }),
        )
        .await;
        let cred = serde_json::json!({
            "token_endpoint": endpoint,
            "client_id": "gyre-test",
            "refresh_token": "old-refresh-token"
        });
        let original = cred.to_string();
        let state = seed_oauth(now, original.as_bytes()).await;

        let log = crate::jobs::JobRunLog::new();
        run_once(&state, &log).await.unwrap();

        // The stored credential is untouched — no fabricated expiry, no
        // half-applied rotation.
        let value = state
            .secrets
            .get_value(&Id::new("sec-oauth"), "default")
            .await
            .unwrap()
            .expect("secret must survive the failed rotation");
        assert_eq!(value, original.as_bytes());

        // The failure is recorded in the run log and escalated to admins.
        let lines = log.take().await;
        assert!(
            lines
                .iter()
                .any(|l| l.contains("FAILED") && l.contains("missing expires_in")),
            "run log must record the missing-expires_in failure, got: {lines:?}"
        );
        let notifs = state
            .notifications
            .list_for_user(
                &Id::new("user-admin"),
                None,
                None,
                None,
                Some(&["SecretExpiring"]),
                None,
                &[],
                50,
                0,
            )
            .await
            .unwrap();
        assert_eq!(
            notifs.len(),
            1,
            "a rotation that cannot determine the new expiry must notify admins"
        );
    }
}
