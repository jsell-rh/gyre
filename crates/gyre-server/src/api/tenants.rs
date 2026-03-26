use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{BudgetConfig, Tenant};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{auth::AuthenticatedAgent, AppState};

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
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Json(req): Json<CreateTenantRequest>,
) -> Result<(StatusCode, Json<TenantResponse>), ApiError> {
    // Admin-only: tenants are the top-level isolation boundary. Allowing
    // non-Admin callers to create tenants would let Developer/Agent roles
    // provision new top-level isolation contexts (NEW-29).
    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
        return Err(ApiError::Forbidden(
            "only Admin role may create tenants".to_string(),
        ));
    }
    let now = now_secs();
    let mut t = Tenant::new(new_id(), req.name, req.slug, now);
    t.oidc_issuer = req.oidc_issuer;
    t.budget = req.budget;
    t.max_workspaces = req.max_workspaces;
    state.tenants.create(&t).await?;
    Ok((StatusCode::CREATED, Json(TenantResponse::from(t))))
}

pub async fn list_tenants(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<TenantResponse>>, ApiError> {
    let tenants = state.tenants.list().await?;
    Ok(Json(
        tenants.into_iter().map(TenantResponse::from).collect(),
    ))
}

pub async fn get_tenant(
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
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
    Json(req): Json<UpdateTenantRequest>,
) -> Result<Json<TenantResponse>, ApiError> {
    // Admin-only: updating tenant OIDC issuer, budget limits, or workspace caps
    // are governance-level changes that must not be accessible to Developer/Agent
    // roles (NEW-29).
    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
        return Err(ApiError::Forbidden(
            "only Admin role may update tenants".to_string(),
        ));
    }
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
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    // Admin-only: deleting a tenant destroys all its workspaces, repos, and
    // agent sessions. Restrict to Admin role only (NEW-29).
    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
        return Err(ApiError::Forbidden(
            "only Admin role may delete tenants".to_string(),
        ));
    }
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

    #[tokio::test(flavor = "multi_thread")]
    async fn developer_cannot_create_or_update_tenant() {
        // NEW-29: Developer-role JWT must be rejected with 403 for POST/PUT/DELETE.
        use crate::abac_middleware::seed_builtin_policies;
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        let state = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(seed_builtin_policies(&state))
        });

        let dev_token = sign_test_jwt(
            &serde_json::json!({
                "sub": "dev-sub",
                "preferred_username": "developer-user",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );

        // POST /api/v1/tenants — Developer should get 403.
        let post_resp = crate::api::api_router()
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tenants")
                    .header("authorization", format!("Bearer {dev_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"evil","slug":"evil"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(post_resp.status(), StatusCode::FORBIDDEN);

        // PUT /api/v1/tenants/:id — use admin to create a tenant first.
        let create_resp = crate::api::api_router()
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tenants")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"legit","slug":"legit"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let created = body_json(create_resp).await;
        let tid = created["id"].as_str().unwrap().to_string();

        let put_resp = crate::api::api_router()
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/tenants/{tid}"))
                    .header("authorization", format!("Bearer {dev_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"hijacked"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_resp.status(), StatusCode::FORBIDDEN);

        // DELETE /api/v1/tenants/:id — Developer should get 403.
        let del_resp = crate::api::api_router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/tenants/{tid}"))
                    .header("authorization", format!("Bearer {dev_token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_resp.status(), StatusCode::FORBIDDEN);
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
