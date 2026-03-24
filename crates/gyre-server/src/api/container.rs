//! M19.3: Container audit API — `GET /api/v1/agents/{id}/container`.

use axum::{
    extract::{Path, State},
    Json,
};
use std::sync::Arc;

use crate::{container_audit::ContainerAuditRecord, AppState};

use super::error::ApiError;

/// GET /api/v1/agents/{id}/container
///
/// Returns the container audit record for the given agent.  Returns 404 if the
/// agent was not spawned via a container target or has no audit record yet.
pub async fn get_agent_container(
    State(state): State<Arc<AppState>>,
    Path(agent_id): Path<String>,
) -> Result<Json<ContainerAuditRecord>, ApiError> {
    state
        .container_audits
        .find_by_agent_id(&agent_id)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("no container audit record for agent {agent_id}")))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
    async fn get_container_not_found_for_unknown_agent() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/agents/no-such-agent/container")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_container_returns_record_after_insert() {
        let state = test_state();
        let rec = crate::container_audit::ContainerAuditRecord {
            agent_id: "agent-x".to_string(),
            container_id: "ctr-abc".to_string(),
            image: "alpine:latest".to_string(),
            image_hash: Some("sha256:dead".to_string()),
            runtime: "docker".to_string(),
            started_at: 1000,
            stopped_at: None,
            exit_code: None,
        };
        state.container_audits.save(&rec).await.unwrap();

        let app = crate::api::api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/agents/agent-x/container")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["container_id"], "ctr-abc");
        assert_eq!(json["image"], "alpine:latest");
        assert_eq!(json["image_hash"], "sha256:dead");
        assert_eq!(json["runtime"], "docker");
    }
}
