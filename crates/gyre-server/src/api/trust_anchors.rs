//! Trust anchor CRUD endpoints (authorization-provenance.md §1.1, TASK-006).
//!
//! `GET    /api/v1/tenants/:id/trust-anchors`        — list trust anchors for a tenant
//! `POST   /api/v1/tenants/:id/trust-anchors`        — create a trust anchor
//! `GET    /api/v1/tenants/:id/trust-anchors/:aid`    — get a specific trust anchor
//! `PUT    /api/v1/tenants/:id/trust-anchors/:aid`    — update a trust anchor
//! `DELETE /api/v1/tenants/:id/trust-anchors/:aid`    — delete a trust anchor
//!
//! All endpoints require Admin role (ABAC: resource_type "attestation").

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::{OutputConstraint, TrustAnchor, TrustAnchorType};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

use crate::{api::error::ApiError, auth::AuthenticatedAgent, AppState};

// ── Request / Response types ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateTrustAnchorRequest {
    /// Stable identifier (e.g., "tenant-keycloak").
    pub id: String,
    /// OIDC issuer URL or SPIFFE trust domain.
    pub issuer: String,
    /// Public key endpoint (JWKS URI).
    pub jwks_uri: String,
    /// Type of identity this anchor authenticates.
    pub anchor_type: TrustAnchorType,
    /// Optional anchor-level output constraints.
    #[serde(default)]
    pub constraints: Vec<OutputConstraint>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTrustAnchorRequest {
    /// OIDC issuer URL or SPIFFE trust domain.
    pub issuer: Option<String>,
    /// Public key endpoint (JWKS URI).
    pub jwks_uri: Option<String>,
    /// Type of identity this anchor authenticates.
    pub anchor_type: Option<TrustAnchorType>,
    /// Anchor-level output constraints.
    pub constraints: Option<Vec<OutputConstraint>>,
}

#[derive(Debug, Serialize)]
pub struct TrustAnchorResponse {
    pub id: String,
    pub issuer: String,
    pub jwks_uri: String,
    pub anchor_type: TrustAnchorType,
    pub constraints: Vec<OutputConstraint>,
}

impl From<TrustAnchor> for TrustAnchorResponse {
    fn from(a: TrustAnchor) -> Self {
        Self {
            id: a.id,
            issuer: a.issuer,
            jwks_uri: a.jwks_uri,
            anchor_type: a.anchor_type,
            constraints: a.constraints,
        }
    }
}

// ── Handlers ────────────────────────────────────────────────────────────────

/// GET /api/v1/tenants/:id/trust-anchors
pub async fn list_trust_anchors(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    auth: AuthenticatedAgent,
) -> Result<Json<Vec<TrustAnchorResponse>>, ApiError> {
    require_admin(&auth)?;
    if auth.tenant_id != tenant_id {
        return Err(ApiError::Forbidden(
            "cannot access trust anchors for a different tenant".to_string(),
        ));
    }
    let anchors = state.trust_anchors.list_by_tenant(&tenant_id).await?;
    Ok(Json(anchors.into_iter().map(Into::into).collect()))
}

/// POST /api/v1/tenants/:id/trust-anchors
pub async fn create_trust_anchor(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    auth: AuthenticatedAgent,
    Json(req): Json<CreateTrustAnchorRequest>,
) -> Result<(StatusCode, Json<TrustAnchorResponse>), ApiError> {
    require_admin(&auth)?;
    if auth.tenant_id != tenant_id {
        return Err(ApiError::Forbidden(
            "cannot access trust anchors for a different tenant".to_string(),
        ));
    }

    // Check for duplicate.
    if state
        .trust_anchors
        .find_by_id(&tenant_id, &req.id)
        .await?
        .is_some()
    {
        return Err(ApiError::InvalidInput(format!(
            "trust anchor '{}' already exists",
            req.id
        )));
    }

    let anchor = TrustAnchor {
        id: req.id,
        issuer: req.issuer,
        jwks_uri: req.jwks_uri,
        anchor_type: req.anchor_type,
        constraints: req.constraints,
    };

    state.trust_anchors.create(&tenant_id, &anchor).await?;

    info!(
        tenant_id = %tenant_id,
        anchor_id = %anchor.id,
        anchor_type = ?anchor.anchor_type,
        "trust_anchor.created"
    );

    Ok((StatusCode::CREATED, Json(anchor.into())))
}

/// GET /api/v1/tenants/:id/trust-anchors/:aid
pub async fn get_trust_anchor(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, anchor_id)): Path<(String, String)>,
    auth: AuthenticatedAgent,
) -> Result<Json<TrustAnchorResponse>, ApiError> {
    require_admin(&auth)?;
    if auth.tenant_id != tenant_id {
        return Err(ApiError::Forbidden(
            "cannot access trust anchors for a different tenant".to_string(),
        ));
    }
    let anchor = state
        .trust_anchors
        .find_by_id(&tenant_id, &anchor_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("trust anchor '{anchor_id}' not found")))?;
    Ok(Json(anchor.into()))
}

/// PUT /api/v1/tenants/:id/trust-anchors/:aid
pub async fn update_trust_anchor(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, anchor_id)): Path<(String, String)>,
    auth: AuthenticatedAgent,
    Json(req): Json<UpdateTrustAnchorRequest>,
) -> Result<Json<TrustAnchorResponse>, ApiError> {
    require_admin(&auth)?;
    if auth.tenant_id != tenant_id {
        return Err(ApiError::Forbidden(
            "cannot access trust anchors for a different tenant".to_string(),
        ));
    }
    let mut anchor = state
        .trust_anchors
        .find_by_id(&tenant_id, &anchor_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("trust anchor '{anchor_id}' not found")))?;

    if let Some(issuer) = req.issuer {
        anchor.issuer = issuer;
    }
    if let Some(jwks_uri) = req.jwks_uri {
        anchor.jwks_uri = jwks_uri;
    }
    if let Some(anchor_type) = req.anchor_type {
        anchor.anchor_type = anchor_type;
    }
    if let Some(constraints) = req.constraints {
        anchor.constraints = constraints;
    }

    state.trust_anchors.update(&tenant_id, &anchor).await?;

    info!(
        tenant_id = %tenant_id,
        anchor_id = %anchor_id,
        "trust_anchor.updated"
    );

    Ok(Json(anchor.into()))
}

/// DELETE /api/v1/tenants/:id/trust-anchors/:aid
pub async fn delete_trust_anchor(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, anchor_id)): Path<(String, String)>,
    auth: AuthenticatedAgent,
) -> Result<StatusCode, ApiError> {
    require_admin(&auth)?;
    if auth.tenant_id != tenant_id {
        return Err(ApiError::Forbidden(
            "cannot access trust anchors for a different tenant".to_string(),
        ));
    }

    // Verify it exists before deleting.
    state
        .trust_anchors
        .find_by_id(&tenant_id, &anchor_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("trust anchor '{anchor_id}' not found")))?;

    state.trust_anchors.delete(&tenant_id, &anchor_id).await?;

    info!(
        tenant_id = %tenant_id,
        anchor_id = %anchor_id,
        "trust_anchor.deleted"
    );

    Ok(StatusCode::NO_CONTENT)
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn require_admin(auth: &AuthenticatedAgent) -> Result<(), ApiError> {
    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
        return Err(ApiError::Forbidden(
            "trust anchor management requires Admin role".to_string(),
        ));
    }
    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, routing::get, Router};
    use http::{Request, StatusCode};
    use std::sync::Arc;
    use tower::ServiceExt;

    fn app() -> (Router, Arc<crate::AppState>) {
        let state = test_state();
        let router = Router::new()
            .route(
                "/api/v1/tenants/:id/trust-anchors",
                get(super::list_trust_anchors).post(super::create_trust_anchor),
            )
            .route(
                "/api/v1/tenants/:id/trust-anchors/:aid",
                get(super::get_trust_anchor)
                    .put(super::update_trust_anchor)
                    .delete(super::delete_trust_anchor),
            )
            .with_state(state.clone());
        (router, state)
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn trust_anchor_crud_lifecycle() {
        let (app, _state) = app();

        // List empty
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tenants/default/trust-anchors")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 0);

        // Create
        let body = serde_json::json!({
            "id": "tenant-keycloak",
            "issuer": "https://keycloak.example.com",
            "jwks_uri": "https://keycloak.example.com/.well-known/jwks.json",
            "anchor_type": "user"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tenants/default/trust-anchors")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["id"], "tenant-keycloak");
        assert_eq!(json["anchor_type"], "user");

        // Get by ID
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tenants/default/trust-anchors/tenant-keycloak")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["issuer"], "https://keycloak.example.com");

        // Update
        let update_body = serde_json::json!({
            "jwks_uri": "https://keycloak.example.com/v2/jwks"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/tenants/default/trust-anchors/tenant-keycloak")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_vec(&update_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["jwks_uri"], "https://keycloak.example.com/v2/jwks");

        // List should have 1
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tenants/default/trust-anchors")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);

        // Delete
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/tenants/default/trust-anchors/tenant-keycloak")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // List should be empty again
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tenants/default/trust-anchors")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn create_duplicate_trust_anchor_fails() {
        let (app, _state) = app();

        let body = serde_json::json!({
            "id": "dup-anchor",
            "issuer": "https://idp.example.com",
            "jwks_uri": "https://idp.example.com/jwks",
            "anchor_type": "addon"
        });

        // First create succeeds.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tenants/default/trust-anchors")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Second create fails.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tenants/default/trust-anchors")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn get_nonexistent_trust_anchor_returns_404() {
        let (app, _state) = app();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tenants/default/trust-anchors/no-such")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn trust_anchors_cross_tenant_returns_403() {
        let (app, _state) = app();

        // Create in own tenant (default).
        let body = serde_json::json!({
            "id": "scoped-anchor",
            "issuer": "https://idp.example.com",
            "jwks_uri": "https://idp.example.com/jwks",
            "anchor_type": "user"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tenants/default/trust-anchors")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Attempt to list trust anchors for a different tenant — must return 403.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tenants/other-tenant/trust-anchors")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Attempt to get a specific anchor from a different tenant — must return 403.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tenants/other-tenant/trust-anchors/scoped-anchor")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Attempt to delete from a different tenant — must return 403.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/tenants/other-tenant/trust-anchors/scoped-anchor")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
