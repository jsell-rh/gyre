//! G10: Local workload attestation — pragmatic SPIFFE alternative.
//!
//! Provides lightweight workload identity for agent processes without requiring
//! an external SPIFFE/SPIRE infrastructure.  On spawn, an attestation record is
//! created capturing the OS PID, hostname, compute target, and stack fingerprint.
//! On heartbeat the PID is probed to confirm the process is still alive.
//!
//! JWT agent tokens embed the workload claims (`wl_pid`, `wl_hostname`,
//! `wl_compute_target`, `wl_stack_hash`) so that any verifier holding the JWKS
//! public key can reconstruct and check the workload identity without talking to
//! the Gyre server.

use serde::{Deserialize, Serialize};

/// A point-in-time workload attestation record for an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadAttestation {
    /// Agent that was attested.
    pub agent_id: String,
    /// OS PID of the agent process on the local host, if known.
    pub pid: Option<u32>,
    /// Hostname where the agent is running.
    pub hostname: String,
    /// Compute target identifier (e.g. "local", docker container ID, SSH host).
    pub compute_target: String,
    /// Stack fingerprint hash at attestation time.
    pub stack_fingerprint: String,
    /// Unix epoch seconds when the attestation was created.
    pub attested_at: u64,
    /// Whether the most recent liveness verification passed.
    pub alive: bool,
    /// Unix epoch seconds of the last liveness verification.
    pub last_verified_at: u64,
    /// Container ID when spawned via ContainerTarget (M19.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_id: Option<String>,
    /// Image digest (sha256) when spawned via ContainerTarget (M19.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_hash: Option<String>,
}

/// Create a new workload attestation for an agent process at spawn time.
pub fn attest_agent(
    agent_id: &str,
    pid: Option<u32>,
    compute_target: &str,
    stack_hash: &str,
) -> WorkloadAttestation {
    attest_agent_with_container(agent_id, pid, compute_target, stack_hash, None, None)
}

/// Create a workload attestation with optional container metadata (M19.4).
pub fn attest_agent_with_container(
    agent_id: &str,
    pid: Option<u32>,
    compute_target: &str,
    stack_hash: &str,
    container_id: Option<String>,
    image_hash: Option<String>,
) -> WorkloadAttestation {
    let now = now_secs();
    let hostname = read_hostname();
    WorkloadAttestation {
        agent_id: agent_id.to_string(),
        pid,
        hostname,
        compute_target: compute_target.to_string(),
        stack_fingerprint: stack_hash.to_string(),
        attested_at: now,
        alive: true,
        last_verified_at: now,
        container_id,
        image_hash,
    }
}

/// Verify an existing attestation against the current runtime state.
///
/// Updates `attestation.alive` and `attestation.last_verified_at` in place.
/// Returns `true` if all checks pass:
/// - `current_pid_alive`: caller confirms the PID probe succeeded.
/// - `current_stack`: the current stack hash; checked against the recorded
///   fingerprint unless either side is empty (unknown/unavailable).
pub fn verify_attestation(
    attestation: &mut WorkloadAttestation,
    current_pid_alive: bool,
    current_stack: &str,
) -> bool {
    attestation.last_verified_at = now_secs();

    let stack_ok = attestation.stack_fingerprint.is_empty()
        || current_stack.is_empty()
        || attestation.stack_fingerprint == current_stack;

    attestation.alive = current_pid_alive && stack_ok;
    attestation.alive
}

/// Check whether a process with the given PID is still running.
///
/// On Linux this probes `/proc/{pid}` which is cheap and race-free for our
/// purposes (we just need a best-effort liveness signal).  On other platforms
/// always returns `true` (unknown = assume alive).
pub fn pid_is_alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        true
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
fn read_hostname() -> String {
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
    fn attest_creates_alive_record() {
        let att = attest_agent("agent-1", Some(1234), "local", "sha256:abc");
        assert_eq!(att.agent_id, "agent-1");
        assert_eq!(att.pid, Some(1234));
        assert_eq!(att.compute_target, "local");
        assert_eq!(att.stack_fingerprint, "sha256:abc");
        assert!(att.alive);
        assert!(att.attested_at > 0);
        assert_eq!(att.last_verified_at, att.attested_at);
    }

    #[test]
    fn verify_alive_pid_same_stack_passes() {
        let mut att = attest_agent("agent-2", Some(999), "local", "sha256:def");
        let ok = verify_attestation(&mut att, true, "sha256:def");
        assert!(ok);
        assert!(att.alive);
    }

    #[test]
    fn verify_dead_pid_fails() {
        let mut att = attest_agent("agent-3", Some(999), "local", "sha256:ghi");
        let ok = verify_attestation(&mut att, false, "sha256:ghi");
        assert!(!ok);
        assert!(!att.alive);
    }

    #[test]
    fn verify_changed_stack_fails() {
        let mut att = attest_agent("agent-4", Some(999), "local", "sha256:original");
        let ok = verify_attestation(&mut att, true, "sha256:different");
        assert!(!ok);
        assert!(!att.alive);
    }

    #[test]
    fn verify_empty_stack_skips_hash_check() {
        // If the stack hash is unknown on either side, skip the check.
        let mut att = attest_agent("agent-5", Some(999), "local", "");
        let ok = verify_attestation(&mut att, true, "sha256:anything");
        assert!(ok);
    }

    #[test]
    fn pid_alive_current_process() {
        // The test process itself must be alive.
        let pid = std::process::id();
        assert!(pid_is_alive(pid));
    }

    #[test]
    fn pid_dead_very_large_pid() {
        // PID 4294967295 is extremely unlikely to exist.
        // On non-Linux platforms this always returns true so we skip there.
        #[cfg(target_os = "linux")]
        assert!(!pid_is_alive(u32::MAX));
    }

    #[test]
    fn hostname_is_not_empty() {
        let h = read_hostname();
        assert!(!h.is_empty());
    }
}
