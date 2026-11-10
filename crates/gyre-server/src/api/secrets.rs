//! Secret Admin API (platform-model.md §7 Admin API, task-098).
//!
//! `POST   /api/v1/secrets`          — create (or update) a secret
//! `GET    /api/v1/secrets`          — list secret metadata by scope (never values)
//! `DELETE /api/v1/secrets/:id`      — delete a secret
//! `POST   /api/v1/secrets/:id/rotate` — force-rotate a secret's value
//!
//! All endpoints require the Admin role (enforced both by ABAC — the
//! `builtin-secret-admin-only` policy denies non-Admin roles on the "secret"
//! resource type — and in-handler via `require_admin`). Every response
//! carries metadata only: the plaintext value is accepted on create/rotate
//! and never returned. All queries are scoped by the caller's tenant_id.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::{Id, Secret, SecretScope, SecretType};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

use crate::{api::error::ApiError, auth::AuthenticatedAgent, AppState};

// ── Request / Response types ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateSecretRequest {
    /// Secret name, e.g. `DATABASE_URL`.
    pub name: String,
    /// Scope level: "tenant" | "workspace" | "repo" | "task".
    pub scope: String,
    /// The scoped entity's id (workspace_id, repo_id, task_id; ignored for
    /// tenant scope, which always targets the caller's tenant).
    #[serde(default)]
    pub scope_id: Option<String>,
    /// Lifecycle class: "static" | "ephemeral" | "rotated" | "derived".
    pub secret_type: String,
    /// Plaintext value. Accepted here; never present in any response.
    pub value: String,
    /// Optional expiry (Unix epoch seconds).
    #[serde(default)]
    pub expires_at: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct RotateSecretRequest {
    /// New plaintext value.
    pub new_value: String,
    /// Optional new expiry (Unix epoch seconds). None = keep current expiry.
    #[serde(default)]
    pub expires_at: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct ListSecretsQuery {
    /// Scope level: "tenant" | "workspace" | "repo" | "task". Required.
    pub scope: String,
    /// The scoped entity's id. Required for workspace/repo/task scope.
    /// Tenant scope defaults to the caller's own tenant.
    #[serde(default)]
    pub scope_id: Option<String>,
}

fn parse_scope(s: &str) -> Result<SecretScope, ApiError> {
    SecretScope::from_str_opt(s).ok_or_else(|| {
        ApiError::InvalidInput(format!(
            "unknown scope '{s}' (expected tenant, workspace, repo, or task)"
        ))
    })
}

fn parse_secret_type(s: &str) -> Result<SecretType, ApiError> {
    SecretType::from_str_opt(s).ok_or_else(|| {
        ApiError::InvalidInput(format!(
            "unknown secret_type '{s}' (expected static, ephemeral, rotated, or derived)"
        ))
    })
}

/// Secret metadata as returned by list/create. Never includes the value.
#[derive(Debug, Serialize)]
pub struct SecretResponse {
    pub id: String,
    pub name: String,
    pub scope: String,
    pub scope_id: String,
    pub secret_type: String,
    pub created_by: String,
    pub created_at: u64,
    pub expires_at: Option<u64>,
    pub last_rotated_at: Option<u64>,
    pub tenant_id: String,
}

impl From<Secret> for SecretResponse {
    fn from(s: Secret) -> Self {
        Self {
            id: s.id.to_string(),
            name: s.name,
            scope: s.scope.as_str().to_string(),
            scope_id: s.scope_id,
            secret_type: s.secret_type.as_str().to_string(),
            created_by: s.created_by,
            created_at: s.created_at,
            expires_at: s.expires_at,
            last_rotated_at: s.last_rotated_at,
            tenant_id: s.tenant_id,
        }
    }
}

// ── Handlers ────────────────────────────────────────────────────────────────

/// POST /api/v1/secrets — create a secret, or update the value of an
/// existing secret with the same (scope, scope_id, name) in the tenant
/// (the spec's "Create/update secret (admin only)").
pub async fn create_secret(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Json(req): Json<CreateSecretRequest>,
) -> Result<(StatusCode, Json<SecretResponse>), ApiError> {
    require_admin(&state, &auth, "POST /api/v1/secrets").await?;
    let tenant_id = auth.tenant_id.clone();

    let scope = parse_scope(&req.scope)?;
    let secret_type = parse_secret_type(&req.secret_type)?;
    let name = req.name.trim().to_string();
    if name.is_empty() || name.len() > 255 {
        return Err(ApiError::InvalidInput(
            "secret name must be 1-255 characters".to_string(),
        ));
    }
    if req.value.is_empty() {
        return Err(ApiError::InvalidInput(
            "secret value must not be empty".to_string(),
        ));
    }

    // Resolve + validate the scope target against the caller's tenant. The
    // scope_id is caller-supplied, so it must be verified to belong to the
    // caller's tenant before it is persisted (anti-forgery: never bind one
    // identity field server-side while its siblings pass through).
    let scope_id = validate_scope_target(&state, &auth, scope, req.scope_id.as_deref()).await?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let created_by = auth.agent_id.clone();

    // Create-or-update semantics: an existing secret with the same
    // (scope, scope_id, name) in this tenant is updated to the new value
    // (rotation) rather than duplicated — the storage layer enforces
    // uniqueness on that tuple, so POST is the spec's "create/update".
    let existing = state
        .secrets
        .list_by_scope(scope, &scope_id, &tenant_id)
        .await?
        .into_iter()
        .find(|s| s.name == name);

    let secret = match existing {
        Some(prev) => {
            let mut updated = prev.clone();
            updated.secret_type = secret_type;
            updated.expires_at = req.expires_at;
            updated.last_rotated_at = Some(now);
            rewrite_secret(&state, &updated, req.value.as_bytes()).await?;
            updated
        }
        None => {
            let secret = Secret {
                id: Id::new(uuid::Uuid::new_v4().to_string()),
                name,
                scope,
                scope_id,
                secret_type,
                created_by,
                created_at: now,
                expires_at: req.expires_at,
                last_rotated_at: None,
                tenant_id: tenant_id.clone(),
            };
            state.secrets.create(&secret, req.value.as_bytes()).await?;
            secret
        }
    };

    info!(
        tenant_id = %tenant_id,
        secret_id = %secret.id,
        scope = scope.as_str(),
        "secret.created_or_updated"
    );

    Ok((StatusCode::CREATED, Json(secret.into())))
}

/// GET /api/v1/secrets?scope=workspace&scope_id=ws-1 — list secret metadata
/// by scope. Never returns values.
pub async fn list_secrets(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Query(q): Query<ListSecretsQuery>,
) -> Result<Json<Vec<SecretResponse>>, ApiError> {
    require_admin(&state, &auth, "GET /api/v1/secrets").await?;
    let tenant_id = auth.tenant_id.clone();

    let scope = parse_scope(&q.scope)?;
    let scope_id = validate_scope_target(&state, &auth, scope, q.scope_id.as_deref()).await?;

    let secrets = state
        .secrets
        .list_by_scope(scope, &scope_id, &tenant_id)
        .await?;
    Ok(Json(secrets.into_iter().map(Into::into).collect()))
}

/// DELETE /api/v1/secrets/:id — delete a secret.
pub async fn delete_secret(
    State(state): State<Arc<AppState>>,
    Path(secret_id): Path<String>,
    auth: AuthenticatedAgent,
) -> Result<StatusCode, ApiError> {
    require_admin(&state, &auth, "DELETE /api/v1/secrets/:id").await?;
    let tenant_id = auth.tenant_id.clone();
    let id = Id::new(secret_id.clone());

    // 404 when absent (or in another tenant) — never a silent no-op.
    state
        .secrets
        .find_by_id(&id, &tenant_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("secret '{secret_id}' not found")))?;

    state.secrets.delete(&id, &tenant_id).await?;

    info!(tenant_id = %tenant_id, secret_id = %secret_id, "secret.deleted");
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/secrets/:id/rotate — force rotation: replace the value and
/// update `last_rotated_at`.
pub async fn rotate_secret(
    State(state): State<Arc<AppState>>,
    Path(secret_id): Path<String>,
    auth: AuthenticatedAgent,
    Json(req): Json<RotateSecretRequest>,
) -> Result<Json<SecretResponse>, ApiError> {
    require_admin(&state, &auth, "POST /api/v1/secrets/:id/rotate").await?;
    let tenant_id = auth.tenant_id.clone();
    let id = Id::new(secret_id.clone());

    if req.new_value.is_empty() {
        return Err(ApiError::InvalidInput(
            "new_value must not be empty".to_string(),
        ));
    }

    state
        .secrets
        .find_by_id(&id, &tenant_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("secret '{secret_id}' not found")))?;

    state
        .secrets
        .rotate(&id, req.new_value.as_bytes(), &tenant_id)
        .await?;

    let mut updated = state
        .secrets
        .find_by_id(&id, &tenant_id)
        .await?
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("secret vanished after rotate")))?;

    // rotate() covers value + last_rotated_at; an expiry change additionally
    // rewrites the row's metadata.
    if let Some(expires_at) = req.expires_at {
        updated.expires_at = Some(expires_at);
        rewrite_secret(&state, &updated, req.new_value.as_bytes()).await?;
    }

    // §7 audit event: secret.rotated — name (never value), scope, actor.
    info!(
        event = "secret.rotated",
        tenant_id = %tenant_id,
        secret_id = %secret_id,
        secret_name = %updated.name,
        scope = updated.scope.as_str(),
        rotated_by = %auth.agent_id,
        "secret.rotated: admin force-rotate"
    );
    Ok(Json(updated.into()))
}

// ── Helpers ─────────────────────────────────────────────────────────────────

async fn require_admin(
    state: &AppState,
    auth: &AuthenticatedAgent,
    endpoint: &str,
) -> Result<(), ApiError> {
    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
        crate::rbac::record_rbac_denied(state, auth, "Admin", endpoint).await;
        return Err(ApiError::Forbidden(
            "secret management requires Admin role".to_string(),
        ));
    }
    Ok(())
}

/// Resolve and validate a caller-supplied (scope, scope_id) pair against the
/// caller's tenant.
///
/// - tenant: scope_id is ignored; the target is always the caller's own
///   tenant (a supplied scope_id must match it).
/// - workspace: the workspace must exist and belong to the caller's tenant.
/// - repo: the repo must exist, and its owning workspace must belong to the
///   caller's tenant.
/// - task: the task must exist in the caller's tenant.
///
/// Returns the validated scope_id to persist.
async fn validate_scope_target(
    state: &Arc<AppState>,
    auth: &AuthenticatedAgent,
    scope: SecretScope,
    scope_id: Option<&str>,
) -> Result<String, ApiError> {
    let tenant_id = &auth.tenant_id;
    match scope {
        SecretScope::Tenant => {
            if let Some(sid) = scope_id {
                if sid != tenant_id {
                    return Err(ApiError::Forbidden(
                        "cannot manage another tenant's secrets".to_string(),
                    ));
                }
            }
            Ok(tenant_id.clone())
        }
        SecretScope::Workspace => {
            let sid = scope_id.ok_or_else(|| {
                ApiError::InvalidInput(
                    "scope_id is required for workspace-scoped secrets".to_string(),
                )
            })?;
            let ws = state
                .workspaces
                .find_by_id(&Id::new(sid))
                .await
                .map_err(ApiError::Internal)?
                .ok_or_else(|| ApiError::NotFound(format!("workspace '{sid}' not found")))?;
            if ws.tenant_id.to_string() != *tenant_id {
                return Err(ApiError::Forbidden(
                    "workspace belongs to a different tenant".to_string(),
                ));
            }
            Ok(ws.id.to_string())
        }
        SecretScope::Repo => {
            let sid = scope_id.ok_or_else(|| {
                ApiError::InvalidInput("scope_id is required for repo-scoped secrets".to_string())
            })?;
            let repo = state
                .repos
                .find_by_id(&Id::new(sid))
                .await
                .map_err(ApiError::Internal)?
                .ok_or_else(|| ApiError::NotFound(format!("repo '{sid}' not found")))?;
            // Repos carry workspace_id, not tenant_id: resolve through the
            // owning workspace.
            let ws = state
                .workspaces
                .find_by_id(&repo.workspace_id)
                .await
                .map_err(ApiError::Internal)?
                .ok_or_else(|| {
                    ApiError::NotFound(format!(
                        "owning workspace '{}' not found for repo '{sid}'",
                        repo.workspace_id
                    ))
                })?;
            if ws.tenant_id.to_string() != *tenant_id {
                return Err(ApiError::Forbidden(
                    "repo belongs to a different tenant".to_string(),
                ));
            }
            Ok(repo.id.to_string())
        }
        SecretScope::Task => {
            let sid = scope_id.ok_or_else(|| {
                ApiError::InvalidInput("scope_id is required for task-scoped secrets".to_string())
            })?;
            let task = state
                .tasks
                .find_by_id(&Id::new(sid))
                .await
                .map_err(ApiError::Internal)?
                .ok_or_else(|| ApiError::NotFound(format!("task '{sid}' not found")))?;
            // Tasks carry workspace_id, not tenant_id: resolve through the
            // governing workspace.
            let ws = state
                .workspaces
                .find_by_id(&task.workspace_id)
                .await
                .map_err(ApiError::Internal)?
                .ok_or_else(|| {
                    ApiError::NotFound(format!(
                        "governing workspace '{}' not found for task '{sid}'",
                        task.workspace_id
                    ))
                })?;
            if ws.tenant_id.to_string() != *tenant_id {
                return Err(ApiError::Forbidden(
                    "task belongs to a different tenant".to_string(),
                ));
            }
            Ok(task.id.to_string())
        }
    }
}

/// Re-write a secret row with updated metadata, preserving the caller's new
/// value. The port's `rotate()` only touches value + `last_rotated_at`, so
/// metadata changes (expiry, type) go through delete + create. The value is
/// re-supplied from the caller's request (never read back out of storage).
async fn rewrite_secret(
    state: &Arc<AppState>,
    secret: &Secret,
    value: &[u8],
) -> Result<(), ApiError> {
    state
        .secrets
        .delete(&secret.id, &secret.tenant_id)
        .await
        .map_err(ApiError::Internal)?;
    state
        .secrets
        .create(secret, value)
        .await
        .map_err(ApiError::Internal)?;
    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
    use axum::{body::Body, routing::get, Router};
    use gyre_domain::Workspace;
    use http::{Request, StatusCode};
    use std::sync::Arc;
    use tower::ServiceExt;

    fn app() -> (Router, Arc<crate::AppState>) {
        let state = crate::mem::test_state();
        let router = Router::new()
            .route(
                "/api/v1/secrets",
                get(super::list_secrets).post(super::create_secret),
            )
            .route(
                "/api/v1/secrets/:id",
                axum::routing::delete(super::delete_secret),
            )
            .route(
                "/api/v1/secrets/:id/rotate",
                axum::routing::post(super::rotate_secret),
            )
            .with_state(state.clone());
        (router, state)
    }

    /// Router backed by a JWT-enabled state, for role-based denial tests.
    fn jwt_app() -> Router {
        let state = make_test_state_with_jwt();
        Router::new()
            .route(
                "/api/v1/secrets",
                get(super::list_secrets).post(super::create_secret),
            )
            .route(
                "/api/v1/secrets/:id",
                axum::routing::delete(super::delete_secret),
            )
            .route(
                "/api/v1/secrets/:id/rotate",
                axum::routing::post(super::rotate_secret),
            )
            .with_state(state)
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn post_json(uri: &str, token: &str, body: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    }

    async fn seed_workspace(state: &Arc<crate::AppState>) -> String {
        let ws = Workspace::new(
            gyre_common::Id::new("ws-secret-1"),
            gyre_common::Id::new("default"),
            "Secrets WS",
            "secrets-ws",
            1_700_000_000,
        );
        state.workspaces.create(&ws).await.unwrap();
        ws.id.to_string()
    }

    #[tokio::test]
    async fn secret_crud_lifecycle() {
        let (app, state) = app();
        let ws_id = seed_workspace(&state).await;

        // Create (workspace scope).
        let body = serde_json::json!({
            "name": "DATABASE_URL",
            "scope": "workspace",
            "scope_id": ws_id,
            "secret_type": "static",
            "value": "postgres://super:secret@db/example",
            "expires_at": 4_102_444_800u64
        });
        let resp = app
            .clone()
            .oneshot(post_json("/api/v1/secrets", "test-token", body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["name"], "DATABASE_URL");
        assert_eq!(json["scope"], "workspace");
        assert_eq!(json["scope_id"], ws_id);
        assert_eq!(json["secret_type"], "static");
        assert_eq!(json["expires_at"], 4_102_444_800u64);
        // The value must never appear in the response.
        assert!(!json.to_string().contains("postgres://super:secret"));
        let secret_id = json["id"].as_str().unwrap().to_string();

        // List — metadata only.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/secrets?scope=workspace&scope_id={ws_id}"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], "DATABASE_URL");
        assert!(!json.to_string().contains("postgres://super:secret"));

        // The stored value round-trips through the port (proves it was
        // really stored, not just acknowledged).
        let value = state
            .secrets
            .get_value(&gyre_common::Id::new(secret_id.clone()), "default")
            .await
            .unwrap()
            .expect("secret value must be stored");
        assert_eq!(value, b"postgres://super:secret@db/example".to_vec());

        // Rotate.
        let body = serde_json::json!({ "new_value": "postgres://rotated@db/example" });
        let resp = app
            .clone()
            .oneshot(post_json(
                &format!("/api/v1/secrets/{secret_id}/rotate"),
                "test-token",
                body,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(
            json["last_rotated_at"].as_u64().unwrap() > 0,
            "rotation must set last_rotated_at"
        );
        assert!(!json.to_string().contains("postgres://rotated"));

        let value = state
            .secrets
            .get_value(&gyre_common::Id::new(secret_id.clone()), "default")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(value, b"postgres://rotated@db/example".to_vec());

        // POST same (scope, name) again → update, not duplicate.
        let body = serde_json::json!({
            "name": "DATABASE_URL",
            "scope": "workspace",
            "scope_id": ws_id,
            "secret_type": "rotated",
            "value": "postgres://v3@db/example"
        });
        let resp = app
            .clone()
            .oneshot(post_json("/api/v1/secrets", "test-token", body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["id"], secret_id);
        assert_eq!(json["secret_type"], "rotated");

        let listed = state
            .secrets
            .list_by_scope(gyre_common::SecretScope::Workspace, &ws_id, "default")
            .await
            .unwrap();
        assert_eq!(listed.len(), 1, "update must not duplicate the secret");

        // Delete.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/secrets/{secret_id}"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Now absent: list is empty, delete again → 404, get_value → None.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/secrets?scope=workspace&scope_id={ws_id}"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 0);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/secrets/{secret_id}"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn cross_tenant_scope_id_is_rejected() {
        let (app, state) = app();

        // Workspace in a different tenant.
        let ws = Workspace::new(
            gyre_common::Id::new("ws-other-tenant"),
            gyre_common::Id::new("tenant-b"),
            "Other Tenant WS",
            "other-ws",
            1_700_000_000,
        );
        state.workspaces.create(&ws).await.unwrap();

        let body = serde_json::json!({
            "name": "STOLEN",
            "scope": "workspace",
            "scope_id": "ws-other-tenant",
            "secret_type": "static",
            "value": "x"
        });
        let resp = app
            .clone()
            .oneshot(post_json("/api/v1/secrets", "test-token", body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Unknown workspace → 404, not a silent create.
        let body = serde_json::json!({
            "name": "GHOST",
            "scope": "workspace",
            "scope_id": "ws-nope",
            "secret_type": "static",
            "value": "x"
        });
        let resp = app
            .oneshot(post_json("/api/v1/secrets", "test-token", body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn non_admin_jwt_is_forbidden() {
        let app = jwt_app();
        let token = sign_test_jwt(
            &serde_json::json!({
                "sub": "dev-sub",
                "preferred_username": "developer",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );

        // Create denied.
        let body = serde_json::json!({
            "name": "NOPE",
            "scope": "tenant",
            "secret_type": "static",
            "value": "x"
        });
        let resp = app
            .clone()
            .oneshot(post_json("/api/v1/secrets", &token, body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // List denied.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/secrets?scope=tenant")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Delete denied.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/secrets/whatever")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Rotate denied.
        let resp = app
            .oneshot(post_json(
                "/api/v1/secrets/whatever/rotate",
                &token,
                serde_json::json!({ "new_value": "x" }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_jwt_can_create_tenant_scoped_secret() {
        let app = jwt_app();
        let token = sign_test_jwt(
            &serde_json::json!({
                "sub": "admin-sub",
                "preferred_username": "admin",
                "tenant_id": "default",
                "realm_access": { "roles": ["admin"] }
            }),
            3600,
        );
        let body = serde_json::json!({
            "name": "TENANT_KEY",
            "scope": "tenant",
            "secret_type": "rotated",
            "value": "tenant-secret-value"
        });
        let resp = app
            .oneshot(post_json("/api/v1/secrets", &token, body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["scope"], "tenant");
        assert_eq!(json["scope_id"], "default");
        assert!(!json.to_string().contains("tenant-secret-value"));
    }

    #[tokio::test]
    async fn task_scope_requires_valid_task_in_tenant() {
        let (app, state) = app();

        let ws_id = seed_workspace(&state).await;
        let now = 1_700_000_100u64;
        let mut task = gyre_domain::Task::new(gyre_common::Id::new("task-sec-1"), "Some task", now);
        task.workspace_id = gyre_common::Id::new(ws_id.clone());
        state.tasks.create(&task).await.unwrap();

        // Valid task scope.
        let body = serde_json::json!({
            "name": "TASK_TOKEN",
            "scope": "task",
            "scope_id": "task-sec-1",
            "secret_type": "ephemeral",
            "value": "v"
        });
        let resp = app
            .clone()
            .oneshot(post_json("/api/v1/secrets", "test-token", body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Foreign-tenant task (workspace in tenant-b) rejected.
        let ws_b = gyre_domain::Workspace::new(
            gyre_common::Id::new("ws-task-other"),
            gyre_common::Id::new("tenant-b"),
            "Other WS",
            "other-ws-2",
            now,
        );
        state.workspaces.create(&ws_b).await.unwrap();
        let mut foreign =
            gyre_domain::Task::new(gyre_common::Id::new("task-sec-2"), "Foreign task", now);
        foreign.workspace_id = ws_b.id;
        state.tasks.create(&foreign).await.unwrap();
        let body = serde_json::json!({
            "name": "TASK_TOKEN",
            "scope": "task",
            "scope_id": "task-sec-2",
            "secret_type": "ephemeral",
            "value": "v"
        });
        let resp = app
            .oneshot(post_json("/api/v1/secrets", "test-token", body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn invalid_scope_and_type_are_rejected() {
        let (app, _state) = app();
        let body = serde_json::json!({
            "name": "X",
            "scope": "galaxy",
            "secret_type": "static",
            "value": "v"
        });
        let resp = app
            .clone()
            .oneshot(post_json("/api/v1/secrets", "test-token", body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let body = serde_json::json!({
            "name": "X",
            "scope": "tenant",
            "secret_type": "magical",
            "value": "v"
        });
        let resp = app
            .oneshot(post_json("/api/v1/secrets", "test-token", body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
