//! Per-repo spec lifecycle configuration.
//!
//! GET  /api/v1/repos/:id/spec-lifecycle  — get current config
//! PUT  /api/v1/repos/:id/spec-lifecycle  — set config (Admin/Developer)

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use std::sync::Arc;

use crate::{auth::AuthenticatedAgent, AppState};

use super::error::ApiError;

pub use gyre_domain::SpecLifecycleConfig;

/// GET /api/v1/repos/:id/spec-lifecycle — get the repo's spec lifecycle config.
///
/// Returns defaults when none has been configured.
pub async fn get_spec_lifecycle(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<Json<SpecLifecycleConfig>, ApiError> {
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    let config = state
        .spec_lifecycle_configs
        .get_for_repo(&repo_id)
        .await?;

    Ok(Json(config))
}

/// PUT /api/v1/repos/:id/spec-lifecycle — set the repo's spec lifecycle config.
///
/// Admin and Developer roles only: the config controls which spec paths
/// trigger automatic task creation and approval invalidation on push.
/// Letting the Agent role weaken it (e.g. disabling the hook or removing
/// watched paths) would let agents push spec changes that go unnoticed —
/// the exact drift spec-lifecycle.md exists to prevent.
pub async fn set_spec_lifecycle(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(repo_id): Path<String>,
    Json(req): Json<SpecLifecycleConfig>,
) -> Result<(StatusCode, Json<SpecLifecycleConfig>), ApiError> {
    if !(auth.roles.contains(&gyre_domain::UserRole::Admin)
        || auth.roles.contains(&gyre_domain::UserRole::Developer))
    {
        return Err(ApiError::Forbidden(
            "only Admin or Developer role may update spec lifecycle config".to_string(),
        ));
    }

    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    state
        .spec_lifecycle_configs
        .set_for_repo(&repo_id, req.clone())
        .await?;

    Ok((StatusCode::OK, Json(req)))
}

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use gyre_domain::{Repository, TaskPriority};
    use http::{Method, Request, StatusCode};
    use tower::ServiceExt;
    use std::future::Future;

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

    fn put(
        app: &Router,
        uri: &str,
        body: serde_json::Value,
    ) -> impl Future<Output = axum::response::Response> {
        let req = Request::builder()
            .method(Method::PUT)
            .uri(uri)
            .header("content-type", "application/json")
            .header("authorization", "Bearer test-token")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        app.clone().oneshot(req)
    }

    async fn get(app: &Router, uri: &str) -> axum::response::Response {
        let req = Request::builder()
            .uri(uri)
            .header("authorization", "Bearer test-token")
            .body(Body::empty())
            .unwrap();
        app.clone().oneshot(req).await.unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_returns_spec_defaults_for_unconfigured_repo() {
        let (app, _state) = app_with_repo();
        let resp = get(&app, "/api/v1/repos/repo-1/spec-lifecycle").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json["enabled"].as_bool().unwrap());
        let watched: Vec<&str> = json["watched_paths"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(watched, vec!["specs/system/", "specs/development/"]);
        assert_eq!(json["default_priority_new"].as_str().unwrap(), "Medium");
        assert_eq!(json["default_priority_modified"].as_str().unwrap(), "High");
        assert_eq!(json["default_priority_deleted"].as_str().unwrap(), "High");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn put_round_trips_custom_config() {
        let (app, state) = app_with_repo();
        let body = serde_json::json!({
            "enabled": false,
            "watched_paths": ["docs/specs/"],
            "ignored_paths": ["docs/specs/archive/"],
            "auto_invalidate_approvals": false,
            "dedup_open_tasks": false,
            "default_priority_new": "Critical",
            "default_priority_modified": "Low",
            "default_priority_deleted": "Medium",
        });
        let resp = put(&app, "/api/v1/repos/repo-1/spec-lifecycle", body).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(!json["enabled"].as_bool().unwrap());

        // GET reflects persisted value through the same port the push hook uses.
        let resp = get(&app, "/api/v1/repos/repo-1/spec-lifecycle").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(!json["enabled"].as_bool().unwrap());
        assert_eq!(
            json["watched_paths"][0].as_str().unwrap(),
            "docs/specs/"
        );
        assert_eq!(json["default_priority_new"].as_str().unwrap(), "Critical");

        // And through the store directly (what git_http.rs reads).
        let cfg = state
            .spec_lifecycle_configs
            .get_for_repo("repo-1")
            .await
            .unwrap();
        assert!(!cfg.enabled);
        assert_eq!(cfg.watched_paths, vec!["docs/specs/".to_string()]);
        assert_eq!(cfg.default_priority_new, TaskPriority::Critical);
        assert_eq!(cfg.default_priority_modified, TaskPriority::Low);
        assert_eq!(cfg.default_priority_deleted, TaskPriority::Medium);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn put_partial_body_applies_serde_defaults() {
        // A payload with only `enabled` must not wipe other settings to
        // zero-values: serde defaults keep the spec baseline.
        let (app, _state) = app_with_repo();
        let body = serde_json::json!({"enabled": true});
        let resp = put(&app, "/api/v1/repos/repo-1/spec-lifecycle", body).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(
            json["watched_paths"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["specs/system/", "specs/development/"]
        );
        assert!(json["auto_invalidate_approvals"].as_bool().unwrap());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn put_unknown_repo_404s() {
        let (app, _state) = app_with_repo();
        let resp = put(
            &app,
            "/api/v1/repos/no-such/spec-lifecycle",
            serde_json::json!({"enabled": true}),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn put_agent_role_forbidden() {
        // Agent role must not be able to disable the lifecycle hook:
        // that would let agents push spec changes that go unnoticed.
        use crate::abac_middleware::seed_builtin_policies;
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        let state = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(seed_builtin_policies(&state))
        });

        let repo = Repository::new(
            gyre_common::Id::new("repo-jwt"),
            gyre_common::Id::new("proj-1"),
            "jwt-repo",
            "/tmp/jwt-repo",
            0,
        );
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(state.repos.create(&repo))
                .unwrap();
        });

        let agent_token = sign_test_jwt(
            &serde_json::json!({
                "sub": "rogue-agent",
                "preferred_username": "rogue-agent",
                "realm_access": { "roles": ["agent"] }
            }),
            3600,
        );

        let body = serde_json::json!({"enabled": false});
        let resp = crate::api::api_router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri("/api/v1/repos/repo-jwt/spec-lifecycle")
                    .header("authorization", format!("Bearer {agent_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
