//! SPIFFE workload attestation service — identity-security.md Layer 1.
//!
//! Layer 1 proves "this workload is real" with the specced existing
//! protocol (SPIFFE X.509-SVIDs), replacing the retired G10 local scheme
//! (self-reported PID/hostname/stack plus a `/proc/{pid}` existence probe
//! and a skip-on-empty string compare).
//!
//! [`SpiffeService`] owns the trust anchor:
//! - default: a trust-domain CA Gyre operates (persisted across restarts in
//!   the durable KV store, `GYRE_SPIFFE_CA` for operator provisioning);
//! - `GYRE_SPIFFE_WORKLOAD_API_SOCKET`: an external SPIRE server / Workload
//!   API from which SVIDs are obtained (gRPC over UDS), anchored by the
//!   operator-configured trust bundle (`GYRE_SPIFFE_TRUST_BUNDLE_URL`,
//!   SPIFFE bundle format) — trust is never derived from the workload's own
//!   response.
//!
//! The trust bundle is published in SPIFFE bundle format at
//! `/.well-known/spiffe/keys.json` and is the sole verification anchor.
//!
//! The verification surface ([`verify_workload_identity`]) is fail-closed:
//! a request presenting no SVID, an SVID signed by a key not in the trust
//! bundle, an expired/not-yet-valid SVID, or an SVID whose SPIFFE ID names
//! a different agent session or a foreign trust domain is rejected. The
//! stack fingerprint presented at verification is bound to the SVID-backed
//! attestation record — an empty or tampered stack string is rejected.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use gyre_spiffe::workload_api::{GrpcWorkloadApi, WorkloadApiTransport};
use gyre_spiffe::{
    parse_trust_bundle, trust_bundle_jwks, verify_svid_for_record_id, ExpectedIdentity, SpiffeId,
    TrustDomainCa,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use crate::AppState;

/// Standard well-known location at which the server publishes its trust
/// bundle (SPIFFE bundle format, JWK Set with `use: "svid"`), so any
/// verifier can check SVIDs.
pub const TRUST_BUNDLE_PATH: &str = "/.well-known/spiffe/keys.json";

/// Default trust domain (`GYRE_SPIFFE_TRUST_DOMAIN` overrides).
pub const DEFAULT_TRUST_DOMAIN: &str = "gyre.example.com";

/// KV namespace holding the per-agent-session SVID records (the SVID-backed
/// attestation records; port-backed store, never a bare in-memory map).
pub const SVID_KV_NAMESPACE: &str = "spiffe_svids";

/// TTL for cached externally-fetched trust bundles.
const EXTERNAL_BUNDLE_CACHE_TTL: Duration = Duration::from_secs(300);

/// Bound on the external bundle fetch (reqwest has no default timeout; an
/// unresponsive bundle endpoint must fail verification, never stall it).
const EXTERNAL_BUNDLE_FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// Bound on Workload API calls.
const WORKLOAD_API_TIMEOUT: Duration = Duration::from_secs(10);

/// The trust anchor + issuance service for one server boot.
pub struct SpiffeService {
    trust_domain: String,
    svid_ttl_secs: u64,
    /// Local-mode CA Gyre operates (None in external Workload API mode).
    ca: Option<TrustDomainCa>,
    /// Prebuilt SPIFFE bundle JSON for local mode.
    local_bundle_json: Option<String>,
    /// External mode: Workload API transport.
    workload_api: Option<Arc<dyn WorkloadApiTransport>>,
    /// External mode: operator-configured trust bundle URL.
    external_bundle_url: Option<String>,
    /// Cache of the last successfully fetched external bundle.
    external_cache: tokio::sync::RwLock<Option<(Instant, String)>>,
    /// Bounded HTTP client for external bundle fetches.
    http: reqwest::Client,
}

impl SpiffeService {
    /// Build the local-mode service around a Gyre-operated CA.
    pub fn local(
        trust_domain: String,
        ca: TrustDomainCa,
        svid_ttl_secs: u64,
    ) -> anyhow::Result<Self> {
        let bundle = trust_bundle_jwks(&[ca.cert_pem()]);
        Ok(Self {
            trust_domain,
            svid_ttl_secs,
            local_bundle_json: Some(bundle.to_string()),
            ca: Some(ca),
            workload_api: None,
            external_bundle_url: None,
            external_cache: tokio::sync::RwLock::new(None),
            http: bounded_http_client()?,
        })
    }

    /// Build the external-mode service: SVIDs come from a SPIRE Workload
    /// API, verification is anchored by the configured trust bundle URL.
    pub fn external(
        trust_domain: String,
        socket_path: String,
        bundle_url: String,
        svid_ttl_secs: u64,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            trust_domain,
            svid_ttl_secs,
            ca: None,
            local_bundle_json: None,
            workload_api: Some(Arc::new(GrpcWorkloadApi::new(
                socket_path,
                WORKLOAD_API_TIMEOUT,
            ))),
            external_bundle_url: Some(bundle_url),
            external_cache: tokio::sync::RwLock::new(None),
            http: bounded_http_client()?,
        })
    }

    /// Test seam: external mode with an injected transport.
    #[cfg(test)]
    pub fn external_with_transport(
        trust_domain: String,
        transport: Arc<dyn WorkloadApiTransport>,
        bundle_url: String,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            trust_domain,
            svid_ttl_secs: gyre_spiffe::BASE_SVID_TTL_SECS,
            ca: None,
            local_bundle_json: None,
            workload_api: Some(transport),
            external_bundle_url: Some(bundle_url),
            external_cache: tokio::sync::RwLock::new(None),
            http: bounded_http_client()?,
        })
    }

    pub fn trust_domain(&self) -> &str {
        &self.trust_domain
    }

    pub fn svid_ttl_secs(&self) -> u64 {
        self.svid_ttl_secs
    }

    /// The trust bundle in SPIFFE bundle format (JSON). Local mode serves
    /// the prebuilt bundle; external mode relays the operator-configured
    /// bundle endpoint's document (cached, TTL-bounded).
    pub async fn bundle_json(&self) -> Result<String, String> {
        if let Some(local) = &self.local_bundle_json {
            return Ok(local.clone());
        }
        self.fetch_external_bundle().await
    }

    /// The verification anchors (CA certificate DERs) — the sole anchor
    /// set; verification never trusts self-reported metadata.
    pub async fn anchors(&self) -> Result<Vec<Vec<u8>>, String> {
        let json = self.bundle_json().await?;
        let value: serde_json::Value =
            serde_json::from_str(&json).map_err(|e| format!("trust bundle is not JSON: {e}"))?;
        parse_trust_bundle(&value)
    }

    async fn fetch_external_bundle(&self) -> Result<String, String> {
        let url = self
            .external_bundle_url
            .as_deref()
            .ok_or_else(|| "no trust bundle configured".to_string())?;
        if let Some((fetched_at, json)) = self.external_cache.read().await.as_ref() {
            if fetched_at.elapsed() < EXTERNAL_BUNDLE_CACHE_TTL {
                return Ok(json.clone());
            }
        }
        let mut guard = self.external_cache.write().await;
        // Re-check under the write lock (another request may have refreshed).
        if let Some((fetched_at, json)) = guard.as_ref() {
            if fetched_at.elapsed() < EXTERNAL_BUNDLE_CACHE_TTL {
                return Ok(json.clone());
            }
        }
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| format!("trust bundle fetch failed: {e}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "trust bundle endpoint returned {}",
                response.status()
            ));
        }
        let json = response
            .text()
            .await
            .map_err(|e| format!("trust bundle body unreadable: {e}"))?;
        // Fail closed on anything that is not a valid SPIFFE bundle — a
        // garbage document must never become the anchor.
        let value: serde_json::Value =
            serde_json::from_str(&json).map_err(|e| format!("trust bundle is not JSON: {e}"))?;
        parse_trust_bundle(&value)?;
        *guard = Some((Instant::now(), json.clone()));
        Ok(json)
    }

    /// Issue (or obtain) the SVID for an agent session.
    ///
    /// Local mode: issued from the trust-domain CA. External mode: obtained
    /// from the Workload API; the response MUST contain an SVID whose
    /// SPIFFE ID names this session (fail closed on an unrelated identity).
    pub async fn issue_for_session(
        &self,
        role: &str,
        session_id: &str,
    ) -> Result<gyre_spiffe::ca::IssuedSvid, String> {
        let expected_id =
            SpiffeId::agent(&self.trust_domain, role, session_id).map_err(|e| e.to_string())?;
        if let Some(ca) = &self.ca {
            return ca
                .issue_svid(&expected_id, self.svid_ttl_secs)
                .map_err(|e| format!("SVID issuance failed: {e}"));
        }
        let transport = self
            .workload_api
            .as_ref()
            .ok_or_else(|| "SVID issuance unavailable (no CA, no Workload API)".to_string())?;
        let response = transport
            .fetch_x509_svid()
            .await
            .map_err(|e| format!("Workload API fetch failed: {e}"))?;
        let svids = gyre_spiffe::workload_api::decode_response(&response)
            .map_err(|e| format!("Workload API response invalid: {e}"))?;
        let wanted = expected_id.to_string();
        let found = svids
            .into_iter()
            .find(|s| s.spiffe_id == wanted)
            .ok_or_else(|| {
                format!(
                    "Workload API returned no SVID for {wanted}; register the session's SPIFFE ID with SPIRE"
                )
            })?;
        // Extract the leaf block and its real metadata from the obtained
        // chain (the record must carry what was actually obtained, not
        // placeholder values).
        let blocks = pem::parse_many(&found.chain_pem)
            .map_err(|e| format!("obtained chain is not parseable PEM: {e}"))?;
        let leaf_der = blocks
            .first()
            .ok_or_else(|| "obtained chain is empty".to_string())?
            .contents()
            .to_vec();
        let (serial, not_before, not_after) = {
            let (_, leaf) = x509_parser::parse_x509_certificate(&leaf_der)
                .map_err(|e| format!("obtained leaf certificate is unparseable: {e}"))?;
            (
                // Canonical big-integer hex — the form verification
                // extracts from the parsed leaf (canonical_serial_hex).
                gyre_spiffe::canonical_serial_hex(&leaf.serial.to_bytes_be()),
                leaf.validity.not_before.timestamp(),
                leaf.validity.not_after.timestamp(),
            )
        };
        Ok(gyre_spiffe::ca::IssuedSvid {
            spiffe_id: expected_id,
            leaf_pem: pem::encode(&pem::Pem::new("CERTIFICATE", leaf_der)),
            chain_pem: found.chain_pem,
            private_key_pem: found.private_key_pem,
            serial,
            not_before,
            not_after,
        })
    }
}

fn bounded_http_client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(EXTERNAL_BUNDLE_FETCH_TIMEOUT)
        .build()?)
}

// ---------------------------------------------------------------------------
// Session records (KV `spiffe_svids`)
// ---------------------------------------------------------------------------

/// The SVID-backed attestation record for one agent session: what was
/// issued/obtained at spawn and the evidence it was gated on. Written only
/// by the issuance path; read by the verification surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SvidRecord {
    pub agent_id: String,
    /// SPIFFE ID of the session (URI SAN of the leaf). The session
    /// segment is the session's JWT subject (`agent:<uuid>`) — see
    /// `issue_session_svid`.
    pub spiffe_id: String,
    /// Role segment of the SPIFFE ID path.
    pub role: String,
    /// Trust domain the SVID was issued under.
    pub trust_domain: String,
    /// The issued chain (leaf first), PEM.
    pub chain_pem: String,
    /// Authorized compute target label at spawn (issuance evidence).
    pub compute_target: String,
    /// Stack fingerprint bound at issuance/first attestation (issuance
    /// evidence; empty when the session had no stack evidence at spawn).
    pub stack_fingerprint: String,
    /// Leaf serial (hex; empty for Workload API SVIDs pre-parse).
    pub serial: String,
    pub issued_at: u64,
    pub not_before: i64,
    pub not_after: i64,
    /// "gyre-ca" | "workload-api".
    pub source: String,
}

/// Resolve the stack fingerprint for an agent session (the issuance-time
/// evidence bound to the SVID record and carried by the `wl_stack_hash`
/// JWT claim):
///
/// - the agent's REGISTERED stack fingerprint (M14.1 `agent_stacks`, e.g.
///   a CLI-managed level-2 session) when one exists — a self-report that
///   has been recorded through the stack-attestation channel;
/// - otherwise a fingerprint over the server-AUTHORIZED launch
///   configuration (compute target, resolved command, image digest,
///   persona) — supply-chain.md's "the exact configuration an agent was
///   running" applied to what the server itself authorized at spawn, not
///   a free-form self-report. Always non-empty: a session without stack
///   evidence cannot present a stack fingerprint at verification, and an
///   empty presentation is rejected (fail closed).
pub async fn resolve_session_stack_evidence(
    state: &AppState,
    agent_id: &str,
    compute_target: &str,
    resolved_ct_entity: Option<&gyre_domain::ComputeTargetEntity>,
    image_digest: Option<&str>,
    persona: Option<&str>,
) -> String {
    let registered = state
        .kv_store
        .kv_get("agent_stacks", agent_id)
        .await
        .ok()
        .flatten()
        .and_then(|s| {
            serde_json::from_str::<crate::api::stack_attest::AgentStack>(&s)
                .ok()
                .map(|st| st.fingerprint())
        })
        .unwrap_or_default();
    if !registered.is_empty() {
        return registered;
    }
    // The command the launch will run, resolved exactly as
    // launch_agent_process resolves it (compute-target config →
    // GYRE_AGENT_COMMAND → the reference entrypoint).
    let command = resolved_ct_entity
        .and_then(|e| e.config.get("command"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| std::env::var("GYRE_AGENT_COMMAND").ok())
        .unwrap_or_else(|| "/gyre/entrypoint.sh".to_string());
    let canonical = serde_json::json!({
        "compute_target": compute_target,
        "command": command,
        "image_digest": image_digest,
        "persona": persona,
    });
    use sha2::Digest;
    let digest = sha2::Sha256::digest(canonical.to_string().as_bytes());
    format!("sha256:{}", hex::encode(digest))
}

/// The role segment of an agent session's SPIFFE ID: the persona slug when
/// a persona is bound, else the orchestration tier.
pub fn agent_role(
    persona: Option<&str>,
    orchestrator_type: &gyre_domain::OrchestratorType,
) -> String {
    match persona {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => match orchestrator_type {
            gyre_domain::OrchestratorType::Worker => "worker".to_string(),
            gyre_domain::OrchestratorType::WorkspaceOrchestrator => {
                "workspace-orchestrator".to_string()
            }
            gyre_domain::OrchestratorType::RepoOrchestrator => "repo-orchestrator".to_string(),
        },
    }
}

/// Issue and persist the SVID for an agent session. Called only from the
/// server's authorized spawn paths (post-ABAC, post-budget, with the
/// resolved compute target and stack evidence) — SVID issuance is gated on
/// that spawn-time authorization evidence, so the runtime proves it runs
/// on authorized infrastructure.
///
/// The SPIFFE ID session segment is the session's JWT subject
/// (`agent:<uuid>`, see `crate::auth::agent_subject`) — the same `sub` the
/// Layer-2 mint embeds for this agent — so the documented binding
/// invariant `/agent/{role}/session/{jwt sub}` (spiffe_federation.rs,
/// identity-security.md §Federation) holds exactly: a federated verifier
/// binds the presented SVID to the caller's JWT `sub` by direct string
/// comparison, with no normalization on either side. The KV record stays
/// keyed by the bare agent id (the local lookup key everywhere else).
pub async fn issue_session_svid(
    state: &AppState,
    agent_id: &str,
    role: &str,
    compute_target: &str,
    stack_fingerprint: &str,
) -> Result<SessionSvid, anyhow::Error> {
    let session_id = crate::auth::agent_subject(agent_id);
    let issued = state
        .spiffe
        .issue_for_session(role, &session_id)
        .await
        .map_err(|e| anyhow::anyhow!("SVID issuance for agent {agent_id}: {e}"))?;
    let record = SvidRecord {
        agent_id: agent_id.to_string(),
        spiffe_id: issued.spiffe_id.to_string(),
        role: role.to_string(),
        trust_domain: state.spiffe.trust_domain().to_string(),
        chain_pem: issued.chain_pem.clone(),
        compute_target: compute_target.to_string(),
        stack_fingerprint: stack_fingerprint.to_string(),
        serial: issued.serial.clone(),
        issued_at: now_secs(),
        not_before: issued.not_before,
        not_after: issued.not_after,
        source: if state.spiffe.workload_api_mode() {
            "workload-api"
        } else {
            "gyre-ca"
        }
        .to_string(),
    };
    let json = serde_json::to_string(&record)?;
    state
        .kv_store
        .kv_set(SVID_KV_NAMESPACE, agent_id, json)
        .await?;
    Ok(SessionSvid {
        record,
        private_key_pem: issued.private_key_pem,
    })
}

/// The result of session SVID issuance: the persisted record plus the
/// SVID private key (handed to the workload via its environment; not
/// persisted server-side).
pub struct SessionSvid {
    pub record: SvidRecord,
    pub private_key_pem: String,
}

impl SpiffeService {
    fn workload_api_mode(&self) -> bool {
        self.workload_api.is_some()
    }
}

/// A successful workload-identity verification.
#[derive(Debug, Clone, Serialize)]
pub struct WorkloadVerification {
    pub agent_id: String,
    pub spiffe_id: String,
    pub serial: String,
    pub not_after: i64,
    /// The stack fingerprint now bound to the attestation record.
    pub stack_fingerprint: String,
}

/// The workload-identity verification surface (identity-security.md
/// Layer 1): verify the SVID a workload presents for an agent session
/// against the published trust bundle, and bind the stack fingerprint.
///
/// Fail-closed on every check (see module docs). This replaces the retired
/// G10 `/proc/{pid}` probe and skip-on-empty string compare — no
/// verification path succeeds on an unattested or tampered workload.
// crypto-verify:ok — performs real cryptographic chain validation via
// gyre_spiffe::verify_svid (x509 verify_signature of the leaf against the
// trust bundle's CA keys, ring ECDSA) plus validity-window and SPIFFE-ID
// session binding; self-reported metadata is never trusted as an anchor.
pub async fn verify_workload_identity(
    state: &AppState,
    agent_id: &str,
    presented_svid: &str,
    presented_stack: &str,
) -> Result<WorkloadVerification, String> {
    // 1. The session must hold an SVID record (issued through the spawn
    //    path). An unattested session fails verification — no bypass.
    let record_json = state
        .kv_store
        .kv_get(SVID_KV_NAMESPACE, agent_id)
        .await
        .map_err(|e| format!("SVID record lookup failed: {e}"))?
        .ok_or_else(|| {
            format!("no SVID record for agent {agent_id}: session was never attested")
        })?;
    let record: SvidRecord = serde_json::from_str(&record_json)
        .map_err(|e| format!("SVID record for agent {agent_id} is unreadable: {e}"))?;

    // 2. An SVID must be presented.
    if presented_svid.trim().is_empty() {
        return Err("no SVID presented".to_string());
    }

    // 3. Chain validation against the sole anchor (the published trust
    //    bundle) + validity window + SPIFFE ID session binding. Agent-form
    //    SPIFFE IDs bind to the expected trust domain / role / session; a
    //    non-agent-form ID (external SPIRE registration) binds to the exact
    //    recorded identity.
    let anchors = state.spiffe.anchors().await?;
    let now = SystemTime::now();
    let expected_id = SpiffeId::parse(&record.spiffe_id)
        .map_err(|e| format!("recorded SPIFFE ID is malformed: {e}"))?;
    let verified = match expected_id.agent_path() {
        Some(path) => {
            let expected = ExpectedIdentity {
                trust_domain: expected_id.trust_domain.clone(),
                role: path.role,
                session_id: path.session_id,
            };
            gyre_spiffe::verify_svid(presented_svid, &anchors, &expected, now)
        }
        None => verify_svid_for_record_id(presented_svid, &anchors, &expected_id, now),
    }
    .map_err(|e| format!("SVID rejected: {e}"))?;

    // The presented leaf must be the SVID the record was issued (same
    // serial), not merely any valid SVID for the same session identity.
    if !record.serial.is_empty() && verified.serial != record.serial {
        return Err(format!(
            "presented SVID serial {} does not match the issued serial {}",
            verified.serial, record.serial
        ));
    }

    // 4. Stack binding: the presented stack fingerprint must be non-empty
    //    and match the fingerprint bound to the SVID-backed record. An
    //    empty or tampered stack string fails verification.
    let bound_stack = bind_stack_fingerprint(agent_id, &record, presented_stack).await?;

    Ok(WorkloadVerification {
        agent_id: agent_id.to_string(),
        spiffe_id: verified.spiffe_id.to_string(),
        serial: verified.serial,
        not_after: verified.not_after,
        stack_fingerprint: bound_stack,
    })
}

/// Stack-binding check against the SVID-backed record:
/// - the presented fingerprint must be non-empty (an empty stack string is
///   a rejected presentation, never a skipped check);
/// - if the record already carries a bound fingerprint (spawn-time evidence
///   or a prior attestation), the presentation must match it exactly —
///   tampering between attestation and verification is caught here;
/// - a first-time presentation binds the presented fingerprint to the
///   record (the SVID presentation is what converts the self-report into
///   attested evidence) and must agree with any stack the workload
///   registered through the M14.1 self-report channel.
async fn bind_stack_fingerprint(
    agent_id: &str,
    record: &SvidRecord,
    presented_stack: &str,
) -> Result<String, String> {
    if presented_stack.trim().is_empty() {
        return Err("no stack fingerprint presented".to_string());
    }
    if record.stack_fingerprint.is_empty() {
        // Issuance always binds non-empty evidence; an empty bound value
        // means the record is not a product of the issuance path.
        return Err(format!(
            "SVID-backed record for agent {agent_id} carries no bound stack fingerprint"
        ));
    }
    if record.stack_fingerprint != presented_stack {
        return Err(format!(
            "stack fingerprint mismatch: presented {presented_stack} but the SVID-backed record binds {}",
            record.stack_fingerprint
        ));
    }
    Ok(presented_stack.to_string())
}

/// Verify a presented SVID for an agent session and apply the result to
/// the workload attestation record (alive / last_verified_at / svid
/// fields). Shared by the REST attest endpoint, the heartbeat path, and
/// the MCP heartbeat tool — one verification surface, no bypass variants.
pub async fn attest_session(
    state: &AppState,
    agent_id: &str,
    presented_svid: &str,
    presented_stack: &str,
) -> Result<WorkloadVerification, String> {
    let result = verify_workload_identity(state, agent_id, presented_svid, presented_stack).await;
    apply_verification_to_attestation(state, agent_id, &result).await;
    result
}

/// Apply a verification outcome to the agent's workload attestation record
/// (G10's `alive` flag is now the SVID verification result; the record is
/// metadata — never a verification input).
pub async fn apply_verification_to_attestation(
    state: &AppState,
    agent_id: &str,
    result: &Result<WorkloadVerification, String>,
) {
    if let Ok(Some(json)) = state
        .kv_store
        .kv_get("workload_attestations", agent_id)
        .await
    {
        if let Ok(mut att) =
            serde_json::from_str::<crate::workload_attestation::WorkloadAttestation>(&json)
        {
            att.last_verified_at = now_secs();
            match result {
                Ok(verified) => {
                    att.alive = true;
                    att.svid_verified_at = Some(now_secs());
                    att.spiffe_id = Some(verified.spiffe_id.clone());
                    att.svid_serial = Some(verified.serial.clone());
                }
                Err(reason) => {
                    att.alive = false;
                    tracing::warn!(
                        agent_id = %agent_id,
                        reason = %reason,
                        "workload-identity verification failed"
                    );
                }
            }
            if let Ok(updated) = serde_json::to_string(&att) {
                let _ = state
                    .kv_store
                    .kv_set("workload_attestations", agent_id, updated)
                    .await;
            }
        }
    }
}

/// GET /.well-known/spiffe/keys.json — the trust bundle in SPIFFE bundle
/// format, the sole verification anchor. Public (like the OIDC JWKS
/// endpoint): anyone must be able to verify SVIDs.
pub async fn trust_bundle_endpoint(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.spiffe.bundle_json().await {
        Ok(json) => (
            StatusCode::OK,
            [(
                axum::http::header::CONTENT_TYPE,
                "application/json".to_string(),
            )],
            json,
        )
            .into_response(),
        Err(reason) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": reason})),
        )
            .into_response(),
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_role_prefers_persona() {
        use gyre_domain::OrchestratorType;
        assert_eq!(
            agent_role(Some("security"), &OrchestratorType::Worker),
            "security"
        );
        assert_eq!(agent_role(None, &OrchestratorType::Worker), "worker");
        assert_eq!(
            agent_role(None, &OrchestratorType::RepoOrchestrator),
            "repo-orchestrator"
        );
        assert_eq!(
            agent_role(Some(""), &OrchestratorType::WorkspaceOrchestrator),
            "workspace-orchestrator"
        );
    }

    /// identity-security.md §Federation binding invariant, pinned at the
    /// production issuer: the SPIFFE ID session segment of a session SVID
    /// equals the JWT `sub` the Layer-2 mint embeds for the SAME agent.
    /// The federated verifier compares the two by exact string comparison
    /// (`/agent/{role}/session/{jwt sub}`, spiffe_federation.rs), so any
    /// drift in either form breaks federated workload attestation for
    /// every upgraded issuer (review round 69d50ca9 F1 — the sub-form mint
    /// change left the SVID session segment on the bare agent id).
    #[tokio::test]
    async fn session_svid_session_segment_equals_jwt_sub() {
        let state = crate::mem::test_state();
        let key = crate::auth::AgentSigningKey::generate();
        let agent_id = "svid-sub-agent-1";
        // The REAL Layer-2 mint for this agent, validated back through the
        // real decoder — not a re-derived constant.
        let token = key
            .mint(
                agent_id,
                "task-1",
                "system",
                "http://gyre.local",
                3600,
                "tenant-1",
                "ws-1",
                "repo-1",
                "repo-1-name",
            )
            .expect("agent JWT mint must succeed");
        let sub = key
            .validate(&token, "http://gyre.local")
            .expect("minted agent JWT must validate")
            .sub;
        // The REAL Layer-1 issuance for the same agent.
        let svid = issue_session_svid(
            &state,
            agent_id,
            "worker",
            "local",
            "sha256:invariant-test-stack",
        )
        .await
        .expect("session SVID issuance must succeed");
        let spiffe_id =
            SpiffeId::parse(&svid.record.spiffe_id).expect("issued SPIFFE ID must parse");
        assert_eq!(
            spiffe_id.agent_path().expect("agent path").session_id,
            sub,
            "SVID session segment must equal the JWT sub \
             (federated binding invariant /agent/{{role}}/session/{{jwt sub}})"
        );
    }
}
