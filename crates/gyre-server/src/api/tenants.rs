use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{BudgetConfig, Tenant};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

#[derive(Deserialize)]
pub struct CreateTenantRequest {
    pub name: String,
    pub slug: String,
    pub oidc_issuer: Option<String>,
    pub budget: Option<BudgetConfig>,
    pub max_workspaces: Option<u32>,
}

#[derive(Deserialize)]
pub struct UpdateTenantRequest {
    pub name: Option<String>,
    pub oidc_issuer: Option<String>,
    pub budget: Option<BudgetConfig>,
    pub max_workspaces: Option<u32>,
}

#[derive(Serialize)]
pub struct TenantResponse {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub oidc_issuer: Option<String>,
    pub budget: Option<BudgetConfig>,
    pub max_workspaces: Option<u32>,
    pub created_at: u64,
}

impl From<Tenant> for TenantResponse {
    fn from(t: Tenant) -> Self {
        Self {
            id: t.id.to_string(),
            name: t.name,
            slug: t.slug,
            oidc_issuer: t.oidc_issuer,
            budget: t.budget,
            max_workspaces: t.max_workspaces,
            created_at: t.created_at,
        }
    }
}

pub async fn create_tenant(
    _admin: crate::auth::AdminOnly,
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateTenantRequest>,
) -> Result<(StatusCode, Json<TenantResponse>), ApiError> {
    let now = now_secs();
    let mut t = Tenant::new(new_id(), req.name, req.slug, now);
    t.oidc_issuer = req.oidc_issuer;
    t.budget = req.budget;
    t.max_workspaces = req.max_workspaces;
    state.tenants.create(&t).await?;
    Ok((StatusCode::CREATED, Json(TenantResponse::from(t))))
}

pub async fn list_tenants(
    _admin: crate::auth::AdminOnly,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<TenantResponse>>, ApiError> {
    let tenants = state.tenants.list().await?;
    Ok(Json(
        tenants.into_iter().map(TenantResponse::from).collect(),
    ))
}

pub async fn get_tenant(
    _admin: crate::auth::AdminOnly,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<TenantResponse>, ApiError> {
    let t = state
        .tenants
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("tenant {id} not found")))?;
    Ok(Json(TenantResponse::from(t)))
}

pub async fn update_tenant(
    _admin: crate::auth::AdminOnly,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateTenantRequest>,
) -> Result<Json<TenantResponse>, ApiError> {
    let mut t = state
        .tenants
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("tenant {id} not found")))?;
    if let Some(name) = req.name {
        t.name = name;
    }
    if let Some(oidc_issuer) = req.oidc_issuer {
        t.oidc_issuer = Some(oidc_issuer);
    }
    if let Some(budget) = req.budget {
        t.budget = Some(budget);
    }
    if let Some(max_workspaces) = req.max_workspaces {
        t.max_workspaces = Some(max_workspaces);
    }
    state.tenants.update(&t).await?;
    Ok(Json(TenantResponse::from(t)))
}

pub async fn delete_tenant(
    _admin: crate::auth::AdminOnly,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .tenants
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("tenant {id} not found")))?;
    state.tenants.delete(&Id::new(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

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
    async fn create_and_list_tenants() {
        let app = app();
        let body = serde_json::json!({ "name": "Acme Corp", "slug": "acme-corp" });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tenants")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let created = body_json(create_resp).await;
        assert_eq!(created["name"], "Acme Corp");
        assert_eq!(created["slug"], "acme-corp");

        let list_resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tenants")
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
    async fn get_tenant_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tenants/nonexistent")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn update_tenant() {
        let app = app();
        let body = serde_json::json!({ "name": "Old Name", "slug": "old-name" });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tenants")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let created = body_json(create_resp).await;
        let id = created["id"].as_str().unwrap().to_string();

        let update_body = serde_json::json!({ "name": "New Name" });
        let update_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/tenants/{id}"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&update_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(update_resp.status(), StatusCode::OK);
        let updated = body_json(update_resp).await;
        assert_eq!(updated["name"], "New Name");
    }

    #[tokio::test]
    async fn delete_tenant() {
        let app = app();
        let body = serde_json::json!({ "name": "Del", "slug": "del" });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tenants")
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
                    .uri(format!("/api/v1/tenants/{id}"))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);
    }
}
