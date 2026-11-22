//! SPIFFE federation: trust-bundle exchange semantics (identity-security.md
//! §Federation — "SPIFFE federation handles the workload attestation layer
//! independently").
//!
//! In the SPIFFE federation model, bundle endpoints are keyed by trust
//! domain: a verifying instance holds an EXCHANGED bundle for each trust
//! domain it federates with, refreshed from that domain's public bundle
//! endpoint on an interval, and validates an SVID from that domain against
//! that domain's exchanged bundle only. A trust domain whose bundle was
//! never exchanged is rejected — the presence of one federation does not
//! open the door to every domain.
//!
//! This module holds the protocol semantics:
//! - [`ExchangedBundles`] — the registry of federated trust domains and
//!   their fetched bundles (keyed by trust domain, refreshed on interval,
//!   fail-closed on refresh failure: verification follows the REFRESHED
//!   bundle, never a silently-stale one);
//! - [`BundleSource`] — the host server's fetch transport over the peers'
//!   public bundle endpoints (mocked in tests);
//! - [`verify_federated_svid`] — the federated verification decision:
//!   extract the SVID's claimed trust domain, resolve that domain's
//!   exchanged bundle, and run the full chain verification binding the
//!   SPIFFE ID to the caller's remote session (`/agent/{role}/session/{jwt
//!   sub}`).
//!
//! The OIDC-issuer federation layer (agent JWTs, JWKS discovery) is a
//! separate, independent layer: neither substitutes for the other. The
//! caller's JWT `sub` binds WHICH session the SVID must name; the SVID's
//! cryptographic validity is anchored solely in the exchanged bundle.

use crate::verify::{extract_spiffe_id, verify_svid, ExpectedIdentity, VerifiedSvid};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::RwLock;

/// A source of exchanged trust-bundle anchors: the host server's fetch
/// transport over its federation peers' public bundle endpoints. All key
/// material enters the verification through here — federation
/// configuration carries endpoints only, never credentials.
// async_trait generates a must_use future whose output already returns
// Result (double_must_use) — not actionable at the source.
#[allow(clippy::double_must_use)]
#[async_trait::async_trait]
pub trait BundleSource: Send + Sync {
    /// Fetch the CURRENT anchors (DER CA certificates) for a federated
    /// trust domain's exchanged bundle. An error fails verification
    /// closed — never an empty anchor set, never a fallback to another
    /// domain's bundle.
    async fn fetch(&self, trust_domain: &str) -> Result<Vec<Vec<u8>>, String>;
}

/// A cached exchanged bundle: the domain's validated anchors and when
/// they were fetched.
#[derive(Debug, Clone)]
struct CachedBundle {
    anchors: Vec<Vec<u8>>,
    fetched_at: Instant,
}

/// The exchanged-bundle registry: the federated trust domains, their
/// fetched bundles (keyed by trust domain), and the refresh interval
/// after which a bundle is re-fetched from its endpoint.
pub struct ExchangedBundles {
    /// The trust domains this instance exchanges bundles with (its
    /// federation peers). Domains not in this set are not federated.
    domains: Vec<String>,
    refresh_interval: Duration,
    source: Arc<dyn BundleSource>,
    cache: RwLock<HashMap<String, CachedBundle>>,
}

impl ExchangedBundles {
    /// Build the registry for a set of federated trust domains.
    pub fn new(
        domains: Vec<String>,
        refresh_interval: Duration,
        source: Arc<dyn BundleSource>,
    ) -> Self {
        Self {
            domains,
            refresh_interval,
            source,
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// The federated trust domains.
    pub fn domains(&self) -> &[String] {
        &self.domains
    }

    /// Whether a trust domain has an exchanged bundle.
    pub fn is_federated(&self, trust_domain: &str) -> bool {
        self.domains.iter().any(|d| d == trust_domain)
    }

    /// The exchanged verification anchors (DER CA certificates) for a
    /// trust domain, refreshing from the source when the cached copy is
    /// older than the refresh interval.
    ///
    /// Fail-closed:
    /// - a trust domain with no exchanged bundle is an error (never an
    ///   empty anchor set, never another domain's bundle);
    /// - a refresh that fails once the interval has expired is an error —
    ///   verification follows the refreshed bundle, not a silently-stale
    ///   one.
    pub async fn anchors_for(&self, trust_domain: &str) -> Result<Vec<Vec<u8>>, String> {
        if !self.is_federated(trust_domain) {
            return Err(format!(
                "trust domain {trust_domain:?} is not federated: no SPIFFE trust bundle \
                 has been exchanged with this instance"
            ));
        }
        // Fresh cache hit under the read lock.
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(trust_domain) {
                if entry.fetched_at.elapsed() < self.refresh_interval {
                    return Ok(entry.anchors.clone());
                }
            }
        }
        let mut guard = self.cache.write().await;
        // Re-check under the write lock (a concurrent caller may have
        // refreshed while we waited).
        if let Some(entry) = guard.get(trust_domain) {
            if entry.fetched_at.elapsed() < self.refresh_interval {
                return Ok(entry.anchors.clone());
            }
        }
        let anchors = self.source.fetch(trust_domain).await?;
        guard.insert(
            trust_domain.to_string(),
            CachedBundle {
                anchors: anchors.clone(),
                fetched_at: Instant::now(),
            },
        );
        Ok(anchors)
    }
}

/// The federated workload-identity verification decision
/// (identity-security.md Layer 1, federation).
///
/// `caller_sub` is the remote session id from the caller's (already
/// Layer-2-verified) JWT — used ONLY to bind the SVID to the presenter's
/// own session, never as a trust anchor.
///
/// Fail-closed on every check:
/// 1. the SVID must parse and carry exactly one URI SAN with a
///    well-formed agent-workload SPIFFE ID;
/// 2. the SPIFFE ID's trust domain must have an EXCHANGED bundle in
///    `bundles` (a never-exchanged domain is rejected even when the
///    caller's OIDC JWT from that instance's issuer is trusted — the
///    layers are independent);
/// 3. the leaf must be cryptographically signed by a key in that
///    domain's exchanged bundle, within its validity window;
/// 4. the SPIFFE ID must name the caller's own remote session.
pub async fn verify_federated_svid(
    presented_pem: &str,
    caller_sub: &str,
    bundles: &ExchangedBundles,
) -> Result<VerifiedSvid, String> {
    if presented_pem.trim().is_empty() {
        return Err("no SVID presented".to_string());
    }

    // 1. Extract the (unverified) SPIFFE ID to select the exchanged
    //    bundle by trust domain. This is a claim from the certificate,
    //    not a trust decision.
    let claimed_id = extract_spiffe_id(presented_pem)
        .map_err(|e| format!("presented SVID is not a parseable agent SVID: {e}"))?;
    let agent_path = claimed_id.agent_path().ok_or_else(|| {
        format!(
            "federated SVID {claimed_id} is not in the agent workload form \
             /agent/{{role}}/session/{{id}}"
        )
    })?;

    // 2. The claimed trust domain must have an exchanged bundle.
    let anchors = bundles.anchors_for(&claimed_id.trust_domain).await?;

    // 3.+4. Full cryptographic verification against that domain's
    //      exchanged bundle. The expected trust domain and role come from
    //      the claimed ID (the peer's own spawn-time choices — the
    //      verifying instance has no independent knowledge of them); the
    //      session binding is the caller's `sub`.
    let expected = ExpectedIdentity {
        trust_domain: claimed_id.trust_domain.clone(),
        role: agent_path.role.clone(),
        session_id: caller_sub.to_string(),
    };
    let verified = verify_svid(presented_pem, &anchors, &expected, SystemTime::now())
        .map_err(|e| format!("federated SVID rejected: {e}"))?;
    Ok(verified)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ca::TrustDomainCa;
    use crate::id::SpiffeId;
    use crate::verify::VerificationError;
    use std::collections::HashSet;

    const PEER_TD: &str = "gyre-a.example.com";

    /// A mock bundle source: per-domain anchor sets that tests can rotate
    /// under the registry's feet, with failure injection and call
    /// counting.
    struct MockSource {
        bundles: std::sync::Mutex<HashMap<String, Vec<Vec<u8>>>>,
        failing: std::sync::Mutex<HashSet<String>>,
        calls: std::sync::atomic::AtomicUsize,
    }

    impl MockSource {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                bundles: std::sync::Mutex::new(HashMap::new()),
                failing: std::sync::Mutex::new(HashSet::new()),
                calls: std::sync::atomic::AtomicUsize::new(0),
            })
        }

        fn serve(&self, td: &str, ca: &TrustDomainCa) {
            self.bundles
                .lock()
                .unwrap()
                .insert(td.to_string(), vec![ca.cert_der().to_vec()]);
        }

        fn drop_domain(&self, td: &str) {
            self.bundles.lock().unwrap().remove(td);
        }

        fn fail_domain(&self, td: &str) {
            self.failing.lock().unwrap().insert(td.to_string());
        }

        fn calls(&self) -> usize {
            self.calls.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl BundleSource for MockSource {
        async fn fetch(&self, trust_domain: &str) -> Result<Vec<Vec<u8>>, String> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.failing.lock().unwrap().contains(trust_domain) {
                return Err(format!("bundle endpoint for {trust_domain} is down").to_string());
            }
            self.bundles
                .lock()
                .unwrap()
                .get(trust_domain)
                .cloned()
                .ok_or_else(|| format!("no bundle served for {trust_domain}"))
        }
    }

    fn registry_for(
        source: &Arc<MockSource>,
        domains: &[&str],
        refresh: Duration,
    ) -> ExchangedBundles {
        ExchangedBundles::new(
            domains.iter().map(|d| d.to_string()).collect(),
            refresh,
            Arc::clone(source) as Arc<dyn BundleSource>,
        )
    }

    fn peer_svid(ca: &TrustDomainCa, session: &str) -> crate::ca::IssuedSvid {
        ca.issue_svid(
            &SpiffeId::agent(PEER_TD, "worker", session).expect("peer SPIFFE ID"),
            crate::BASE_SVID_TTL_SECS,
        )
        .expect("peer SVID issuance")
    }

    /// The two-configuration probe (the task's verification intent) at the
    /// protocol level: the SAME A-trust-domain SVID is rejected without
    /// the bundle exchange and verifies with it.
    #[tokio::test]
    async fn federated_svid_verifies_only_with_exchanged_bundle() {
        let ca = TrustDomainCa::generate(PEER_TD).expect("peer CA");
        let svid = peer_svid(&ca, "fed-session-1");
        let source = MockSource::new();
        source.serve(PEER_TD, &ca);

        // (a) no exchanged bundle: rejected, and rejected as NOT
        //     FEDERATED (not as a crypto failure — the layer is absent).
        let unbundled = registry_for(&source, &[], Duration::from_secs(300));
        let err = verify_federated_svid(&svid.chain_pem, "fed-session-1", &unbundled)
            .await
            .expect_err("unfederated trust domain must be rejected");
        assert!(err.contains("not federated"), "unexpected error: {err}");

        // (b) exchanged bundle: verifies, extracting the same SPIFFE ID
        //     A's own verifiers would.
        let bundled = registry_for(&source, &[PEER_TD], Duration::from_secs(300));
        let verified = verify_federated_svid(&svid.chain_pem, "fed-session-1", &bundled)
            .await
            .expect("exchanged-bundle SVID must verify");
        assert_eq!(
            verified.spiffe_id.to_string(),
            "spiffe://gyre-a.example.com/agent/worker/session/fed-session-1"
        );
        assert_eq!(verified.agent_path.session_id, "fed-session-1");

        // Cross-check against A's own verification: same anchors, same
        // extracted identity and serial.
        let a_side = verify_svid(
            &svid.chain_pem,
            &[ca.cert_der().to_vec()],
            &ExpectedIdentity {
                trust_domain: PEER_TD.to_string(),
                role: "worker".to_string(),
                session_id: "fed-session-1".to_string(),
            },
            SystemTime::now(),
        )
        .expect("A-side verification");
        assert_eq!(verified.spiffe_id, a_side.spiffe_id);
        assert_eq!(verified.serial, a_side.serial);
    }

    /// A never-exchanged trust domain is rejected even when a different
    /// peer's bundle IS exchanged, and even for a genuine SVID from that
    /// domain's own CA.
    #[tokio::test]
    async fn never_exchanged_trust_domain_rejected_alongside_federated_peer() {
        let ca_a = TrustDomainCa::generate(PEER_TD).expect("peer CA");
        let source = MockSource::new();
        source.serve(PEER_TD, &ca_a);
        let registry = registry_for(&source, &[PEER_TD], Duration::from_secs(300));

        let ca_c = TrustDomainCa::generate("gyre-c.example.org").expect("third CA");
        let svid = ca_c
            .issue_svid(
                &SpiffeId::agent("gyre-c.example.org", "worker", "fed-session-1")
                    .expect("third SPIFFE ID"),
                crate::BASE_SVID_TTL_SECS,
            )
            .expect("third SVID issuance");
        let err = verify_federated_svid(&svid.chain_pem, "fed-session-1", &registry)
            .await
            .expect_err("never-exchanged trust domain must be rejected");
        assert!(err.contains("not federated"), "unexpected error: {err}");
    }

    /// A rogue SVID claiming a federated trust domain but signed by a key
    /// NOT in the exchanged bundle is rejected by the chain validation.
    #[tokio::test]
    async fn rogue_signer_for_federated_domain_rejected() {
        let honest = TrustDomainCa::generate(PEER_TD).expect("honest CA");
        let source = MockSource::new();
        source.serve(PEER_TD, &honest);
        let registry = registry_for(&source, &[PEER_TD], Duration::from_secs(300));

        let rogue = TrustDomainCa::generate(PEER_TD).expect("rogue CA");
        let svid = peer_svid(&rogue, "fed-session-1");
        let err = verify_federated_svid(&svid.chain_pem, "fed-session-1", &registry)
            .await
            .expect_err("rogue signer must be rejected");
        assert!(
            err.contains(&VerificationError::UntrustedSigner.to_string()),
            "unexpected error: {err}"
        );
    }

    /// Session binding: a federated agent cannot present another remote
    /// session's SVID.
    #[tokio::test]
    async fn other_sessions_svid_rejected() {
        let ca = TrustDomainCa::generate(PEER_TD).expect("peer CA");
        let svid = peer_svid(&ca, "someone-elses-session");
        let source = MockSource::new();
        source.serve(PEER_TD, &ca);
        let registry = registry_for(&source, &[PEER_TD], Duration::from_secs(300));
        let err = verify_federated_svid(&svid.chain_pem, "fed-session-1", &registry)
            .await
            .expect_err("another session's SVID must be rejected");
        assert!(err.contains("session"), "unexpected error: {err}");
    }

    /// No SVID presented → rejected.
    #[tokio::test]
    async fn no_svid_rejected() {
        let source = MockSource::new();
        let registry = registry_for(&source, &[PEER_TD], Duration::from_secs(300));
        let err = verify_federated_svid("  ", "x", &registry)
            .await
            .expect_err("empty presentation must be rejected");
        assert!(err.contains("no SVID presented"));
    }

    /// Bundle rotation: the cached (old) anchors are served until the
    /// refresh interval expires — a rotated SVID fails CLOSED against the
    /// old bundle — then the registry re-fetches and verification follows
    /// the refreshed bundle (the rotated SVID verifies, the old-CA SVID
    /// stops verifying).
    #[tokio::test]
    async fn bundle_rotation_follows_refreshed_bundle() {
        let ca_v1 = TrustDomainCa::generate(PEER_TD).expect("CA v1");
        let ca_v2 = TrustDomainCa::generate(PEER_TD).expect("CA v2");
        let svid_v1 = peer_svid(&ca_v1, "fed-session-1");
        let svid_v2 = peer_svid(&ca_v2, "fed-session-1");
        let source = MockSource::new();
        source.serve(PEER_TD, &ca_v1);
        let refresh = Duration::from_millis(200);
        let registry = registry_for(&source, &[PEER_TD], refresh);

        // Pre-rotation: the v1 SVID verifies (bundle fetched + cached).
        verify_federated_svid(&svid_v1.chain_pem, "fed-session-1", &registry)
            .await
            .expect("v1 SVID must verify against the exchanged v1 bundle");
        let fetches_after_first = source.calls();
        assert!(fetches_after_first >= 1);

        // Rotate: the peer now serves the v2 bundle.
        source.serve(PEER_TD, &ca_v2);

        // While the cache is fresh (within the interval), the rotated SVID
        // fails closed against the old-bundle-only cache — and no
        // re-fetch happened.
        let err = verify_federated_svid(&svid_v2.chain_pem, "fed-session-1", &registry)
            .await
            .expect_err("rotated SVID must fail closed against the old bundle");
        assert!(
            err.contains(&VerificationError::UntrustedSigner.to_string()),
            "unexpected error: {err}"
        );
        assert_eq!(source.calls(), fetches_after_first);

        // After the interval expires, the registry re-fetches and the
        // rotated SVID verifies.
        tokio::time::sleep(refresh + Duration::from_millis(50)).await;
        verify_federated_svid(&svid_v2.chain_pem, "fed-session-1", &registry)
            .await
            .expect("rotated SVID must verify after the refreshed bundle");
        assert!(source.calls() > fetches_after_first);

        // Verification FOLLOWS the refreshed bundle: the old-CA SVID no
        // longer verifies (the v2 bundle does not retain the v1 key).
        let err = verify_federated_svid(&svid_v1.chain_pem, "fed-session-1", &registry)
            .await
            .expect_err("old-CA SVID must stop verifying after refresh");
        assert!(
            err.contains(&VerificationError::UntrustedSigner.to_string()),
            "unexpected error: {err}"
        );
    }

    /// Once the refresh interval has expired, a refresh that fails (peer
    /// endpoint down) fails verification closed — the registry never
    /// silently serves a stale bundle past its interval.
    #[tokio::test]
    async fn expired_refresh_failure_fails_closed() {
        let ca = TrustDomainCa::generate(PEER_TD).expect("CA");
        let svid = peer_svid(&ca, "fed-session-1");
        let source = MockSource::new();
        source.serve(PEER_TD, &ca);
        let refresh = Duration::from_millis(150);
        let registry = registry_for(&source, &[PEER_TD], refresh);

        verify_federated_svid(&svid.chain_pem, "fed-session-1", &registry)
            .await
            .expect("healthy peer must verify");

        // The peer goes down; the interval expires.
        source.fail_domain(PEER_TD);
        tokio::time::sleep(refresh + Duration::from_millis(50)).await;
        let err = verify_federated_svid(&svid.chain_pem, "fed-session-1", &registry)
            .await
            .expect_err("expired refresh must fail closed, not serve stale");
        assert!(err.contains("down"), "unexpected error: {err}");
    }

    /// A domain whose endpoint stops serving a bundle (deregistered
    /// upstream) fails closed after the refresh interval rather than
    /// keeping the old anchors forever.
    #[tokio::test]
    async fn withdrawn_bundle_fails_closed_after_interval() {
        let ca = TrustDomainCa::generate(PEER_TD).expect("CA");
        let svid = peer_svid(&ca, "fed-session-1");
        let source = MockSource::new();
        source.serve(PEER_TD, &ca);
        let refresh = Duration::from_millis(150);
        let registry = registry_for(&source, &[PEER_TD], refresh);

        verify_federated_svid(&svid.chain_pem, "fed-session-1", &registry)
            .await
            .expect("served bundle must verify");

        source.drop_domain(PEER_TD);
        tokio::time::sleep(refresh + Duration::from_millis(50)).await;
        let err = verify_federated_svid(&svid.chain_pem, "fed-session-1", &registry)
            .await
            .expect_err("withdrawn bundle must fail closed after refresh");
        assert!(err.contains("no bundle served"), "unexpected error: {err}");
    }
}
