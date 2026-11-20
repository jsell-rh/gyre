//! Workload attestation surface (identity-security.md Layer 1):
//!
//! - GET  /api/v1/agents/{id}/workload — query the SVID-backed workload
//!   attestation record.
//! - POST /api/v1/agents/{id}/attest — present the session's X.509-SVID
//!   for workload-identity verification (fail-closed chain validation
//!   against the published trust bundle, plus stack-fingerprint binding).

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::{auth::AuthenticatedAgent, workload_attestation::WorkloadAttestation, AppState};

use super::error::ApiError;

/// GET /api/v1/agents/{id}/workload
///
/// Returns the current workload attestation record for an agent.
/// The record is created on spawn (with the session's SVID identity) and
/// updated by the workload-identity verification surface (attest /
/// heartbeat). Returns 404 if the agent has no attestation record.
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

/// POST /api/v1/agents/{id}/attest request body: the workload presents its
/// SVID (PEM chain, leaf first — the `GYRE_SVID_PEM` environment variable)
/// and its current stack fingerprint.
#[derive(Debug, Deserialize)]
pub struct AttestRequest {
    pub svid: String,
    #[serde(default)]
    pub stack_fingerprint: String,
}

/// POST /api/v1/agents/{id}/attest
///
/// The Layer-1 workload-identity verification surface: validates the
/// presented SVID's chain against the published trust bundle, enforces
/// certificate validity periods, binds the SPIFFE ID to this agent
/// session, and binds the presented stack fingerprint to the SVID-backed
/// attestation record. Fail-closed: 403 with the rejection reason on any
/// failed check (no SVID presented, untrusted signer, expired/not-yet
/// valid, foreign trust domain, different session, empty/tampered stack).
pub async fn attest_workload(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Path(id): Path<String>,
    Json(req): Json<AttestRequest>,
) -> Result<(StatusCode, Json<crate::spiffe::WorkloadVerification>), ApiError> {
    // The agent must exist (404 for an unknown session).
    state
        .agents
        .find_by_id(&gyre_common::Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("agent {id} not found")))?;

    match crate::spiffe::attest_session(&state, &id, &req.svid, &req.stack_fingerprint).await {
        Ok(verification) => Ok((StatusCode::OK, Json(verification))),
        Err(reason) => Err(ApiError::Forbidden(format!(
            "workload-identity verification failed: {reason}"
        ))),
    }
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
        // identity-security.md Layer 1: `alive` is the workload-identity
        // verification result, not a /proc probe — a freshly seeded
        // metadata record has NOT been verified, so it reads false until
        // an SVID presentation verifies (see attest tests below).
        assert_eq!(json["alive"], false);
        assert!(json["spiffe_id"].is_null(), "no SVID presentation yet");
        assert!(json["svid_serial"].is_null(), "no SVID presentation yet");
    }

    async fn create_agent(app: &Router, name: &str) -> String {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents")
                    .header("authorization", "Bearer test-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({ "name": name })).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    async fn post_attest(
        app: &Router,
        id: &str,
        body: serde_json::Value,
    ) -> axum::response::Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/agents/{id}/attest"))
                    .header("content-type", "application/json")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn attest_workload_verifies_session_svid() {
        // identity-security.md Layer 1 happy path: a workload presents the
        // SVID issued for its session; the chain validates against the
        // published trust bundle, the SPIFFE ID matches the session, and
        // the record flips to verified.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let id = create_agent(&app, "attest-agent").await;
        let svid = crate::spiffe::issue_session_svid(
            &state,
            &id,
            "test-persona",
            "local",
            "sha256:test-stack-fingerprint",
        )
        .await
        .expect("test SVID issuance must succeed");

        // Seed the spawn-time metadata record (as the spawn path writes).
        let att = crate::workload_attestation::attest_agent_with_container(
            &id,
            Some(4242),
            "local",
            "sha256:test-stack-fingerprint",
            None,
            None,
            None,
            None,
        );
        state
            .kv_store
            .kv_set(
                "workload_attestations",
                &id,
                serde_json::to_string(&att).unwrap(),
            )
            .await
            .unwrap();

        let resp = post_attest(
            &app,
            &id,
            serde_json::json!({
                "svid": svid.record.chain_pem,
                "stack_fingerprint": "sha256:test-stack-fingerprint",
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            json["spiffe_id"],
            format!(
                "spiffe://gyre.example.com/agent/test-persona/session/{}",
                crate::auth::agent_subject(&id)
            )
        );

        // The attestation record now carries the verification result.
        let rec = state
            .kv_store
            .kv_get("workload_attestations", &id)
            .await
            .unwrap()
            .expect("attestation record");
        let rec: serde_json::Value = serde_json::from_str(&rec).unwrap();
        assert_eq!(rec["alive"], true);
        assert_eq!(
            rec["spiffe_id"],
            format!(
                "spiffe://gyre.example.com/agent/test-persona/session/{}",
                crate::auth::agent_subject(&id)
            )
        );
        assert!(rec["svid_serial"].is_string());
    }

    #[tokio::test]
    async fn attest_workload_rejects_untrusted_and_mismatched() {
        // Fail-closed probes: an SVID from a foreign trust domain, an SVID
        // minted for a different session, and a missing SVID are all
        // rejected with 403 — no bypass for unattested workloads.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());
        let id = create_agent(&app, "attest-reject-agent").await;

        // Foreign trust domain.
        let foreign_ca = gyre_spiffe::TrustDomainCa::generate("attacker.example")
            .expect("foreign CA generation must succeed");
        let foreign = foreign_ca
            .issue_svid(
                &gyre_spiffe::SpiffeId::parse(&format!(
                    "spiffe://attacker.example/agent/test-persona/session/{}",
                    crate::auth::agent_subject(&id)
                ))
                .expect("foreign SPIFFE ID must parse"),
                gyre_spiffe::BASE_SVID_TTL_SECS,
            )
            .expect("foreign SVID issuance must succeed");
        let resp = post_attest(
            &app,
            &id,
            serde_json::json!({
                "svid": foreign.chain_pem,
                "stack_fingerprint": "sha256:test-stack-fingerprint",
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Valid issuer but a different session.
        let other = create_agent(&app, "attest-other-agent").await;
        let other_svid = crate::spiffe::issue_session_svid(
            &state,
            &other,
            "test-persona",
            "local",
            "sha256:test-stack-fingerprint",
        )
        .await
        .expect("test SVID issuance must succeed");
        let resp = post_attest(
            &app,
            &id,
            serde_json::json!({
                "svid": other_svid.record.chain_pem,
                "stack_fingerprint": "sha256:test-stack-fingerprint",
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // No SVID at all.
        let resp = post_attest(
            &app,
            &id,
            serde_json::json!({ "stack_fingerprint": "sha256:test-stack-fingerprint" }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn attest_workload_unknown_agent_404() {
        let resp = post_attest(
            &app(),
            "no-such-agent",
            serde_json::json!({ "svid": "", "stack_fingerprint": "" }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn trust_bundle_served_at_well_known_location() {
        // The trust bundle (SPIFFE bundle format) is published at the
        // standard well-known location, unauthenticated — any verifier
        // must be able to check SVIDs (same model as the OIDC JWKS).
        let state = test_state();
        let app = crate::build_router(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(crate::spiffe::TRUST_BUNDLE_PATH)
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
        // JWK Set with at least one SVID anchor for the test trust domain.
        assert!(!json["keys"].as_array().unwrap().is_empty());
        assert_eq!(json["keys"][0]["use"], "svid");
        assert_eq!(json["keys"][0]["kty"], "EC");
    }
}
