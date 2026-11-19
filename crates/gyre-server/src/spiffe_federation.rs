//! SPIFFE federation: trust-bundle exchange between Gyre instances
//! (identity-security.md §Federation — "SPIFFE federation handles the
//! workload attestation layer independently").
//!
//! The OIDC-issuer federation layer (Layer 2: `GYRE_TRUSTED_ISSUERS`,
//! `auth::validate_federated_jwt` with remote JWKS discovery) is
//! independent of this module and stays as-is. This module federates the
//! Layer-1 workload-attestation layer per the SPIFFE federation model:
//!
//! - **Bundle endpoints keyed by trust domain.** A peer is configured as
//!   `trust_domain=endpoint_url` in `GYRE_SPIFFE_FEDERATION_PEERS`. The
//!   endpoint is the peer's public SPIFFE trust bundle (the document it
//!   serves at `/.well-known/spiffe/keys.json` in SPIFFE bundle format —
//!   a JWK Set with `use: "svid"` whose `x5c` members carry the trust
//!   domain's CA certificates). When only an origin is configured the
//!   standard well-known path is appended.
//! - **Refresh on interval.** Exchanged bundles are cached and re-fetched
//!   from the peer's endpoint once the configured refresh interval
//!   (`GYRE_SPIFFE_FEDERATION_REFRESH_SECS`, default 300 s) expires, so a
//!   peer's bundle rotation is picked up within the interval. A refresh
//!   that cannot be completed fails verification closed — verification
//!   follows the *refreshed* bundle, never a silently-stale one.
//! - **No static credential material is exchanged.** The federation
//!   configuration consists of issuer/trust-domain endpoints only; all
//!   key material (anchors) is fetched from the public bundle endpoint,
//!   exactly as the OIDC layer fetches keys via public JWKS discovery.
//!
//! The verification surface ([`attest_federated_session`]) is fail-closed:
//! a federated agent's SVID is accepted only if
//! 1. it parses and carries exactly one URI SAN with a well-formed
//!    agent-workload SPIFFE ID,
//! 2. the SPIFFE ID's trust domain has an **exchanged** bundle configured
//!    on this instance (a trust domain whose bundle was never exchanged
//!    is rejected — even when the caller's OIDC JWT from that instance's
//!    issuer is trusted; the two layers are independent),
//! 3. the leaf is cryptographically signed by a key in that trust
//!    domain's exchanged bundle (chain validation, validity windows),
//! 4. the SPIFFE ID names the caller's own remote session
//!    (`/agent/{role}/session/{jwt sub}`) — a federated agent cannot
//!    present another session's SVID.
//!
//! Neither layer substitutes for the other: the caller's Layer-2 JWT
//! authenticates *who* is presenting (and its `sub` binds the expected
//! session), while the Layer-1 SVID is validated solely against the
//! exchanged SPIFFE bundle. The JWT never vouches for the SVID's
//! cryptographic validity, and a valid SVID never authenticates a caller.

use crate::workload_attestation::WorkloadAttestation;
use gyre_spiffe::federation::BundleSource;
use std::sync::Arc;
use std::time::Duration;

use crate::AppState;

/// Env var holding the federation peers: comma-separated
/// `trust_domain=endpoint_url` entries (the endpoint being the peer's
/// public SPIFFE trust-bundle URL; a bare origin gets the standard
/// `/.well-known/spiffe/keys.json` path appended).
pub const FEDERATION_PEERS_ENV: &str = "GYRE_SPIFFE_FEDERATION_PEERS";

/// Env var for the exchanged-bundle refresh interval in seconds.
pub const FEDERATION_REFRESH_SECS_ENV: &str = "GYRE_SPIFFE_FEDERATION_REFRESH_SECS";

/// Default refresh interval for exchanged bundles (mirrors the OIDC
/// federation's 5-minute JWKS cache TTL).
const DEFAULT_REFRESH_SECS: u64 = 300;

/// Bound on the bundle-endpoint fetch (reqwest has no default timeout; an
/// unresponsive peer endpoint must fail verification, never stall it).
const BUNDLE_FETCH_TIMEOUT: Duration = Duration::from_secs(10);

// ---------------------------------------------------------------------------
// Peer configuration
// ---------------------------------------------------------------------------

/// One federation peer: a trust domain and the public endpoint its trust
/// bundle is fetched from. No key material lives here — the anchors are
/// always fetched from the endpoint (identity-security.md §Federation:
/// "no static credentials exchanged between instances").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederationPeer {
    pub trust_domain: String,
    pub bundle_endpoint: String,
}

/// Parse one `trust_domain=endpoint_url` peer entry.
///
/// Fail-closed validation:
/// - the trust domain must be usable in a SPIFFE ID (charset per the
///   SPIFFE ID specification) and must differ from this instance's own
///   trust domain (an SVID for the local trust domain is verified by the
///   LOCAL surface against the LOCAL bundle, never by an exchanged one);
/// - the endpoint must be an absolute http(s) URL; a bare origin gets the
///   standard well-known bundle path appended.
pub fn parse_peer_entry(entry: &str, local_trust_domain: &str) -> Result<FederationPeer, String> {
    let entry = entry.trim();
    if entry.is_empty() {
        return Err("empty peer entry".to_string());
    }
    let (trust_domain, endpoint) = entry
        .split_once('=')
        .ok_or_else(|| format!("peer entry {entry:?} must be trust_domain=endpoint_url"))?;
    let trust_domain = trust_domain.trim().to_lowercase();
    // Validate the trust domain through the SPIFFE ID parser (charset,
    // non-empty) — a domain that cannot appear in a SPIFFE ID can never
    // match an SVID's trust domain.
    gyre_spiffe::SpiffeId::agent(&trust_domain, "probe", "probe")
        .map_err(|e| format!("peer trust domain {trust_domain:?} is invalid: {e}"))?;
    if trust_domain == local_trust_domain {
        return Err(format!(
            "peer trust domain {trust_domain:?} equals this instance's own trust domain; \
             local SVIDs are verified by the local surface"
        ));
    }
    let mut url = url::Url::parse(endpoint.trim())
        .map_err(|e| format!("peer endpoint {endpoint:?} is not a valid URL: {e}"))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(format!(
            "peer endpoint {endpoint:?} must be http(s); refusing scheme {:?}",
            url.scheme()
        ));
    }
    // A bare origin (no path) refers to the standard well-known location
    // at which Gyre (and SPIFFE-conformant peers) publish their bundle.
    if url.path() == "/" || url.path().is_empty() {
        url.set_path(crate::spiffe::TRUST_BUNDLE_PATH);
    }
    Ok(FederationPeer {
        trust_domain,
        bundle_endpoint: url.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Exchanged-bundle registry
// ---------------------------------------------------------------------------

/// The host-side bundle source: fetches a peer's trust bundle from its
/// public bundle endpoint over bounded HTTP. All key material enters
/// verification through this fetch — the federation configuration
/// carries endpoints only, never credentials (identity-security.md
/// §Federation: "no static credentials exchanged between instances").
struct HttpBundleSource {
    peers: Vec<FederationPeer>,
    /// Bounded HTTP client for bundle-endpoint fetches.
    http: reqwest::Client,
}

#[async_trait::async_trait]
impl BundleSource for HttpBundleSource {
    async fn fetch(&self, trust_domain: &str) -> Result<Vec<Vec<u8>>, String> {
        let peer = self
            .peers
            .iter()
            .find(|p| p.trust_domain == trust_domain)
            .ok_or_else(|| {
                format!(
                    "trust domain {trust_domain:?} is not federated: no SPIFFE trust bundle \
                 has been exchanged with this instance"
                )
            })?;
        let response = self
            .http
            .get(&peer.bundle_endpoint)
            .send()
            .await
            .map_err(|e| {
                format!(
                    "federation bundle fetch from {} failed: {e}",
                    peer.bundle_endpoint
                )
            })?;
        if !response.status().is_success() {
            return Err(format!(
                "federation bundle endpoint {} returned {}",
                peer.bundle_endpoint,
                response.status()
            ));
        }
        let body = response.text().await.map_err(|e| {
            format!(
                "federation bundle from {} is unreadable: {e}",
                peer.bundle_endpoint
            )
        })?;
        let value: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
            format!(
                "federation bundle from {} is not JSON: {e}",
                peer.bundle_endpoint
            )
        })?;
        gyre_spiffe::parse_trust_bundle(&value).map_err(|e| {
            format!(
                "federation bundle from {} is not a valid SPIFFE bundle: {e}",
                peer.bundle_endpoint
            )
        })
    }
}

/// The SPIFFE trust-bundle exchange registry: the configured federation
/// peers and their exchanged bundles (keyed by trust domain, refreshed on
/// interval). The registry semantics live in `gyre_spiffe::federation`
/// (`ExchangedBundles`); this wrapper adds the peer configuration and the
/// HTTP bundle source.
pub struct FederationBundles {
    peers: Vec<FederationPeer>,
    inner: gyre_spiffe::federation::ExchangedBundles,
}

impl FederationBundles {
    /// Build the registry from validated peers. Callers construct this via
    /// [`FederationBundles::from_env`] (production) or directly (tests).
    pub fn new(peers: Vec<FederationPeer>, refresh_interval: Duration) -> Self {
        let source = Arc::new(HttpBundleSource {
            peers: peers.clone(),
            http: reqwest::Client::builder()
                .timeout(BUNDLE_FETCH_TIMEOUT)
                .build()
                .expect("federation bundle HTTP client construction must succeed"),
        });
        let domains = peers.iter().map(|p| p.trust_domain.clone()).collect();
        Self {
            peers,
            inner: gyre_spiffe::federation::ExchangedBundles::new(
                domains,
                refresh_interval,
                source,
            ),
        }
    }

    /// Build the registry from the environment
    /// (`GYRE_SPIFFE_FEDERATION_PEERS`, `GYRE_SPIFFE_FEDERATION_REFRESH_SECS`).
    ///
    /// Invalid peer entries are skipped with a loud warning — one bad
    /// entry must not silently break the whole registry — but never
    /// partially accepted: an entry either validates whole or is dropped.
    pub fn from_env(local_trust_domain: &str) -> Self {
        let entries: Vec<String> = std::env::var(FEDERATION_PEERS_ENV)
            .ok()
            .map(|v| {
                v.split(',')
                    .map(str::to_string)
                    .filter(|e| !e.trim().is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let peers: Vec<FederationPeer> = entries
            .iter()
            .filter_map(|entry| match parse_peer_entry(entry, local_trust_domain) {
                Ok(peer) => Some(peer),
                Err(reason) => {
                    tracing::warn!(
                        entry = %entry,
                        reason = %reason,
                        "SPIFFE federation: skipping invalid peer entry"
                    );
                    None
                }
            })
            .collect();
        let refresh_secs = std::env::var(FEDERATION_REFRESH_SECS_ENV)
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|s| *s > 0)
            .unwrap_or(DEFAULT_REFRESH_SECS);
        let registry = Self::new(peers, Duration::from_secs(refresh_secs));
        if !registry.peers.is_empty() {
            tracing::info!(
                peers = ?registry.peers.iter().map(|p| p.trust_domain.clone()).collect::<Vec<_>>(),
                refresh_secs,
                "SPIFFE federation: exchanging trust bundles with configured peers"
            );
        }
        registry
    }

    /// The configured peers (trust domains + public bundle endpoints —
    /// endpoints only, never key material).
    pub fn peers(&self) -> &[FederationPeer] {
        &self.peers
    }

    /// Whether a trust domain has an exchanged bundle configured.
    pub fn is_federated(&self, trust_domain: &str) -> bool {
        self.inner.is_federated(trust_domain)
    }

    /// The exchanged verification anchors (DER CA certificates) for a
    /// trust domain, refreshing from the peer's bundle endpoint when the
    /// cached copy is older than the refresh interval. Fail-closed (see
    /// `gyre_spiffe::federation::ExchangedBundles::anchors_for`): a
    /// domain with no exchanged bundle and a refresh that cannot be
    /// completed are both errors — never an empty anchor set, never a
    /// silently-stale bundle.
    pub async fn anchors_for(&self, trust_domain: &str) -> Result<Vec<Vec<u8>>, String> {
        self.inner.anchors_for(trust_domain).await
    }

    /// The federated workload-identity verification decision
    /// (identity-security.md Layer 1, federation): see
    /// `gyre_spiffe::federation::verify_federated_svid`. `caller_sub` is
    /// the remote session id from the caller's (already Layer-2-verified)
    /// JWT — used only to bind the SVID to the presenter's own session,
    /// never as a trust anchor.
    pub async fn verify_svid_for_caller(
        &self,
        caller_sub: &str,
        presented_pem: &str,
    ) -> Result<gyre_spiffe::VerifiedSvid, String> {
        gyre_spiffe::federation::verify_federated_svid(presented_pem, caller_sub, &self.inner).await
    }
}

// ---------------------------------------------------------------------------
// Federated workload-identity verification (Layer 1)
// ---------------------------------------------------------------------------

/// A successful federated workload-identity verification: the caller
/// presented an SVID that cryptographically validates against the
/// exchanged bundle of its trust domain and names the caller's own remote
/// session.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FederatedWorkloadVerification {
    /// The caller's federated agent id on this instance
    /// (`{remote-host}/{jwt sub}`).
    pub agent_id: String,
    /// The SPIFFE ID extracted from the verified SVID — the same identity
    /// the issuing instance's own verifiers extract.
    pub spiffe_id: String,
    /// The SVID's trust domain (the federated peer's domain).
    pub trust_domain: String,
    /// Role segment of the SPIFFE ID path (the peer's spawn-time role).
    pub role: String,
    /// Session segment of the SPIFFE ID path (the caller's remote `sub`).
    pub session_id: String,
    /// Leaf serial (canonical hex).
    pub serial: String,
    /// Leaf not-after (Unix epoch seconds).
    pub not_after: i64,
}

/// The federated workload-identity verification surface
/// (identity-security.md Layer 1, federation): verify a federated agent's
/// presented X.509-SVID against the trust bundle exchanged with the
/// SVID's trust domain, and record the outcome in the caller's workload
/// attestation record.
///
/// `caller_agent_id` is the caller's federated agent id
/// (`{remote-host}/{sub}`); `caller_sub` is the remote session id from the
/// caller's (already Layer-2-verified) JWT — used ONLY to bind the SVID
/// to the presenter's own session, never as a trust anchor. `claims` is
/// the caller's JWT claims (metadata for the attestation record).
///
/// Every check fails closed; see the module documentation.
pub async fn attest_federated_session(
    state: &AppState,
    caller_agent_id: &str,
    caller_sub: &str,
    presented_svid: &str,
    claims: Option<&serde_json::Value>,
) -> Result<FederatedWorkloadVerification, String> {
    let result = verify_federated_svid(state, caller_agent_id, caller_sub, presented_svid).await;
    match &result {
        Ok(verification) => {
            record_federated_outcome(state, caller_agent_id, claims, Some(verification)).await
        }
        Err(reason) => {
            tracing::warn!(
                agent_id = %caller_agent_id,
                reason = %reason,
                "federated workload-identity verification failed"
            );
            record_federated_failure(state, caller_agent_id).await;
        }
    }
    result
}

async fn verify_federated_svid(
    state: &AppState,
    caller_agent_id: &str,
    caller_sub: &str,
    presented_svid: &str,
) -> Result<FederatedWorkloadVerification, String> {
    // The full fail-closed decision chain (trust-domain-keyed bundle
    // lookup, chain validation against the exchanged anchors, session
    // binding to the caller's `sub`) lives in
    // gyre_spiffe::federation::verify_federated_svid.
    let verified = state
        .spiffe_federation
        .verify_svid_for_caller(caller_sub, presented_svid)
        .await?;

    Ok(FederatedWorkloadVerification {
        agent_id: caller_agent_id.to_string(),
        spiffe_id: verified.spiffe_id.to_string(),
        trust_domain: verified.spiffe_id.trust_domain.clone(),
        role: verified.agent_path.role.clone(),
        session_id: verified.agent_path.session_id.clone(),
        serial: verified.serial.clone(),
        not_after: verified.not_after,
    })
}

/// Apply a federated verification outcome to the caller's workload
/// attestation record (`workload_attestations` KV namespace, keyed by the
/// federated agent id).
///
/// - Success upserts the record: a first successful presentation creates
///   it (metadata from the caller's JWT `wl_*` claims — the remote
///   instance's mint, recorded as metadata, never a verification input),
///   a repeat presentation refreshes the verification fields.
/// - Failure flips an EXISTING record to `alive=false` (a previously
///   attested federated agent whose SVID no longer verifies — expired,
///   rotated away, or from a no-longer-federated domain — must not stay
///   reported as attested); a first failed presentation creates nothing.
async fn record_federated_outcome(
    state: &AppState,
    agent_id: &str,
    claims: Option<&serde_json::Value>,
    outcome: Option<&FederatedWorkloadVerification>,
) {
    let existing = state
        .kv_store
        .kv_get("workload_attestations", agent_id)
        .await
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str::<WorkloadAttestation>(&s).ok());
    let now = now_secs();
    let record = match (existing, outcome) {
        (Some(mut att), Some(verification)) => {
            att.alive = true;
            att.last_verified_at = now;
            att.svid_verified_at = Some(now);
            att.spiffe_id = Some(verification.spiffe_id.clone());
            att.svid_serial = Some(verification.serial.clone());
            att
        }
        (Some(mut att), None) => {
            // Failed re-verification: the record must stop reporting the
            // workload as attested (fail closed on the reporting side).
            att.alive = false;
            att.last_verified_at = now;
            att.svid_verified_at = None;
            att
        }
        (None, Some(verification)) => WorkloadAttestation {
            agent_id: agent_id.to_string(),
            // Metadata from the remote instance's JWT mint, if present.
            pid: claims
                .and_then(|c| c.get("wl_pid"))
                .and_then(|v| v.as_u64())
                .map(|p| p as u32),
            hostname: claims
                .and_then(|c| c.get("wl_hostname"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            compute_target: claims
                .and_then(|c| c.get("wl_compute_target"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("federated:{}", verification.trust_domain)),
            stack_fingerprint: claims
                .and_then(|c| c.get("wl_stack_hash"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            attested_at: now,
            alive: true,
            last_verified_at: now,
            container_id: claims
                .and_then(|c| c.get("wl_container_id"))
                .and_then(|v| v.as_str())
                .map(str::to_string),
            image_hash: claims
                .and_then(|c| c.get("wl_image_hash"))
                .and_then(|v| v.as_str())
                .map(str::to_string),
            spiffe_id: Some(verification.spiffe_id.clone()),
            svid_serial: Some(verification.serial.clone()),
            svid_verified_at: Some(now),
        },
        (None, None) => return,
    };
    if let Ok(json) = serde_json::to_string(&record) {
        if let Err(e) = state
            .kv_store
            .kv_set("workload_attestations", agent_id, json)
            .await
        {
            tracing::warn!(
                agent_id = %agent_id,
                error = %e,
                "SPIFFE federation: could not persist the workload attestation record"
            );
        }
    }
}

/// Record a failed federated verification attempt against the caller's
/// existing attestation record (flips `alive` to false; creates nothing).
async fn record_federated_failure(state: &AppState, agent_id: &str) {
    record_federated_outcome(state, agent_id, None, None).await;
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod test_support {
    //! A minimal HTTP/1.1 server for exercising the REAL fetch paths
    //! (bundle endpoint, OIDC discovery, JWKS) against a local listener —
    //! no HTTP client seams, no injected caches.

    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Serve `handler(method, path) -> (status, body)` until the process
    /// drops the last reference. Each connection is served on its own
    /// thread; responses are `Connection: close`.
    pub(crate) struct TestHttpServer {
        pub origin: String,
        pub request_count: Arc<AtomicUsize>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl TestHttpServer {
        pub fn requests_served(&self) -> usize {
            self.request_count.load(Ordering::SeqCst)
        }
    }

    impl Drop for TestHttpServer {
        fn drop(&mut self) {
            self.handle.take();
            // The accept loop exits when the listener is dropped by the
            // last Arc clone; leaked handler threads park on accept and
            // are cleaned up at process exit (test-only).
        }
    }

    impl TestHttpServer {
        pub(crate) fn spawn<F>(handler: F) -> TestHttpServer
        where
            F: Fn(&str, &str) -> (u16, String) + Send + Sync + 'static,
        {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind test HTTP server");
            let origin = format!("http://{}", listener.local_addr().unwrap());
            let handler = Arc::new(handler);
            let request_count = Arc::new(AtomicUsize::new(0));
            let counter = Arc::clone(&request_count);
            let listener = Arc::new(listener);
            let l = Arc::clone(&listener);
            let handle = std::thread::spawn(move || {
                while let Ok((stream, _)) = l.accept() {
                    let handler = Arc::clone(&handler);
                    let counter = Arc::clone(&counter);
                    std::thread::spawn(move || serve_connection(stream, handler, counter));
                }
            });
            let _ = listener; // keep the listener alive for the thread lifetime
            TestHttpServer {
                origin,
                request_count,
                handle: Some(handle),
            }
        }
    }

    fn serve_connection<F>(mut stream: TcpStream, handler: Arc<F>, counter: Arc<AtomicUsize>)
    where
        F: Fn(&str, &str) -> (u16, String),
    {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        // Read headers (and any Content-Length body) — bounded so a broken
        // client cannot hang the test thread forever.
        let header_end = loop {
            match stream.read(&mut chunk) {
                Ok(0) => return,
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(pos) = find_header_end(&buf) {
                        break pos;
                    }
                    if buf.len() > 64 * 1024 {
                        return;
                    }
                }
                Err(_) => return,
            }
        };
        let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
        let content_length = head
            .lines()
            .find_map(|l| {
                let (name, value) = l.split_once(':')?;
                if name.trim().eq_ignore_ascii_case("content-length") {
                    value.trim().parse::<usize>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0);
        while buf.len() < header_end + 4 + content_length {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(_) => return,
            }
        }
        let mut parts = head.lines().next().unwrap_or("").split_whitespace();
        let method = parts.next().unwrap_or("GET").to_string();
        let target = parts.next().unwrap_or("/").to_string();
        let path = target.split('?').next().unwrap_or("/").to_string();

        counter.fetch_add(1, Ordering::SeqCst);
        let (status, body) = handler(&method, &path);
        let reason = match status {
            200 => "OK",
            404 => "Not Found",
            _ => "Error",
        };
        let response = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
    }

    fn find_header_end(buf: &[u8]) -> Option<usize> {
        buf.windows(4).position(|w| w == b"\r\n\r\n")
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::TestHttpServer;
    use super::*;
    use gyre_spiffe::{SpiffeId, TrustDomainCa};
    use std::sync::Arc;

    const LOCAL_TD: &str = "gyre.example.com";
    const PEER_TD: &str = "gyre-a.example.com";

    fn peer_ca_bundle(ca: &TrustDomainCa) -> String {
        gyre_spiffe::trust_bundle_jwks(&[ca.cert_pem()]).to_string()
    }

    /// A state whose federation registry exchanges bundles for PEER_TD
    /// from the given HTTP origin.
    fn state_with_peer(origin: &str, refresh: Duration) -> Arc<AppState> {
        let base = crate::mem::test_state();
        let mut state = (*base).clone();
        state.spiffe_federation = Arc::new(FederationBundles::new(
            vec![FederationPeer {
                trust_domain: PEER_TD.to_string(),
                bundle_endpoint: format!("{origin}/.well-known/spiffe/keys.json"),
            }],
            refresh,
        ));
        Arc::new(state)
    }

    fn plain_state() -> Arc<AppState> {
        let base = crate::mem::test_state();
        let mut state = (*base).clone();
        state.spiffe_federation =
            Arc::new(FederationBundles::new(vec![], Duration::from_secs(300)));
        Arc::new(state)
    }

    #[test]
    fn parse_peer_entry_validates_and_defaults_path() {
        let peer = parse_peer_entry("Peer-A.example.com=https://peer-a.example.com", LOCAL_TD)
            .expect("valid entry must parse");
        assert_eq!(peer.trust_domain, "peer-a.example.com");
        assert_eq!(
            peer.bundle_endpoint,
            "https://peer-a.example.com/.well-known/spiffe/keys.json"
        );

        let explicit = parse_peer_entry(
            "peer-a.example.com=https://peer-a.example.com/bundle.json",
            LOCAL_TD,
        )
        .expect("explicit endpoint must parse");
        assert_eq!(
            explicit.bundle_endpoint,
            "https://peer-a.example.com/bundle.json"
        );

        // Rejections: missing '=', invalid trust domain charset, local
        // trust domain, non-http scheme, garbage URL.
        assert!(parse_peer_entry("no-separator-here", LOCAL_TD).is_err());
        assert!(parse_peer_entry("Bad_Domain!.example=https://x.example", LOCAL_TD).is_err());
        assert!(parse_peer_entry("gyre.example.com=https://x.example", LOCAL_TD).is_err());
        assert!(parse_peer_entry("a.example=ftp://x.example", LOCAL_TD).is_err());
        assert!(parse_peer_entry("a.example=not a url", LOCAL_TD).is_err());
        assert!(parse_peer_entry("", LOCAL_TD).is_err());
    }

    /// AC1/AC2 core probe at the service level: the SAME A-trust-domain
    /// SVID verifies when the bundle is exchanged and is rejected when it
    /// is not — the two configurations are distinguished.
    #[tokio::test]
    async fn federated_svid_verifies_only_with_exchanged_bundle() {
        let ca_a = TrustDomainCa::generate(PEER_TD).expect("peer CA generation");
        let bundle = peer_ca_bundle(&ca_a);
        let svid = ca_a
            .issue_svid(
                &SpiffeId::agent(PEER_TD, "worker", "fed-session-1").expect("spiffe id"),
                gyre_spiffe::BASE_SVID_TTL_SECS,
            )
            .expect("peer SVID issuance");

        let server = TestHttpServer::spawn(move |_m, _p| (200, bundle.clone()));

        // (a) B trusts the OIDC issuer (simulated by the caller being
        // authenticated — here the service call) but has NOT exchanged
        // A's bundle: the SVID must NOT verify.
        let state = plain_state();
        let err = attest_federated_session(
            &state,
            "peer-a.example/fed-session-1",
            "fed-session-1",
            &svid.chain_pem,
            None,
        )
        .await
        .expect_err("unfederated trust domain must be rejected");
        assert!(err.contains("not federated"), "unexpected error: {err}");

        // (b) the bundle is exchanged: the same SVID verifies and yields
        // the same SPIFFE ID A's own verifiers would extract.
        let state = state_with_peer(&server.origin, Duration::from_secs(300));
        let verification = attest_federated_session(
            &state,
            "peer-a.example/fed-session-1",
            "fed-session-1",
            &svid.chain_pem,
            None,
        )
        .await
        .expect("exchanged-bundle SVID must verify");
        assert_eq!(
            verification.spiffe_id,
            "spiffe://gyre-a.example.com/agent/worker/session/fed-session-1"
        );
        assert_eq!(verification.trust_domain, PEER_TD);
        assert_eq!(verification.session_id, "fed-session-1");
        // The A-side cross-check (the same SPIFFE ID and serial A's own
        // verifiers extract from the same anchors) is proven at the
        // protocol level in gyre-spiffe's federation tests.

        // The bundle was actually fetched from the endpoint (not seeded,
        // not configured key material).
        assert!(server.requests_served() >= 1);
    }

    /// AC2: an SVID from a trust domain whose bundle was never exchanged
    /// is rejected EVEN when a different peer's bundle IS exchanged (the
    /// presence of one federation must not open the door to all domains).
    #[tokio::test]
    async fn never_exchanged_trust_domain_rejected_alongside_federated_peer() {
        let ca_a = TrustDomainCa::generate(PEER_TD).expect("peer CA generation");
        let server = TestHttpServer::spawn(move |_m, _p| (200, peer_ca_bundle(&ca_a)));
        let state = state_with_peer(&server.origin, Duration::from_secs(300));

        // A genuine SVID from a THIRD instance's CA (gyre-c.example.org),
        // whose bundle was never exchanged with B.
        let ca_c = TrustDomainCa::generate("gyre-c.example.org").expect("third CA generation");
        let svid = ca_c
            .issue_svid(
                &SpiffeId::agent("gyre-c.example.org", "worker", "fed-session-1").expect("id"),
                gyre_spiffe::BASE_SVID_TTL_SECS,
            )
            .expect("third SVID issuance");
        let err = attest_federated_session(
            &state,
            "peer-a.example/fed-session-1",
            "fed-session-1",
            &svid.chain_pem,
            None,
        )
        .await
        .expect_err("never-exchanged trust domain must be rejected");
        assert!(err.contains("not federated"), "unexpected error: {err}");
    }

    /// A rogue SVID claiming a federated trust domain but signed by a key
    /// NOT in the exchanged bundle is rejected by the chain validation.
    #[tokio::test]
    async fn rogue_signer_for_federated_domain_rejected() {
        let honest = TrustDomainCa::generate(PEER_TD).expect("honest CA generation");
        let server = TestHttpServer::spawn(move |_m, _p| (200, peer_ca_bundle(&honest)));
        let state = state_with_peer(&server.origin, Duration::from_secs(300));

        let rogue = TrustDomainCa::generate(PEER_TD).expect("rogue CA generation");
        let svid = rogue
            .issue_svid(
                &SpiffeId::agent(PEER_TD, "worker", "fed-session-1").expect("id"),
                gyre_spiffe::BASE_SVID_TTL_SECS,
            )
            .expect("rogue SVID issuance");
        let err = attest_federated_session(
            &state,
            "peer-a.example/fed-session-1",
            "fed-session-1",
            &svid.chain_pem,
            None,
        )
        .await
        .expect_err("rogue signer must be rejected");
        assert!(
            err.contains("UntrustedSigner") || err.contains("not signed"),
            "unexpected error: {err}"
        );
    }

    /// Session binding: a federated agent cannot present another remote
    /// session's SVID (the SVID must name the caller's own `sub`).
    #[tokio::test]
    async fn other_sessions_svid_rejected() {
        let ca_a = TrustDomainCa::generate(PEER_TD).expect("peer CA generation");
        let bundle = peer_ca_bundle(&ca_a);
        let server = TestHttpServer::spawn(move |_m, _p| (200, bundle.clone()));
        let state = state_with_peer(&server.origin, Duration::from_secs(300));

        let svid = ca_a
            .issue_svid(
                &SpiffeId::agent(PEER_TD, "worker", "someone-elses-session").expect("id"),
                gyre_spiffe::BASE_SVID_TTL_SECS,
            )
            .expect("SVID issuance");
        let err = attest_federated_session(
            &state,
            "peer-a.example/fed-session-1",
            "fed-session-1",
            &svid.chain_pem,
            None,
        )
        .await
        .expect_err("another session's SVID must be rejected");
        assert!(err.contains("session"), "unexpected error: {err}");
    }

    /// AC3: bundle refresh/rotation. After the peer rotates its bundle,
    /// B keeps the old anchors only until the refresh interval expires —
    /// a rotated (new-CA) SVID fails closed against the old bundle, and
    /// verifies after B re-fetches within the interval; verification then
    /// FOLLOWS the refreshed bundle (the old-CA SVID stops verifying).
    #[tokio::test]
    async fn bundle_rotation_follows_refreshed_bundle() {
        let ca_v1 = TrustDomainCa::generate(PEER_TD).expect("CA v1");
        let ca_v2 = TrustDomainCa::generate(PEER_TD).expect("CA v2");
        let svid_v1 = ca_v1
            .issue_svid(
                &SpiffeId::agent(PEER_TD, "worker", "fed-session-1").expect("id"),
                gyre_spiffe::BASE_SVID_TTL_SECS,
            )
            .expect("SVID v1");
        let svid_v2 = ca_v2
            .issue_svid(
                &SpiffeId::agent(PEER_TD, "worker", "fed-session-1").expect("id"),
                gyre_spiffe::BASE_SVID_TTL_SECS,
            )
            .expect("SVID v2");

        // The peer's endpoint serves v1 now; rotation swaps the served
        // document under B's feet.
        let served = Arc::new(std::sync::Mutex::new(peer_ca_bundle(&ca_v1)));
        let served_for_server = Arc::clone(&served);
        let server = TestHttpServer::spawn(move |_m, _p| {
            let body = served_for_server.lock().unwrap().clone();
            (200, body)
        });
        let refresh = Duration::from_millis(300);
        let state = state_with_peer(&server.origin, refresh);

        // Pre-rotation: the v1 SVID verifies (bundle fetched + cached).
        attest_federated_session(
            &state,
            "peer-a.example/fed-session-1",
            "fed-session-1",
            &svid_v1.chain_pem,
            None,
        )
        .await
        .expect("v1 SVID must verify against the exchanged v1 bundle");
        let fetches_after_first = server.requests_served();
        assert!(fetches_after_first >= 1);

        // Rotate: the peer now serves the v2 bundle.
        *served.lock().unwrap() = peer_ca_bundle(&ca_v2);

        // While B's cache is still fresh (within the interval), the
        // rotated SVID fails closed against the old-bundle-only cache.
        let err = attest_federated_session(
            &state,
            "peer-a.example/fed-session-1",
            "fed-session-1",
            &svid_v2.chain_pem,
            None,
        )
        .await
        .expect_err("rotated SVID must fail closed against the old bundle");
        assert!(
            err.contains("UntrustedSigner") || err.contains("not signed"),
            "unexpected error: {err}"
        );
        // No re-fetch happened before the interval expired.
        assert_eq!(server.requests_served(), fetches_after_first);

        // After the interval expires, B re-fetches and the rotated SVID
        // verifies.
        tokio::time::sleep(refresh + Duration::from_millis(100)).await;
        attest_federated_session(
            &state,
            "peer-a.example/fed-session-1",
            "fed-session-1",
            &svid_v2.chain_pem,
            None,
        )
        .await
        .expect("rotated SVID must verify after the refreshed bundle");
        assert!(server.requests_served() > fetches_after_first);

        // Verification FOLLOWS the refreshed bundle: the old-CA SVID no
        // longer verifies (the v2 bundle does not retain the v1 key).
        let err = attest_federated_session(
            &state,
            "peer-a.example/fed-session-1",
            "fed-session-1",
            &svid_v1.chain_pem,
            None,
        )
        .await
        .expect_err("old-CA SVID must stop verifying after refresh");
        assert!(
            err.contains("UntrustedSigner") || err.contains("not signed"),
            "unexpected error: {err}"
        );
    }

    /// AC3 fail-closed side: once the refresh interval has expired, a
    /// refresh that fails (endpoint erroring) fails verification closed —
    /// B never silently serves a stale bundle past its interval.
    #[tokio::test]
    async fn expired_refresh_failure_fails_closed() {
        let ca = TrustDomainCa::generate(PEER_TD).expect("CA generation");
        let svid = ca
            .issue_svid(
                &SpiffeId::agent(PEER_TD, "worker", "fed-session-1").expect("id"),
                gyre_spiffe::BASE_SVID_TTL_SECS,
            )
            .expect("SVID issuance");
        let healthy = Arc::new(std::sync::Mutex::new(true));
        let bundle = peer_ca_bundle(&ca);
        let healthy_for_server = Arc::clone(&healthy);
        let server = TestHttpServer::spawn(move |_m, _p| {
            if *healthy_for_server.lock().unwrap() {
                (200, bundle.clone())
            } else {
                (500, "{\"error\": \"peer is down\"}".to_string())
            }
        });
        let refresh = Duration::from_millis(200);
        let state = state_with_peer(&server.origin, refresh);

        attest_federated_session(
            &state,
            "peer-a.example/fed-session-1",
            "fed-session-1",
            &svid.chain_pem,
            None,
        )
        .await
        .expect("healthy peer must verify");

        // The peer goes down; the interval expires.
        *healthy.lock().unwrap() = false;
        tokio::time::sleep(refresh + Duration::from_millis(100)).await;
        let err = attest_federated_session(
            &state,
            "peer-a.example/fed-session-1",
            "fed-session-1",
            &svid.chain_pem,
            None,
        )
        .await
        .expect_err("expired refresh must fail closed, not serve stale");
        assert!(
            err.contains("bundle endpoint") || err.contains("fetch"),
            "unexpected error: {err}"
        );
    }

    /// The attestation record is upserted on success and flipped to
    /// alive=false when a previously-attested federated agent later fails.
    #[tokio::test]
    async fn attestation_record_lifecycle() {
        let ca = TrustDomainCa::generate(PEER_TD).expect("CA generation");
        let bundle = peer_ca_bundle(&ca);
        let svid = ca
            .issue_svid(
                &SpiffeId::agent(PEER_TD, "worker", "fed-session-1").expect("id"),
                gyre_spiffe::BASE_SVID_TTL_SECS,
            )
            .expect("SVID issuance");
        let server = TestHttpServer::spawn(move |_m, _p| (200, bundle.clone()));
        let state = state_with_peer(&server.origin, Duration::from_secs(300));

        // Success creates the record, alive, with the wl_* metadata from
        // the caller's claims.
        let claims = serde_json::json!({
            "sub": "fed-session-1",
            "wl_stack_hash": "sha256:abc",
            "wl_compute_target": "docker:abc123",
        });
        attest_federated_session(
            &state,
            "peer-a.example/fed-session-1",
            "fed-session-1",
            &svid.chain_pem,
            Some(&claims),
        )
        .await
        .expect("verification must succeed");
        let json = state
            .kv_store
            .kv_get("workload_attestations", "peer-a.example/fed-session-1")
            .await
            .expect("kv read")
            .expect("record must exist after a successful presentation");
        let record: WorkloadAttestation = serde_json::from_str(&json).expect("record parses");
        assert!(record.alive);
        assert_eq!(
            record.spiffe_id.as_deref(),
            Some("spiffe://gyre-a.example.com/agent/worker/session/fed-session-1")
        );
        assert_eq!(record.stack_fingerprint, "sha256:abc");
        assert_eq!(record.compute_target, "docker:abc123");
        assert!(record.svid_serial.is_some());

        // A later failed presentation (expired SVID — validity window in
        // the past) flips the record to NOT attested.
        let not_before = time::OffsetDateTime::from_unix_timestamp(1_000_000_000).unwrap();
        let not_after = time::OffsetDateTime::from_unix_timestamp(1_000_001_000).unwrap();
        let expired = ca
            .issue_svid_with_validity(
                &SpiffeId::agent(PEER_TD, "worker", "fed-session-1").expect("id"),
                not_before,
                not_after,
            )
            .expect("expired SVID issuance");
        attest_federated_session(
            &state,
            "peer-a.example/fed-session-1",
            "fed-session-1",
            &expired.chain_pem,
            Some(&claims),
        )
        .await
        .expect_err("expired SVID must fail");
        let json = state
            .kv_store
            .kv_get("workload_attestations", "peer-a.example/fed-session-1")
            .await
            .expect("kv read")
            .expect("record must still exist");
        let record: WorkloadAttestation = serde_json::from_str(&json).expect("record parses");
        assert!(
            !record.alive,
            "failed re-verification must flip alive to false"
        );
        assert!(record.svid_verified_at.is_none());
    }

    /// No SVID presented → rejected (fail closed), no record created.
    #[tokio::test]
    async fn no_svid_rejected() {
        let state = plain_state();
        let err = attest_federated_session(&state, "peer-a.example/x", "x", "  ", None)
            .await
            .expect_err("empty presentation must be rejected");
        assert!(err.contains("no SVID presented"));
    }
}
