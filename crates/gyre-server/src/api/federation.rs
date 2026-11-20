//! Federation API (G11) — cross-instance JWT verification via OIDC discovery.
//!
//! Exposes the list of trusted remote Gyre instances configured via
//! `GYRE_TRUSTED_ISSUERS`. Callers can use this to discover which external
//! Gyre instances' agent JWTs are accepted by this server.
//!
//! Also exposes the SPIFFE-federation workload-attestation surface
//! (identity-security.md §Federation, "SPIFFE federation handles the
//! workload attestation layer independently"):
//! - `POST /api/v1/federation/spiffe/attest` — a federated agent presents
//!   its X.509-SVID for Layer-1 verification against the trust bundle
//!   exchanged with the SVID's trust domain;
//! - `GET /api/v1/federation/spiffe/workload` — a federated agent queries
//!   its own workload-attestation record on this instance.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use crate::auth::AuthenticatedAgent;
use crate::workload_attestation::WorkloadAttestation;
use crate::AppState;

use super::error::ApiError;

/// GET /api/v1/federation/trusted-issuers
///
/// Returns the list of trusted remote Gyre base URLs whose agent JWTs are
/// accepted by this server's auth middleware.
///
/// Response: `{"trusted_issuers": ["https://gyre-2.example.com", ...]}`
pub async fn list_trusted_issuers(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "trusted_issuers": state.trusted_issuers,
        })),
    )
}

/// POST /api/v1/federation/spiffe/attest request body: the federated
/// workload presents its SVID (PEM chain, leaf first — the
/// `GYRE_SVID_PEM` environment variable on the remote instance).
#[derive(Debug, Deserialize)]
pub struct FederatedAttestRequest {
    pub svid: String,
}

/// Resolve the caller's Layer-2 (OIDC-federation) claims when — and only
/// when — the caller is a federated agent: the JWT issuer must be one of
/// the configured trusted issuers AND not this instance's own issuer
/// (local agents verify their SVIDs through the local surface,
/// `POST /api/v1/agents/{id}/attest`, against the local trust bundle).
fn federated_caller_claims<'a>(
    auth: &'a AuthenticatedAgent,
    state: &AppState,
) -> Option<&'a serde_json::Value> {
    let claims = auth.jwt_claims.as_ref()?;
    let iss = claims.get("iss")?.as_str()?;
    let normalized = iss.trim_end_matches('/');
    let local = state.base_url.trim_end_matches('/');
    if normalized == local {
        return None;
    }
    state
        .trusted_issuers
        .iter()
        .any(|t| t.trim_end_matches('/') == normalized)
        .then_some(claims)
}

/// POST /api/v1/federation/spiffe/attest
///
/// The federated Layer-1 workload-identity verification surface: verifies
/// the presented SVID against the trust bundle exchanged with the SVID's
/// trust domain (fail-closed — a trust domain whose bundle was never
/// exchanged is rejected even though the caller's OIDC JWT authenticated),
/// binds the SPIFFE ID to the caller's remote session, and records the
/// outcome in the caller's workload-attestation record.
pub async fn attest_federated_workload(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Json(req): Json<FederatedAttestRequest>,
) -> Result<
    (
        StatusCode,
        Json<crate::spiffe_federation::FederatedWorkloadVerification>,
    ),
    ApiError,
> {
    let claims = federated_caller_claims(&auth, &state).ok_or_else(|| {
        ApiError::Forbidden(
            "workload-attestation federation requires a federated agent caller; local workloads \
             use POST /api/v1/agents/{id}/attest"
                .to_string(),
        )
    })?;
    let sub = claims.get("sub").and_then(|v| v.as_str()).ok_or_else(|| {
        ApiError::Forbidden("federated caller has no session subject".to_string())
    })?;

    match crate::spiffe_federation::attest_federated_session(
        &state,
        &auth.agent_id,
        sub,
        &req.svid,
        Some(claims),
    )
    .await
    {
        Ok(verification) => Ok((StatusCode::OK, Json(verification))),
        Err(reason) => Err(ApiError::Forbidden(format!(
            "federated workload-identity verification failed: {reason}"
        ))),
    }
}

/// GET /api/v1/federation/spiffe/workload
///
/// Returns the CALLER's own federated workload-attestation record (the
/// record written by the federated attest surface). 404 when the caller
/// has never presented a verifying SVID.
pub async fn get_federated_workload(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
) -> Result<Json<WorkloadAttestation>, ApiError> {
    // Federated callers only (the local surface serves local agents).
    federated_caller_claims(&auth, &state).ok_or_else(|| {
        ApiError::Forbidden(
            "workload-attestation federation requires a federated agent caller".to_string(),
        )
    })?;
    state
        .kv_store
        .kv_get("workload_attestations", &auth.agent_id)
        .await
        .map_err(ApiError::Internal)?
        .and_then(|s| serde_json::from_str::<WorkloadAttestation>(&s).ok())
        .map(Json)
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "no workload attestation for federated agent {}",
                auth.agent_id
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spiffe_federation::test_support::TestHttpServer;
    use crate::spiffe_federation::{FederationBundles, FederationPeer};
    use axum::body::Body;
    use gyre_spiffe::{SpiffeId, TrustDomainCa, BASE_SVID_TTL_SECS};
    use http::{Request, StatusCode};
    use std::time::Duration;
    use tower::ServiceExt;

    const PEER_TD: &str = "gyre-a.example.com";

    /// Spin up "instance A": a remote Gyre whose public endpoints are all
    /// served over real HTTP from a local listener —
    /// `/.well-known/openid-configuration` + `/.well-known/jwks.json`
    /// (Layer-2 OIDC federation) and `/.well-known/spiffe/keys.json`
    /// (Layer-1 trust bundle). Returns the server, A's agent-JWT signing
    /// key, and A's trust-domain CA.
    fn spawn_instance_a() -> (TestHttpServer, crate::auth::AgentSigningKey, TrustDomainCa) {
        let key = crate::auth::AgentSigningKey::generate();
        let ca = TrustDomainCa::generate(PEER_TD).expect("peer CA generation");
        let jwks = key.jwks_json.clone();
        let bundle = gyre_spiffe::trust_bundle_jwks(&[ca.cert_pem()]).to_string();
        // The origin is only known once the listener is bound; the
        // discovery document must echo it, so share it through a cell.
        let origin_cell = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let cell = std::sync::Arc::clone(&origin_cell);
        let server = TestHttpServer::spawn(move |_method, path| {
            let origin = cell.lock().unwrap().clone();
            match path {
                "/.well-known/openid-configuration" => (
                    200,
                    serde_json::json!({
                        "issuer": origin,
                        "jwks_uri": format!("{origin}/.well-known/jwks.json"),
                    })
                    .to_string(),
                ),
                "/.well-known/jwks.json" => (200, jwks.clone()),
                "/.well-known/spiffe/keys.json" => (200, bundle.clone()),
                _ => (404, "{}".to_string()),
            }
        });
        *origin_cell.lock().unwrap() = server.origin.clone();
        (server, key, ca)
    }

    /// Instance B's state: A's OIDC issuer is trusted (Layer 2) in BOTH
    /// configurations; A's SPIFFE bundle is exchanged (Layer 1) only when
    /// `with_peer` is true.
    fn state_for_b(origin: &str, with_peer: bool) -> Arc<AppState> {
        let base = crate::mem::test_state();
        let mut state = (*base).clone();
        state.trusted_issuers = vec![origin.to_string()];
        let peers = if with_peer {
            vec![FederationPeer {
                trust_domain: PEER_TD.to_string(),
                bundle_endpoint: format!("{origin}/.well-known/spiffe/keys.json"),
            }]
        } else {
            vec![]
        };
        state.spiffe_federation = Arc::new(FederationBundles::new(peers, Duration::from_secs(300)));
        Arc::new(state)
    }

    fn federated_token(key: &crate::auth::AgentSigningKey, issuer: &str) -> String {
        key.mint(
            "fed-session-1",
            "task-9",
            "system",
            issuer,
            3600,
            "tenant-remote",
            "ws-remote",
            "repo-remote",
            "remote-repo",
        )
        .expect("remote JWT mint must succeed")
    }

    /// The remote session subject a real Gyre issuer binds into BOTH the
    /// JWT `sub` and the SVID's SPIFFE ID session segment — decoded from
    /// the REAL mint (not re-derived from a constant) so this probe fails
    /// if either side drifts from the shared contract
    /// `/agent/{role}/session/{jwt sub}` (spiffe_federation.rs).
    fn federated_sub(key: &crate::auth::AgentSigningKey, token: &str, issuer: &str) -> String {
        key.validate(token, issuer)
            .expect("real mint must validate against its own issuer")
            .sub
    }

    fn peer_svid(ca: &TrustDomainCa, session: &str) -> gyre_spiffe::ca::IssuedSvid {
        ca.issue_svid(
            &SpiffeId::agent(PEER_TD, "worker", session).expect("peer SPIFFE ID"),
            BASE_SVID_TTL_SECS,
        )
        .expect("peer SVID issuance")
    }

    async fn post_attest(
        state: &Arc<AppState>,
        token: &str,
        svid: &str,
    ) -> axum::response::Response {
        crate::build_router(Arc::clone(state))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/federation/spiffe/attest")
                    .header("content-type", "application/json")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({ "svid": svid })).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn get_workload(state: &Arc<AppState>, token: &str) -> axum::response::Response {
        crate::build_router(Arc::clone(state))
            .oneshot(
                Request::builder()
                    .uri("/api/v1/federation/spiffe/workload")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// The task's verification-intent probe, end to end over HTTP: a
    /// federated agent from instance A authenticates at instance B (its
    /// Layer-2 JWT validates via real OIDC discovery against A in BOTH
    /// configurations) and presents its SVID. Without the bundle exchange
    /// the workload attestation must NOT verify or be reported as
    /// attested; with the exchange the same SVID verifies and yields the
    /// same SPIFFE ID A's own verifiers would extract.
    #[tokio::test]
    async fn federated_agent_two_configuration_probe() {
        let (server, key, ca) = spawn_instance_a();
        let token = federated_token(&key, &server.origin);
        // The session subject the remote issuer really minted into the
        // token's `sub`; the peer SVID names the SAME session, per the
        // documented invariant `/agent/{role}/session/{jwt sub}`.
        let sub = federated_sub(&key, &token, &server.origin);
        let svid = peer_svid(&ca, &sub);
        let expected_id = format!("spiffe://{PEER_TD}/agent/worker/session/{sub}");

        // (a) B trusts A's OIDC issuer but has NOT exchanged A's SPIFFE
        // bundle: the agent AUTHENTICATES (Layer 2 — the request passes
        // auth + ABAC and reaches the handler, so the failure below is
        // the workload-attestation rejection, not an auth rejection) but
        // its workload attestation does NOT verify.
        let state = state_for_b(&server.origin, false);
        let resp = post_attest(&state, &token, &svid.chain_pem).await;
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "unexchanged bundle must fail the workload attestation"
        );
        let json = body_json(resp).await;
        let reason = json["error"].as_str().unwrap_or_default();
        assert!(
            reason.contains("not federated"),
            "expected a not-federated rejection, got: {reason}"
        );
        // And it is not reported as attested: no record exists.
        let resp = get_workload(&state, &token).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // (b) B exchanges A's bundle: the SAME SVID verifies.
        let state = state_for_b(&server.origin, true);
        let resp = post_attest(&state, &token, &svid.chain_pem).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["spiffe_id"], expected_id);
        assert_eq!(json["trust_domain"], PEER_TD);
        assert_eq!(json["session_id"], sub);

        // The workload is now reported as attested, keyed by the
        // federated agent id ("{remote-host}/{sub}").
        let resp = get_workload(&state, &token).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["alive"], true);
        assert_eq!(json["spiffe_id"], expected_id);
        assert_eq!(
            json["agent_id"],
            format!("{}/{}", server.origin.trim_start_matches("http://"), sub)
        );
    }

    /// An SVID from a trust domain whose bundle was never exchanged —
    /// even though a DIFFERENT peer's bundle is exchanged and the caller's
    /// JWT is trusted — is rejected (identity-security.md §Federation:
    /// exchanged bundles are per trust domain).
    #[tokio::test]
    async fn never_exchanged_trust_domain_rejected_over_http() {
        let (server, key, _ca) = spawn_instance_a();
        let token = federated_token(&key, &server.origin);
        let sub = federated_sub(&key, &token, &server.origin);

        // A genuine SVID from a third instance (gyre-c.example.org) whose
        // bundle B never exchanged, presented by the trusted A-agent.
        let third = TrustDomainCa::generate("gyre-c.example.org").expect("third CA");
        let svid = third
            .issue_svid(
                &SpiffeId::agent("gyre-c.example.org", "worker", &sub).expect("third SPIFFE ID"),
                BASE_SVID_TTL_SECS,
            )
            .expect("third SVID issuance");

        let state = state_for_b(&server.origin, true);
        let resp = post_attest(&state, &token, &svid.chain_pem).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let json = body_json(resp).await;
        let reason = json["error"].as_str().unwrap_or_default();
        assert!(
            reason.contains("not federated"),
            "expected a not-federated rejection, got: {reason}"
        );
    }

    /// Local callers (the system token, local agents) do not use the
    /// federated surface — local workloads attest through
    /// POST /api/v1/agents/{id}/attest against the LOCAL trust bundle.
    #[tokio::test]
    async fn local_caller_rejected_on_federation_surface() {
        let (server, _key, _ca) = spawn_instance_a();
        let state = state_for_b(&server.origin, true);
        let resp = post_attest(&state, "test-token", "-----BEGIN CERTIFICATE-----\n").await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let json = body_json(resp).await;
        let reason = json["error"].as_str().unwrap_or_default();
        assert!(
            reason.contains("federated agent caller"),
            "expected a non-federated-caller rejection, got: {reason}"
        );

        let resp = get_workload(&state, "test-token").await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
