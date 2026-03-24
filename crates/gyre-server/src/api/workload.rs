//! G10: GET /api/v1/agents/{id}/workload — query workload attestation status.

use axum::{
    extract::{Path, State},
    Json,
};
use std::sync::Arc;

use crate::{auth::AuthenticatedAgent, workload_attestation::WorkloadAttestation, AppState};

use super::error::ApiError;

/// GET /api/v1/agents/{id}/workload
///
/// Returns the current workload attestation record for an agent.
/// The attestation is created on spawn and updated on every heartbeat.
/// Returns 404 if the agent has no attestation (e.g. spawned before G10).
pub async fn get_workload(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<Json<WorkloadAttestation>, ApiError> {
    state
        .kv_store
        .kv_get("workload_attestations", &id)
        .await
        .map_err(ApiError::Internal)?
        .and_then(|s| serde_json::from_str::<WorkloadAttestation>(&s).ok())
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("no workload attestation for agent {id}")))
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

    #[tokio::test]
    async fn get_workload_missing_returns_404() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/agents/no-such-agent/workload")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_workload_returns_attestation() {
        use crate::workload_attestation::attest_agent;

        let state = test_state();
        let att = attest_agent("agent-test", Some(1234), "local", "sha256:abc");
        let json = serde_json::to_string(&att).unwrap();
        state
            .kv_store
            .kv_set("workload_attestations", "agent-test", json)
            .await
            .unwrap();

        let app = crate::api::api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/agents/agent-test/workload")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["agent_id"], "agent-test");
        assert_eq!(json["pid"], 1234);
        assert_eq!(json["compute_target"], "local");
        assert_eq!(json["stack_fingerprint"], "sha256:abc");
        assert_eq!(json["alive"], true);
    }
}
