//! SVID-backed workload attestation records (identity-security.md Layer 1).
//!
//! A workload attestation record is the metadata view of an agent
//! session's Layer-1 identity: where it was launched (compute target,
//! hostname, PID/container metadata — informational) and the state of its
//! X.509-SVID verification (`spiffe_id`, `svid_serial`, `svid_verified_at`,
//! `alive`). The record is written at spawn (alongside SVID issuance in
//! `api::spawn`) and updated by the workload-identity verification surface
//! (`spiffe::attest_session`) — it is never a verification input. The
//! retired G10 local scheme (self-reported metadata "verified" by a
//! `/proc/{pid}` existence probe plus a skip-on-empty string compare) was
//! a custom auth scheme the identity-security.md "no custom auth schemes"
//! constraint forbids; Layer 1 now uses the specced SPIFFE protocol.
//!
//! JWT agent tokens embed the workload claims (`wl_pid`, `wl_hostname`,
//! `wl_compute_target`, `wl_stack_hash`) so that any verifier holding the
//! JWKS public key can reconstruct the workload identity without talking
//! to the Gyre server; the stack fingerprint claim is the same value the
//! SVID-backed attestation record carries (bound at issuance/attestation
//! by `spiffe::verify_workload_identity`).

use serde::{Deserialize, Serialize};

/// A point-in-time workload attestation record for an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadAttestation {
    /// Agent that was attested.
    pub agent_id: String,
    /// OS PID of the agent process on the local host, if known (metadata).
    pub pid: Option<u32>,
    /// Hostname where the agent is running (metadata).
    pub hostname: String,
    /// Compute target identifier (e.g. "local", docker container ID, SSH host).
    pub compute_target: String,
    /// Stack fingerprint bound to the SVID-backed attestation.
    pub stack_fingerprint: String,
    /// Unix epoch seconds when the attestation was created.
    pub attested_at: u64,
    /// Whether the most recent workload-identity (SVID) verification passed.
    pub alive: bool,
    /// Unix epoch seconds of the last workload-identity verification.
    pub last_verified_at: u64,
    /// Container ID when spawned via ContainerTarget (M19.4, metadata).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_id: Option<String>,
    /// Image digest (sha256) when spawned via ContainerTarget (M19.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_hash: Option<String>,
    /// SPIFFE ID of the session's X.509-SVID
    /// (`spiffe://{trust-domain}/agent/{role}/session/{id}`), once issued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spiffe_id: Option<String>,
    /// Leaf serial (hex) of the session's SVID, once verified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub svid_serial: Option<String>,
    /// Unix epoch seconds of the last successful SVID verification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub svid_verified_at: Option<u64>,
}

/// Create a new workload attestation for an agent process at spawn time,
/// carrying the SVID issued for the session.
pub fn attest_agent(
    agent_id: &str,
    pid: Option<u32>,
    compute_target: &str,
    stack_hash: &str,
) -> WorkloadAttestation {
    attest_agent_with_container(
        agent_id,
        pid,
        compute_target,
        stack_hash,
        None,
        None,
        None,
        None,
    )
}

/// Create a workload attestation with optional container metadata (M19.4)
/// and the session's SVID identity.
#[allow(clippy::too_many_arguments)]
pub fn attest_agent_with_container(
    agent_id: &str,
    pid: Option<u32>,
    compute_target: &str,
    stack_hash: &str,
    container_id: Option<String>,
    image_hash: Option<String>,
    spiffe_id: Option<String>,
    svid_serial: Option<String>,
) -> WorkloadAttestation {
    let now = now_secs();
    WorkloadAttestation {
        agent_id: agent_id.to_string(),
        pid,
        hostname: read_hostname(),
        compute_target: compute_target.to_string(),
        stack_fingerprint: stack_hash.to_string(),
        attested_at: now,
        // An unverified session is not alive: `alive` becomes true only
        // when the SVID verification surface passes (spiffe::attest_session).
        alive: false,
        last_verified_at: now,
        container_id,
        image_hash,
        spiffe_id,
        svid_serial,
        svid_verified_at: None,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Read the hostname without pulling in an extra crate.
///
/// Resolution order:
/// 1. `/proc/sys/kernel/hostname` (Linux kernel interface)
/// 2. `HOSTNAME` environment variable
/// 3. `"unknown"`
pub(crate) fn read_hostname() -> String {
    // Linux kernel interface — most reliable.
    if let Ok(h) = std::fs::read_to_string("/proc/sys/kernel/hostname") {
        let trimmed = h.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attest_creates_unverified_record_with_svid_identity() {
        let att = attest_agent_with_container(
            "agent-1",
            Some(1234),
            "local",
            "sha256:abc",
            None,
            None,
            Some("spiffe://gyre.example.com/agent/worker/session/agent-1".to_string()),
            Some("0a1b".to_string()),
        );
        assert_eq!(att.agent_id, "agent-1");
        assert_eq!(att.pid, Some(1234));
        assert_eq!(att.compute_target, "local");
        assert_eq!(att.stack_fingerprint, "sha256:abc");
        // Secure by default: a freshly created record is NOT verified.
        assert!(!att.alive);
        assert!(att.attested_at > 0);
        assert_eq!(
            att.spiffe_id.as_deref(),
            Some("spiffe://gyre.example.com/agent/worker/session/agent-1")
        );
        assert_eq!(att.svid_serial.as_deref(), Some("0a1b"));
        assert!(att.svid_verified_at.is_none());
    }
}
