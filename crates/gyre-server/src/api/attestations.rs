//! Merge attestation bundle verification endpoint
//! (agent-gates.md §Part 3 Verification).

use axum::{extract::State, Json};
use gyre_domain::attestation_verification::VerificationResult;
use gyre_domain::merge_attestation::MergeAttestationBundle;
use std::sync::Arc;

use crate::api::error::ApiError;
use crate::AppState;

/// POST /api/v1/attestations/verify
///
/// Accepts a `MergeAttestationBundle` JSON document and runs the full
/// verification protocol over it, returning the per-component
/// `VerificationResult`:
///
/// - `bundle_signature_valid` — the forge signature over the canonical
///   unsigned document, verified against the issuer's key
/// - `gate_results[]` — each gate attestation's signature verified
///   against the party its `signed_by` names
/// - `spec_approval_valid` — the spec approval signature, when the
///   bundle carries one (`null` when there is nothing to verify)
/// - `consistency_check` — all gate results reference the same MR/spec
///   (structural check)
/// - `errors[]` — every finding, naming the failing component
///
/// The bundle is caller-supplied: verification is a pure evaluation of
/// the submitted document against the issuer's key material — no
/// platform state is read or mutated. This is the endpoint external
/// tools (auditors, compliance pipelines, federating Gyre instances)
/// call to verify a bundle retrieved offline (e.g. from a merge
/// commit's git note under `refs/notes/merge-attestations`); the CLI
/// wraps it as `gyre verify-attestation <commit-sha>`.
///
/// A bundle that fails verification is a *valid answer*, not an error:
/// the endpoint returns 200 with `valid: false` and the per-component
/// findings. Only transport/processing failures produce error status
/// codes. A body that does not deserialize as a bundle is a 400.
///
/// ABAC: resource_type = attestation, action = read.
pub async fn verify_bundle(
    State(state): State<Arc<AppState>>,
    Json(bundle): Json<MergeAttestationBundle>,
) -> Result<Json<VerificationResult>, ApiError> {
    let result = state
        .attestation_verifier
        .verify_bundle(&bundle)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("attestation verification failed: {e}")))?;
    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::post,
        Router,
    };
    use gyre_ports::AttestationSigner;
    use tower::ServiceExt;

    fn test_state() -> Arc<AppState> {
        // Explicit in-memory opt-in (architecture.md §Storage): the default
        // (unset) backend is now durable SQLite at the platform default
        // data path — a unit test must never touch the operator's real
        // database file, and these tests rely on the fresh-per-boot
        // signing key of the stateless mode.
        crate::select_in_memory_storage();
        crate::build_state("gyre-test-token", "http://localhost:0", None)
    }

    fn app(state: &Arc<AppState>) -> Router {
        // The router must serve with the SAME state that signed the
        // bundle: `build_state` generates a fresh Ed25519 issuer key per
        // call in tests (no DB), and verification is an evaluation of the
        // bundle against the *issuer's* key — a second, independent
        // state would reject every legitimately-signed bundle (and make
        // the tamper tests pass vacuously on key mismatch instead of on
        // the tamper).
        Router::new()
            .route("/api/v1/attestations/verify", post(verify_bundle))
            .with_state(state.clone())
    }

    /// A minimal well-formed bundle: one forge-signed gate and a forge
    /// bundle signature over the canonical unsigned document, produced
    /// with the same signers the merge processor uses.
    async fn signed_bundle(state: &Arc<AppState>) -> MergeAttestationBundle {
        let forge = crate::merge_attestation::ForgeSigner::new(state.agent_signing_key.clone());
        let mut gate = gyre_domain::merge_attestation::GateAttestation {
            name: "tests".to_string(),
            gate_type: "TestCommand".to_string(),
            status: "Passed".to_string(),
            output_hash: "sha256:abcd".to_string(),
            started_at: Some(1),
            finished_at: Some(2),
            reviewer_agent_id: None,
            reviewer_oidc_sub: None,
            reviewer_stack_attestation: None,
            review_decision: None,
            review_body_hash: None,
            signed_by: "forge".to_string(),
            signature: None,
        };
        gate.signature = Some(
            forge
                .sign(gate.unsigned_gate_json().as_bytes())
                .await
                .unwrap(),
        );
        let mut bundle = MergeAttestationBundle {
            merge_attestation_version: gyre_domain::merge_attestation::MERGE_ATTESTATION_VERSION
                .to_string(),
            mr_id: "MR-1".to_string(),
            merge_commit_sha: "abc123".to_string(),
            merged_at: 1_774_000_000,
            spec_ref: None,
            spec_approval: None,
            author: None,
            gates: vec![gate],
            bundle_signature: None,
        };
        bundle.bundle_signature = Some(
            forge
                .sign(bundle.unsigned_canonical_json().as_bytes())
                .await
                .unwrap(),
        );
        bundle
    }

    fn post_json(body: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/api/v1/attestations/verify")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap()
    }

    #[tokio::test]
    async fn valid_bundle_verifies_over_http() {
        let state = test_state();
        let bundle = signed_bundle(&state).await;
        let response = app(&state)
            .oneshot(post_json(serde_json::to_value(&bundle).unwrap()))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: VerificationResult = serde_json::from_slice(&bytes).unwrap();
        assert!(result.valid, "got {:?}", result.errors);
        assert!(result.bundle_signature_valid);
        assert!(result.consistency_check);
        assert_eq!(result.gate_results.len(), 1);
        assert!(result.gate_results[0].signature_valid);
        assert_eq!(result.spec_approval_valid, None);
    }

    #[tokio::test]
    async fn tampered_bundle_returns_200_with_valid_false_and_findings() {
        let state = test_state();
        let mut bundle = signed_bundle(&state).await;
        // Tamper after signing: the bundle signature must fail.
        bundle.merged_at += 1;
        let response = app(&state)
            .oneshot(post_json(serde_json::to_value(&bundle).unwrap()))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "a failed verification is a valid answer, not an HTTP error"
        );
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: VerificationResult = serde_json::from_slice(&bytes).unwrap();
        assert!(!result.valid);
        assert!(!result.bundle_signature_valid);
        assert!(result.errors.iter().any(|e| e.contains("bundle_signature")));
    }

    #[tokio::test]
    async fn missing_bundle_signature_is_reported_not_passed() {
        let state = test_state();
        let mut bundle = signed_bundle(&state).await;
        bundle.bundle_signature = None;
        let response = app(&state)
            .oneshot(post_json(serde_json::to_value(&bundle).unwrap()))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: VerificationResult = serde_json::from_slice(&bytes).unwrap();
        assert!(!result.valid);
        assert!(!result.bundle_signature_valid);
    }

    #[tokio::test]
    async fn body_that_is_not_a_bundle_is_rejected_400() {
        let state = test_state();
        let response = app(&state)
            .oneshot(post_json(serde_json::json!({"not": "a bundle"})))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}
