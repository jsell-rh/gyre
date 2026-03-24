//! Pre-accept push gate configuration per repository.
//!
//! GET  /api/v1/repos/:id/push-gates  — list configured gate names for the repo
//! PUT  /api/v1/repos/:id/push-gates  — set gate list (replaces existing)

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

use super::error::ApiError;

// ---------------------------------------------------------------------------
// Request / Response
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SetPushGatesRequest {
    /// Ordered list of gate names to enable for this repo.
    /// Use an empty list to disable all pre-accept gates.
    /// Known names: "conventional-commit", "task-ref", "no-em-dash"
    pub gates: Vec<String>,
}

#[derive(Serialize)]
pub struct PushGatesResponse {
    pub repo_id: String,
    pub gates: Vec<String>,
    /// Gate names that are registered in the server's gate registry.
    pub available: Vec<String>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/repos/:id/push-gates — list configured push-gate names.
pub async fn get_push_gates(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<Json<PushGatesResponse>, ApiError> {
    // Validate repo exists.
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let gates = state
        .repo_push_gates
        .get_for_repo(&repo_id)
        .await
        .unwrap_or_default();

    let available: Vec<String> = state
        .push_gate_registry
        .iter()
        .map(|g| g.name().to_string())
        .collect();

    Ok(Json(PushGatesResponse {
        repo_id,
        gates,
        available,
    }))
}

/// PUT /api/v1/repos/:id/push-gates — set pre-accept gate list for this repo.
pub async fn set_push_gates(
    State(state): State<Arc<AppState>>,
    _admin: crate::auth::AdminOnly,
    Path(repo_id): Path<String>,
    Json(req): Json<SetPushGatesRequest>,
) -> Result<(StatusCode, Json<PushGatesResponse>), ApiError> {
    // Validate repo exists.
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    // Validate that requested gates are known.
    let available: Vec<String> = state
        .push_gate_registry
        .iter()
        .map(|g| g.name().to_string())
        .collect();

    let unknown: Vec<&str> = req
        .gates
        .iter()
        .filter(|name| !available.contains(name))
        .map(|s| s.as_str())
        .collect();

    if !unknown.is_empty() {
        return Err(ApiError::InvalidInput(format!(
            "unknown gate name(s): {}. Available: {}",
            unknown.join(", "),
            available.join(", ")
        )));
    }

    state
        .repo_push_gates
        .set_for_repo(&repo_id, req.gates.clone())
        .await?;

    Ok((
        StatusCode::OK,
        Json(PushGatesResponse {
            repo_id,
            gates: req.gates,
            available,
        }),
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
        // Block on insert for test setup.
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
    async fn get_push_gates_initially_empty() {
        let (app, _state) = app_with_repo();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/repo-1/push-gates")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["gates"].as_array().unwrap().len(), 0);
        // Available should list all three built-in gates.
        let available = json["available"].as_array().unwrap();
        assert!(available.len() >= 3);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn set_push_gates_then_get() {
        let (app, _state) = app_with_repo();

        let body = serde_json::json!({ "gates": ["conventional-commit", "no-em-dash"] });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/repos/repo-1/push-gates")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let gates = json["gates"].as_array().unwrap();
        assert_eq!(gates.len(), 2);
        assert_eq!(gates[0].as_str().unwrap(), "conventional-commit");

        // GET returns same list.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/repo-1/push-gates")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["gates"].as_array().unwrap().len(), 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn set_push_gates_unknown_gate_returns_400() {
        let (app, _state) = app_with_repo();

        let body = serde_json::json!({ "gates": ["unknown-gate"] });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/repos/repo-1/push-gates")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn set_push_gates_unknown_repo_returns_404() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state);

        let body = serde_json::json!({ "gates": [] });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/repos/no-such-repo/push-gates")
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
