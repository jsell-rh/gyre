//! SPIFFE Workload API client (gRPC over a Unix Domain Socket).
//!
//! When an external SPIRE server / Workload API is configured
//! (`GYRE_SPIFFE_WORKLOAD_API_SOCKET`), Gyre obtains the agent session's
//! X.509-SVIDs from it instead of issuing them from its own trust-domain
//! CA. This is the specced existing-protocol path for Layer 1.
//!
//! Wire protocol: the SPIFFE Workload API (`spiffe.workload.v1.
//! SpiffeWorkloadAPI/FetchX509SVID`, bidirectional streaming over UDS,
//! header `workload.spiffe.io` required). The response's `X509SVID`
//! messages carry (per the Workload API specification): `spiffe_id`,
//! `x509_svid` (DER certificate chain, leaf first), `x509_svid_key`
//! (PKCS#8), and `bundle` (the trust domain's SPIFFE bundle). The
//! protobuf field numbers below are the published ones; the tests
//! hand-encode wire-format bytes with the same numbers so the decoder is
//! pinned to the documented encoding, not to its own re-serialization.
//!
//! The trust anchor in external mode is the operator-configured trust
//! bundle (`GYRE_SPIFFE_TRUST_BUNDLE_URL`, served in SPIFFE bundle
//! format, e.g. the SPIRE server's federation bundle endpoint) — trust is
//! never derived from the workload's own response.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// A `spiffe.workload.v1.X509SVIDResponse` (first message of the
/// FetchX509SVID stream).
#[derive(Clone, PartialEq, prost::Message)]
pub struct X509SvidResponse {
    #[prost(message, repeated, tag = "1")]
    pub svids: Vec<X509Svid>,
}

/// A `spiffe.workload.v1.X509SVID`.
#[derive(Clone, PartialEq, prost::Message)]
pub struct X509Svid {
    /// The SPIFFE ID of the workload this SVID identifies.
    #[prost(string, tag = "1")]
    pub spiffe_id: String,
    /// DER-encoded X.509 chain, leaf first.
    #[prost(bytes = "vec", tag = "2")]
    pub x509_svid: Vec<u8>,
    /// PKCS#8-encoded private key.
    #[prost(bytes = "vec", tag = "3")]
    pub x509_svid_key: Vec<u8>,
    /// SPIFFE bundle of the trust domain (protobuf-marshaled).
    #[prost(bytes = "vec", tag = "4")]
    pub bundle: Vec<u8>,
}

/// A `spiffe.workload.v1.FetchX509SVIDRequest` (empty message).
#[derive(Clone, Copy, PartialEq, prost::Message)]
pub struct FetchX509SvidRequest {}

/// One SVID obtained from an external Workload API, in the shape Gyre's
/// issuance path consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadApiSvid {
    pub spiffe_id: String,
    /// PEM chain, leaf first.
    pub chain_pem: String,
    /// PKCS#8 PEM private key.
    pub private_key_pem: String,
}

/// The transport used to reach the Workload API. Production uses the
/// gRPC-over-UDS implementation; tests inject a fake to exercise response
/// handling without a SPIRE server.
// clippy: async-trait's expansion re-applies #[must_use] to a method that
// already returns Result (double_must_use) — not actionable at the source.
#[allow(clippy::double_must_use)]
#[async_trait::async_trait]
pub trait WorkloadApiTransport: Send + Sync {
    /// Send a FetchX509SVID request and return the first response message.
    async fn fetch_x509_svid(&self) -> Result<X509SvidResponse>;
}

/// gRPC Workload API client over a Unix Domain Socket.
pub struct GrpcWorkloadApi {
    socket_path: String,
    timeout: Duration,
}

impl GrpcWorkloadApi {
    pub fn new(socket_path: impl Into<String>, timeout: Duration) -> Self {
        Self {
            socket_path: socket_path.into(),
            timeout,
        }
    }

    /// The gRPC method path (`package.Service/Method`).
    fn method_path() -> &'static str {
        "/spiffe.workload.v1.SpiffeWorkloadAPI/FetchX509SVID"
    }

    /// Connect to the Workload API socket and return the tonic channel.
    /// Exposed to tests so the channel setup is exercised.
    async fn connect(&self) -> Result<tonic::transport::Channel> {
        use tower::service_fn;
        let socket_path = self.socket_path.clone();
        // Lazy: the connector runs per-request, so a Workload API that
        // appears after boot is usable without re-creating the service.
        let channel = tonic::transport::Endpoint::from_static("http://workload-api")
            .connect_with_connector_lazy(service_fn(move |_: tonic::transport::Uri| {
                let path = socket_path.clone();
                async move {
                    let stream = tokio::net::UnixStream::connect(path).await?;
                    Ok::<_, std::io::Error>(hyper_util::rt::TokioIo::new(stream))
                }
            }));
        Ok(channel)
    }
}

#[async_trait::async_trait]
impl WorkloadApiTransport for GrpcWorkloadApi {
    async fn fetch_x509_svid(&self) -> Result<X509SvidResponse> {
        // Latency-bounded: an unresponsive external service must fail the
        // issuance path, never stall it (no default timeout exists).
        let fut = async {
            let channel = self.connect().await?;
            let codec =
                tonic::codec::ProstCodec::<FetchX509SvidRequest, X509SvidResponse>::default();
            let mut grpc = tonic::client::Grpc::new(channel);
            // The Workload API requires the `workload.spiffe.io` header on
            // every call; unknown/absent metadata is rejected.
            let mut request = tonic::Request::new(tokio_stream::iter([FetchX509SvidRequest {}]));
            request.metadata_mut().insert(
                "workload.spiffe.io",
                "true".parse().expect("static header value parses"),
            );
            grpc.ready().await?;
            let path = Self::method_path()
                .parse::<tonic::codegen::http::uri::PathAndQuery>()
                .expect("static method path parses");
            let response = grpc
                .streaming::<_, FetchX509SvidRequest, X509SvidResponse, _>(request, path, codec)
                .await?;
            let mut stream = response.into_inner();
            let message = stream
                .message()
                .await?
                .context("Workload API closed the stream without a response")?;
            Ok::<X509SvidResponse, anyhow::Error>(message)
        };
        match tokio::time::timeout(self.timeout, fut).await {
            Ok(r) => r,
            Err(_) => Err(anyhow::anyhow!(
                "Workload API fetch timed out after {}s",
                self.timeout.as_secs()
            )),
        }
    }
}

/// Decode a Workload API response into the issuance path's SVID shape.
///
/// Verifies the DER chain parses and the SPIFFE ID is well-formed before
/// handing anything to the issuance path (fail closed on garbage).
pub fn decode_response(response: &X509SvidResponse) -> Result<Vec<WorkloadApiSvid>> {
    let mut out = Vec::new();
    for svid in &response.svids {
        crate::id::SpiffeId::parse(&svid.spiffe_id).with_context(|| {
            format!(
                "Workload API returned malformed SPIFFE ID {:?}",
                svid.spiffe_id
            )
        })?;
        // Split the DER chain into PEM blocks (leaf first).
        let mut pems = Vec::new();
        let mut rest: &[u8] = &svid.x509_svid;
        while !rest.is_empty() {
            let (remaining, _) = x509_parser::parse_x509_certificate(rest)
                .with_context(|| "Workload API returned an unparseable certificate chain")?;
            let consumed = rest.len() - remaining.len();
            let cert_der = &rest[..consumed];
            pems.push(pem::encode(&pem::Pem::new(
                "CERTIFICATE",
                cert_der.to_vec(),
            )));
            rest = remaining;
        }
        if pems.is_empty() {
            anyhow::bail!("Workload API returned an empty certificate chain");
        }
        let key_pem = pem::encode(&pem::Pem::new("PRIVATE KEY", svid.x509_svid_key.clone()));
        out.push(WorkloadApiSvid {
            spiffe_id: svid.spiffe_id.clone(),
            chain_pem: pems.join("\n"),
            private_key_pem: key_pem,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-encode an X509SVIDResponse on the wire with EXPLICIT field
    /// numbers, so the prost definitions above are pinned to the
    /// documented encoding rather than round-tripping through themselves.
    fn hand_encode_response(svids: &[(&str, &[u8], &[u8])]) -> Vec<u8> {
        fn varint(mut v: u64, out: &mut Vec<u8>) {
            loop {
                let mut b = (v & 0x7f) as u8;
                v >>= 7;
                if v != 0 {
                    b |= 0x80;
                }
                out.push(b);
                if v == 0 {
                    break;
                }
            }
        }
        fn tag(field: u32, wire: u32, out: &mut Vec<u8>) {
            varint(((field as u64) << 3) | wire as u64, out);
        }
        fn len_delimited(field: u32, bytes: &[u8], out: &mut Vec<u8>) {
            tag(field, 2, out);
            varint(bytes.len() as u64, out);
            out.extend_from_slice(bytes);
        }
        let mut body = Vec::new();
        for (id, der, key) in svids {
            let mut svid_msg = Vec::new();
            len_delimited(1, id.as_bytes(), &mut svid_msg); // spiffe_id = 1
            len_delimited(2, der, &mut svid_msg); // x509_svid = 2
            len_delimited(3, key, &mut svid_msg); // x509_svid_key = 3
            len_delimited(1, &svid_msg, &mut body); // svids = 1 (repeated)
        }
        body
    }

    struct FakeTransport(X509SvidResponse);

    #[async_trait::async_trait]
    impl WorkloadApiTransport for FakeTransport {
        async fn fetch_x509_svid(&self) -> Result<X509SvidResponse> {
            Ok(self.0.clone())
        }
    }

    #[tokio::test]
    async fn decodes_wire_encoded_response() {
        // One SVID: a real DER certificate from our CA (any parseable DER
        // proves chain splitting) and a fake key blob.
        let ca = crate::ca::TrustDomainCa::generate("example.org").unwrap();
        let id = crate::id::SpiffeId::agent("example.org", "worker", "s-1").unwrap();
        let svid = ca.issue_svid(&id, 60).unwrap();
        let leaf_der = pem::parse(&svid.leaf_pem).unwrap().contents().to_vec();
        let key_der = pem::parse(&svid.private_key_pem)
            .unwrap()
            .contents()
            .to_vec();

        let wire = hand_encode_response(&[(
            "spiffe://example.org/agent/worker/session/s-1",
            &leaf_der,
            &key_der,
        )]);
        let decoded: X509SvidResponse =
            prost::Message::decode(wire.as_slice()).expect("wire bytes decode");
        assert_eq!(decoded.svids.len(), 1);
        assert_eq!(
            decoded.svids[0].spiffe_id,
            "spiffe://example.org/agent/worker/session/s-1"
        );
        assert_eq!(decoded.svids[0].x509_svid, leaf_der);

        let transport = FakeTransport(decoded);
        let svids = decode_response(&transport.fetch_x509_svid().await.unwrap()).unwrap();
        assert_eq!(svids.len(), 1);
        assert_eq!(
            svids[0].spiffe_id,
            "spiffe://example.org/agent/worker/session/s-1"
        );
        assert!(svids[0].chain_pem.contains("BEGIN CERTIFICATE"));
        assert!(svids[0].private_key_pem.contains("BEGIN PRIVATE KEY"));
    }

    #[tokio::test]
    async fn decode_rejects_malformed_spiffe_id() {
        let response = X509SvidResponse {
            svids: vec![X509Svid {
                spiffe_id: "not-a-spiffe-id".to_string(),
                x509_svid: Vec::new(),
                x509_svid_key: Vec::new(),
                bundle: Vec::new(),
            }],
        };
        assert!(decode_response(&response).is_err());
    }

    #[tokio::test]
    async fn decode_rejects_empty_chain() {
        let response = X509SvidResponse {
            svids: vec![X509Svid {
                spiffe_id: "spiffe://example.org/agent/worker/session/s-1".to_string(),
                x509_svid: Vec::new(),
                x509_svid_key: Vec::new(),
                bundle: Vec::new(),
            }],
        };
        assert!(decode_response(&response).is_err());
    }

    #[tokio::test]
    async fn grpc_client_times_out_on_dead_socket() {
        // A socket path that exists but never accepts: the client must
        // fail within the timeout, not hang.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("never.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let _guard = listener; // bound but we never accept/answer
        let client = GrpcWorkloadApi::new(path.to_str().unwrap(), Duration::from_millis(500));
        let start = std::time::Instant::now();
        let result = client.fetch_x509_svid().await;
        assert!(result.is_err());
        assert!(start.elapsed() < Duration::from_secs(5));
    }
}
