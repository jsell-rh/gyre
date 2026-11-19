//! Rekor transparency-log client adapters (agent-gates.md §Part 3
//! Storage: "in Rekor transparency log for non-repudiation"; task-264).

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use gyre_domain::merge_attestation::MergeAttestationBundle;
use gyre_ports::rekor::RekorClient;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::time::Duration;

/// Latency bound for every outbound Rekor call. reqwest has no default
/// total timeout; without this bound a hung Rekor would stall merge
/// completion indefinitely.
const REKOR_HTTP_TIMEOUT_SECS: u64 = 10;

/// The real Rekor transparency-log client (task-264 — replaces the
/// NoopRekorClient stub wired until now).
///
/// Submits each merge attestation bundle as a `hashedrekord` entry to the
/// configured Rekor instance (`POST {rekor}/api/v1/log/entries`, the same
/// wire format the commit-signing stack in gyre-server's `sigstore.rs`
/// uses and verified against upstream there):
///
/// ```json
/// {
///   "kind": "hashedrekord", "apiVersion": "0.0.1",
///   "spec": {
///     "data":   { "hash": { "algorithm": "sha256", "value": <sha256 of the bundle's canonical JSON> } },
///     "signature": { "content": <the bundle's forge Ed25519 signature>,
///                    "publicKey": { "content": <base64 PEM of the forge public key> } }
///   }
/// }
/// ```
///
/// The entry therefore binds the bundle's digest, its existing
/// multi-party-signed `bundle_signature`, and the forge public key — the
/// same signature every verifier of the bundle already checks.
///
/// Failure semantics (task-264): an unconfigured log (`rekor_url: None`)
/// and any HTTP/transport error are EXPLICIT `Err`s — never a silent
/// `Ok(None)`. The merge has already landed by the time bundles are
/// submitted, so the caller surfaces the error as a warning/audit state
/// rather than rolling the merge back, but a Rekor outage is never
/// reported as successful transparency-log recording.
pub struct HttpRekorClient {
    rekor_url: Option<String>,
    forge_public_key_pem: String,
    client: reqwest::Client,
}

impl HttpRekorClient {
    /// Build a client for `rekor_url` (the value of `GYRE_REKOR_URL`,
    /// already trimmed; `None` leaves the log unconfigured). The
    /// `forge_public_key_pem` is the forge Ed25519 public half as a PEM
    /// SPKI block — recorded in every entry so the bundled signature is
    /// independently verifiable against the transparency log.
    pub fn new(rekor_url: Option<String>, forge_public_key_pem: String) -> Self {
        Self {
            rekor_url: rekor_url
                .map(|u| u.trim_end_matches('/').to_string())
                .filter(|u| !u.is_empty()),
            forge_public_key_pem,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(REKOR_HTTP_TIMEOUT_SECS))
                .build()
                .expect("reqwest client with a timeout always builds"),
        }
    }
}

#[async_trait]
impl RekorClient for HttpRekorClient {
    async fn submit(&self, bundle: &MergeAttestationBundle) -> Result<Option<String>> {
        let rekor_url = self.rekor_url.as_deref().ok_or_else(|| {
            anyhow!(
                "Rekor transparency log not configured (GYRE_REKOR_URL unset) — merge \
                 attestation bundle for MR {} not recorded for non-repudiation",
                bundle.mr_id
            )
        })?;
        let signature = bundle.bundle_signature.as_deref().ok_or_else(|| {
            anyhow!(
                "merge attestation bundle for MR {} carries no bundle signature — \
                 nothing to record in the transparency log",
                bundle.mr_id
            )
        })?;
        let payload = bundle.unsigned_canonical_json();
        let digest = hex::encode(Sha256::digest(payload.as_bytes()));
        let entry = json!({
            "kind": "hashedrekord",
            "apiVersion": "0.0.1",
            "spec": {
                "data": {
                    "hash": { "algorithm": "sha256", "value": digest }
                },
                "signature": {
                    "content": signature,
                    "publicKey": { "content": BASE64.encode(self.forge_public_key_pem.as_bytes()) },
                },
            },
        });
        let resp = self
            .client
            .post(format!("{rekor_url}/api/v1/log/entries"))
            .header("Content-Type", "application/json")
            .body(serde_json::to_string(&entry)?)
            .send()
            .await
            .context("rekor entry POST failed")?;
        let status = resp.status();
        let text = resp.text().await.context("rekor response body")?;
        if !status.is_success() {
            return Err(anyhow!(
                "rekor entry POST for MR {} returned {status}: {text}",
                bundle.mr_id
            ));
        }
        let parsed: serde_json::Value =
            serde_json::from_str(&text).context("rekor entry response invalid")?;
        let uuid = parsed
            .as_object()
            .and_then(|m| m.keys().next().cloned())
            .ok_or_else(|| anyhow!("rekor returned no entry UUID"))?;
        Ok(Some(uuid))
    }

    fn is_configured(&self) -> bool {
        self.rekor_url.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    fn bundle(signature: Option<&str>) -> MergeAttestationBundle {
        MergeAttestationBundle {
            merge_attestation_version: "1.0".to_string(),
            mr_id: "mr-1".to_string(),
            merge_commit_sha: "abc".to_string(),
            merged_at: 0,
            spec_ref: None,
            spec_approval: None,
            author: None,
            gates: vec![],
            bundle_signature: signature.map(str::to_string),
        }
    }

    /// An unconfigured client is explicit about it: `is_configured` is
    /// false and every submission fails with an error naming the missing
    /// configuration — never a silent Ok(None) (task-264).
    #[tokio::test]
    async fn unconfigured_client_fails_explicitly() {
        let client = HttpRekorClient::new(None, "-----BEGIN PUBLIC KEY-----".to_string());
        assert!(!client.is_configured());
        let err = client.submit(&bundle(Some("sig"))).await.unwrap_err();
        assert!(err.to_string().contains("not configured"), "got: {err:#}");
    }

    /// A bundle without a signature cannot be recorded — explicit error,
    /// never an entry over unsigned material.
    #[tokio::test]
    async fn unsigned_bundle_is_refused() {
        let client =
            HttpRekorClient::new(Some("http://rekor.invalid".to_string()), "pk".to_string());
        let err = client.submit(&bundle(None)).await.unwrap_err();
        assert!(
            err.to_string().contains("no bundle signature"),
            "got: {err:#}"
        );
    }

    /// The real wire path against a local HTTP server speaking the Rekor
    /// contract: a successful POST returns the entry UUID from the
    /// response's UUID→entry map, and the request body is the hashedrekord
    /// carrying the bundle's digest, signature, and forge public key.
    /// A 500 from Rekor is an explicit error, never Ok(None).
    #[tokio::test]
    async fn submit_round_trips_hashedrekord_and_surfaces_outage() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc as StdArc;

        let seen_body: StdArc<parking_lot::Mutex<String>> = StdArc::new(Default::default());
        let fail = StdArc::new(AtomicBool::new(false));
        let seen_body_clone = StdArc::clone(&seen_body);
        let fail_clone = StdArc::clone(&fail);

        // Minimal HTTP/1.1 server speaking the Rekor entries contract.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = match listener.accept().await {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let seen = StdArc::clone(&seen_body_clone);
                let fail_now = fail_clone.load(Ordering::SeqCst);
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 65536];
                    let mut req = String::new();
                    loop {
                        let n = socket.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        req.push_str(&String::from_utf8_lossy(&buf[..n]));
                        // Crude but sufficient: the body is the last chunk.
                        if req.contains("\"kind\":\"hashedrekord\"")
                            || req.contains("\"kind\": \"hashedrekord\"")
                        {
                            break;
                        }
                    }
                    *seen.lock() = req;
                    let (status, body) = if fail_now {
                        ("500 Internal Server Error", "{}".to_string())
                    } else {
                        (
                            "201 Created",
                            "{\"69da70f2a4e8c5c1e1a3f0e1b7a9e6c3b9a7f4b2\": \
                              {\"body\": \"ZXhhbXBsZQ==\"}}"
                                .to_string(),
                        )
                    };
                    let resp = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });

        let client = HttpRekorClient::new(
            Some(format!("http://{addr}")),
            "-----BEGIN PUBLIC KEY-----".to_string(),
        );
        assert!(client.is_configured());

        // Happy path: entry UUID from the response map.
        let b = bundle(Some("c2ln"));
        let uuid = client
            .submit(&b)
            .await
            .expect("submission against the mock Rekor must succeed")
            .expect("a configured successful submission returns an entry UUID");
        assert_eq!(uuid, "69da70f2a4e8c5c1e1a3f0e1b7a9e6c3b9a7f4b2");

        // The request was the hashedrekord over the bundle's canonical
        // JSON: digest + signature + forge public key all present.
        let req = seen_body.lock().clone();
        let expected_digest = hex::encode(Sha256::digest(b.unsigned_canonical_json().as_bytes()));
        assert!(req.contains("/api/v1/log/entries"), "path: {req}");
        assert!(req.contains(&expected_digest), "digest missing: {req}");
        assert!(req.contains("c2ln"), "bundle signature missing: {req}");
        assert!(
            req.contains(&BASE64.encode(b"-----BEGIN PUBLIC KEY-----")),
            "forge public key missing: {req}"
        );

        // Outage: HTTP 500 is an explicit error naming the MR — never a
        // silent Ok(None) (task-264).
        fail.store(true, Ordering::SeqCst);
        let err = client.submit(&b).await.unwrap_err();
        assert!(
            err.to_string().contains("mr-1"),
            "the outage error must name the bundle: {err:#}"
        );
        assert!(err.to_string().contains("500"));

        server.abort();
    }
}
