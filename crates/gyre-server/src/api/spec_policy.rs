//! Per-repo spec enforcement policy.
//!
//! GET  /api/v1/repos/:id/spec-policy  — get current policy
//! PUT  /api/v1/repos/:id/spec-policy  — set policy (AdminOnly)

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use serde::Serialize;
use std::sync::Arc;

use crate::AppState;

use super::error::ApiError;

pub use gyre_domain::SpecPolicy;

#[derive(Serialize)]
pub struct SpecPolicyResponse {
    pub repo_id: String,
    pub require_spec_ref: bool,
    pub require_approved_spec: bool,
    pub warn_stale_spec: bool,
    pub require_current_spec: bool,
}

impl SpecPolicyResponse {
    fn from_policy(repo_id: String, policy: &SpecPolicy) -> Self {
        SpecPolicyResponse {
            repo_id,
            require_spec_ref: policy.require_spec_ref,
            require_approved_spec: policy.require_approved_spec,
            warn_stale_spec: policy.warn_stale_spec,
            require_current_spec: policy.require_current_spec,
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/repos/:id/spec-policy — get current spec enforcement policy.
pub async fn get_spec_policy(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<Json<SpecPolicyResponse>, ApiError> {
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let policy = state
        .spec_policies
        .get_for_repo(&repo_id)
        .await
        .unwrap_or_default();

    Ok(Json(SpecPolicyResponse::from_policy(repo_id, &policy)))
}

/// PUT /api/v1/repos/:id/spec-policy — set spec enforcement policy (AdminOnly).
pub async fn set_spec_policy(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
    Json(req): Json<SpecPolicy>,
) -> Result<(StatusCode, Json<SpecPolicyResponse>), ApiError> {
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    state
        .spec_policies
        .set_for_repo(&repo_id, req.clone())
        .await?;

    Ok((
        StatusCode::OK,
        Json(SpecPolicyResponse::from_policy(repo_id, &req)),
    ))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use gyre_domain::Repository;
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app_with_repo() -> (Router, std::sync::Arc<crate::AppState>) {
        let state = test_state();
        let repo = Repository::new(
            gyre_common::Id::new("repo-1"),
            gyre_common::Id::new("proj-1"),
            "test-repo",
            "/tmp/test-repo",
            0,
        );
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(state.repos.create(&repo))
                .unwrap();
        });
        let app = crate::api::api_router().with_state(state.clone());
        (app, state)
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_spec_policy_defaults_to_false() {
        let (app, _state) = app_with_repo();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/repo-1/spec-policy")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(!json["require_spec_ref"].as_bool().unwrap());
        assert!(!json["require_approved_spec"].as_bool().unwrap());
        assert!(!json["warn_stale_spec"].as_bool().unwrap());
        assert!(!json["require_current_spec"].as_bool().unwrap());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn set_warn_stale_spec_round_trips() {
        let (app, _state) = app_with_repo();
        let body = serde_json::json!({
            "require_spec_ref": false,
            "require_approved_spec": false,
            "warn_stale_spec": true,
            "require_current_spec": false
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/repos/repo-1/spec-policy")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json["warn_stale_spec"].as_bool().unwrap());
        assert!(!json["require_current_spec"].as_bool().unwrap());

        // GET reflects persisted value.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/repo-1/spec-policy")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json["warn_stale_spec"].as_bool().unwrap());
        assert!(!json["require_current_spec"].as_bool().unwrap());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn set_require_current_spec_round_trips() {
        let (app, _state) = app_with_repo();
        let body = serde_json::json!({
            "require_spec_ref": false,
            "require_approved_spec": false,
            "warn_stale_spec": false,
            "require_current_spec": true
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/repos/repo-1/spec-policy")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(!json["warn_stale_spec"].as_bool().unwrap());
        assert!(json["require_current_spec"].as_bool().unwrap());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn new_fields_default_when_omitted_from_put() {
        let (app, _state) = app_with_repo();
        // Older client sends only the original two fields — new fields must default to false.
        let body = serde_json::json!({
            "require_spec_ref": true,
            "require_approved_spec": false
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/repos/repo-1/spec-policy")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json["require_spec_ref"].as_bool().unwrap());
        assert!(!json["warn_stale_spec"].as_bool().unwrap());
        assert!(!json["require_current_spec"].as_bool().unwrap());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn set_then_get_spec_policy() {
        let (app, _state) = app_with_repo();
        let body = serde_json::json!({
            "require_spec_ref": true,
            "require_approved_spec": false
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/repos/repo-1/spec-policy")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json["require_spec_ref"].as_bool().unwrap());
        assert!(!json["require_approved_spec"].as_bool().unwrap());

        // GET returns same values.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/repo-1/spec-policy")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json["require_spec_ref"].as_bool().unwrap());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn set_spec_policy_unknown_repo_returns_404() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state);
        let body = serde_json::json!({
            "require_spec_ref": true,
            "require_approved_spec": true
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/repos/no-such/spec-policy")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
