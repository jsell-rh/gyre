use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{ComputeTargetEntity, ComputeTargetType};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;
use gyre_domain::UserRole;

use super::error::ApiError;
use super::{new_id, now_secs};

// ── DTOs ──────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateComputeTargetRequest {
    pub name: String,
    pub target_type: String,
    pub config: Option<serde_json::Value>,
    pub is_default: Option<bool>,
}

#[derive(Deserialize)]
pub struct UpdateComputeTargetRequest {
    pub name: Option<String>,
    pub config: Option<serde_json::Value>,
    pub is_default: Option<bool>,
}

#[derive(Serialize)]
pub struct ComputeTargetResponse {
    pub id: String,
    pub tenant_id: String,
    pub name: String,
    pub target_type: String,
    pub config: serde_json::Value,
    pub is_default: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<ComputeTargetEntity> for ComputeTargetResponse {
    fn from(ct: ComputeTargetEntity) -> Self {
        Self {
            id: ct.id.to_string(),
            tenant_id: ct.tenant_id.to_string(),
            name: ct.name,
            target_type: ct.target_type.to_string(),
            config: ct.config,
            is_default: ct.is_default,
            created_at: ct.created_at,
            updated_at: ct.updated_at,
        }
    }
}

fn parse_target_type(s: &str) -> Result<ComputeTargetType, ApiError> {
    ComputeTargetType::from_db_str(s).ok_or_else(|| {
        ApiError::InvalidInput(format!(
            "invalid target_type '{s}'; must be Container, Ssh, or Kubernetes"
        ))
    })
}

/// Extract tenant_id from the authenticated agent's JWT.
fn tenant_id_from_auth(auth: &crate::auth::AuthenticatedAgent) -> Id {
    Id::new(&auth.tenant_id)
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// GET /api/v1/compute-targets — list targets for the authenticated user's tenant.
pub async fn list_compute_targets(
    auth: crate::auth::AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<ComputeTargetResponse>>, ApiError> {
    let tenant_id = tenant_id_from_auth(&auth);
    let targets = state.compute_targets.list_by_tenant(&tenant_id).await?;
    Ok(Json(
        targets
            .into_iter()
            .map(ComputeTargetResponse::from)
            .collect(),
    ))
}

/// POST /api/v1/compute-targets — create a new compute target (Tenant Admin only).
pub async fn create_compute_target(
    auth: crate::auth::AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateComputeTargetRequest>,
) -> Result<(StatusCode, Json<ComputeTargetResponse>), ApiError> {
    if !auth.roles.contains(&UserRole::Admin) {
        return Err(ApiError::Forbidden(
            "compute target management requires Admin role".to_string(),
        ));
    }
    let tenant_id = tenant_id_from_auth(&auth);
    let target_type = parse_target_type(&req.target_type)?;
    let now = now_secs();
    let mut ct = ComputeTargetEntity::new(new_id(), tenant_id.clone(), req.name, target_type, now);
    if let Some(cfg) = req.config {
        ct.config = cfg;
    }
    if let Some(is_default) = req.is_default {
        ct.is_default = is_default;
    }
    // Guard duplicate name — the DB UNIQUE constraint would return 500 otherwise.
    let existing = state.compute_targets.list_by_tenant(&tenant_id).await?;
    if existing.iter().any(|t| t.name == ct.name) {
        return Err(ApiError::Conflict(format!(
            "a compute target named '{}' already exists in this tenant",
            ct.name
        )));
    }
    state.compute_targets.create(&ct).await?;
    Ok((StatusCode::CREATED, Json(ComputeTargetResponse::from(ct))))
}

/// GET /api/v1/compute-targets/:id — fetch a single compute target.
pub async fn get_compute_target(
    auth: crate::auth::AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ComputeTargetResponse>, ApiError> {
    let tenant_id = tenant_id_from_auth(&auth);
    let ct = state
        .compute_targets
        .get_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("compute target {id} not found")))?;
    if ct.tenant_id != tenant_id {
        return Err(ApiError::NotFound(format!("compute target {id} not found")));
    }
    Ok(Json(ComputeTargetResponse::from(ct)))
}

/// PUT /api/v1/compute-targets/:id — update config/name/is_default (Tenant Admin only).
pub async fn update_compute_target(
    auth: crate::auth::AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateComputeTargetRequest>,
) -> Result<Json<ComputeTargetResponse>, ApiError> {
    if !auth.roles.contains(&UserRole::Admin) {
        return Err(ApiError::Forbidden(
            "compute target management requires Admin role".to_string(),
        ));
    }
    let tenant_id = tenant_id_from_auth(&auth);
    let mut ct = state
        .compute_targets
        .get_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("compute target {id} not found")))?;
    if ct.tenant_id != tenant_id {
        return Err(ApiError::NotFound(format!("compute target {id} not found")));
    }
    if let Some(name) = req.name {
        ct.name = name;
    }
    if let Some(cfg) = req.config {
        ct.config = cfg;
    }
    if let Some(is_default) = req.is_default {
        ct.is_default = is_default;
    }
    ct.updated_at = now_secs();
    state.compute_targets.update(&ct).await?;
    Ok(Json(ComputeTargetResponse::from(ct)))
}

/// DELETE /api/v1/compute-targets/:id — delete (409 if workspaces reference it; Tenant Admin only).
pub async fn delete_compute_target(
    auth: crate::auth::AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    if !auth.roles.contains(&UserRole::Admin) {
        return Err(ApiError::Forbidden(
            "compute target management requires Admin role".to_string(),
        ));
    }
    let tenant_id = tenant_id_from_auth(&auth);
    let ct = state
        .compute_targets
        .get_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("compute target {id} not found")))?;
    if ct.tenant_id != tenant_id {
        return Err(ApiError::NotFound(format!("compute target {id} not found")));
    }
    if state
        .compute_targets
        .has_workspace_references(&Id::new(&id))
        .await?
    {
        return Err(ApiError::Conflict(format!(
            "compute target {id} is referenced by one or more workspaces"
        )));
    }
    state.compute_targets.delete(&Id::new(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        crate::api::api_router().with_state(test_state())
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn create_and_list_compute_targets() {
        let app = app();
        let body = serde_json::json!({
            "name": "my-cluster",
            "target_type": "Kubernetes",
            "config": { "kubeconfig": "/etc/kube/config" }
        });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/compute-targets")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let created = body_json(create_resp).await;
        assert_eq!(created["name"], "my-cluster");
        assert_eq!(created["target_type"], "Kubernetes");
        assert_eq!(created["config"]["kubeconfig"], "/etc/kube/config");

        let list_resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/compute-targets")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list_resp.status(), StatusCode::OK);
        let list = body_json(list_resp).await;
        assert_eq!(list.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn invalid_target_type_rejected() {
        let body = serde_json::json!({ "name": "bad", "target_type": "docker" });
        let resp = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/compute-targets")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn get_compute_target_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/compute-targets/no-such")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn update_compute_target() {
        let app = app();
        let body = serde_json::json!({ "name": "orig", "target_type": "Ssh" });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/compute-targets")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let created = body_json(create_resp).await;
        let id = created["id"].as_str().unwrap().to_string();

        let update_body = serde_json::json!({ "name": "renamed", "is_default": true });
        let update_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/compute-targets/{id}"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&update_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(update_resp.status(), StatusCode::OK);
        let updated = body_json(update_resp).await;
        assert_eq!(updated["name"], "renamed");
        assert_eq!(updated["is_default"], true);
    }

    #[tokio::test]
    async fn non_admin_cannot_create_compute_target() {
        let state = test_state();
        // Register a non-admin agent token.
        state
            .kv_store
            .kv_set(
                "agent_tokens",
                "non-admin-agent",
                "agent-tok-ct-1".to_string(),
            )
            .await
            .unwrap();
        let body = serde_json::json!({ "name": "no", "target_type": "Container" });
        let resp = crate::api::api_router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/compute-targets")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer agent-tok-ct-1")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn duplicate_name_returns_conflict() {
        let app = app();
        let body = serde_json::json!({ "name": "dup-target", "target_type": "Container" });
        let r1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/compute-targets")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r1.status(), StatusCode::CREATED);

        let r2 = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/compute-targets")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r2.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn delete_compute_target() {
        let app = app();
        let body = serde_json::json!({ "name": "to-delete", "target_type": "Container" });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/compute-targets")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let created = body_json(create_resp).await;
        let id = created["id"].as_str().unwrap().to_string();

        let del_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/compute-targets/{id}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);

        let get_resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/compute-targets/{id}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_resp.status(), StatusCode::NOT_FOUND);
    }
}
