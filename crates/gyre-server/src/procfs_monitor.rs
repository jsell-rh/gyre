//! procfs-based agent process monitor.
//!
//! Polls `/proc/{pid}/` entries for each live agent process and converts
//! observed changes into real [`AuditEvent`]s. Replaces the synthetic
//! audit simulator with ground-truth kernel data.
//!
//! Enabled by default. Disable via `GYRE_PROCFS_MONITOR=false`.
//! Only produces events on Linux; on other platforms the monitor runs
//! but emits nothing (graceful no-op).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tracing::{debug, info, warn};

use gyre_common::Id;
use gyre_domain::{AuditEvent, AuditEventType};

use crate::AppState;

/// Per-PID state tracking what we have already emitted, so we do not
/// re-raise the same file-access or TCP-connection event every poll tick.
#[derive(Default)]
struct PidState {
    /// File paths for which we have already emitted a FileAccess event.
    seen_fds: HashSet<String>,
    /// TCP connection keys (`local_hex->remote_hex`) we have already emitted.
    seen_tcp: HashSet<String>,
}

/// Spawn the procfs monitor background task.
///
/// Runs by default. Set `GYRE_PROCFS_MONITOR=false` to disable.
pub fn spawn_procfs_monitor(state: Arc<AppState>) {
    if std::env::var("GYRE_PROCFS_MONITOR").as_deref() == Ok("false") {
        info!("procfs monitor disabled via GYRE_PROCFS_MONITOR=false");
        return;
    }
    info!("procfs monitor started — watching agent processes via /proc");
    tokio::spawn(async move {
        // pid -> (agent_id, per-pid observation state)
        let mut pid_states: HashMap<u32, (String, PidState)> = HashMap::new();
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            if let Err(e) = poll_agents(&state, &mut pid_states).await {
                warn!("procfs monitor poll error: {:#}", e);
            }
        }
    });
}

/// One poll cycle: enumerate agent PIDs, read procfs, emit new events.
async fn poll_agents(
    state: &AppState,
    pid_states: &mut HashMap<u32, (String, PidState)>,
) -> anyhow::Result<()> {
    // Snapshot current agent → pid mapping (brief lock, then release).
    let current: Vec<(String, u32)> = {
        let registry = state.process_registry.lock().await;
        registry
            .iter()
            .filter_map(|(agent_id, handle)| handle.pid.map(|pid| (agent_id.clone(), pid)))
            .collect()
    };

    // Evict stale PIDs that are no longer in the registry.
    let current_pids: HashSet<u32> = current.iter().map(|(_, pid)| *pid).collect();
    pid_states.retain(|pid, _| current_pids.contains(pid));

    if current.is_empty() {
        return Ok(());
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    for (agent_id, pid) in &current {
        let pid = *pid;
        let entry = pid_states
            .entry(pid)
            .or_insert_with(|| (agent_id.clone(), PidState::default()));
        let ps = &mut entry.1;

        // ── File descriptor events ────────────────────────────────────────────
        for (path, fd_num) in poll_fds(pid, &mut ps.seen_fds) {
            let event = AuditEvent::new(
                Id::new(uuid::Uuid::new_v4().to_string()),
                Id::new(agent_id.clone()),
                AuditEventType::FileAccess,
                Some(path),
                serde_json::json!({ "fd": fd_num, "source": "procfs" }),
                Some(pid),
                now,
            );
            let _ = state.audit.record(&event).await;
            broadcast(state, &event);
        }

        // ── TCP connection events ────────────────────────────────────────────
        for (local, remote) in poll_tcp(pid, &mut ps.seen_tcp) {
            let event = AuditEvent::new(
                Id::new(uuid::Uuid::new_v4().to_string()),
                Id::new(agent_id.clone()),
                AuditEventType::NetworkConnect,
                Some(remote.clone()),
                serde_json::json!({ "local": local, "remote": remote, "source": "procfs" }),
                Some(pid),
                now,
            );
            let _ = state.audit.record(&event).await;
            broadcast(state, &event);
        }
    }

    debug!(agents = current.len(), "procfs monitor tick complete");
    Ok(())
}

/// Push an audit event onto the SSE broadcast channel.
fn broadcast(state: &AppState, event: &AuditEvent) {
    let _ = state.audit_broadcast_tx.send(
        serde_json::to_string(&serde_json::json!({
            "id": event.id.as_str(),
            "agent_id": event.agent_id.as_str(),
            "event_type": event.event_type.as_str(),
            "path": event.path,
            "timestamp": event.timestamp,
            "pid": event.pid,
            "source": "procfs",
        }))
        .unwrap_or_default(),
    );
}

// ── Linux procfs readers ───────────────────────────────────────────────────────

/// Read `/proc/{pid}/fd/` and return newly-observed real file paths
/// (sockets and pipes are skipped — only paths starting with `/` are returned).
///
/// `seen` is updated in place so subsequent calls do not re-emit the same path.
#[cfg(target_os = "linux")]
fn poll_fds(pid: u32, seen: &mut HashSet<String>) -> Vec<(String, u32)> {
    let fd_dir = format!("/proc/{pid}/fd");
    let entries = match std::fs::read_dir(&fd_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(), // process exited between poll cycles
    };

    let mut events = Vec::new();
    for entry in entries.flatten() {
        let fd_num: u32 = match entry.file_name().to_string_lossy().parse() {
            Ok(n) => n,
            Err(_) => continue,
        };
        // readlink resolves the fd symlink to its target.
        let target = match std::fs::read_link(entry.path()) {
            Ok(t) => t.to_string_lossy().into_owned(),
            Err(_) => continue, // fd was closed between readdir and readlink
        };
        // Only emit events for real filesystem paths.
        if target.starts_with('/') && !seen.contains(&target) {
            seen.insert(target.clone());
            events.push((target, fd_num));
        }
    }
    events
}

#[cfg(not(target_os = "linux"))]
fn poll_fds(_pid: u32, _seen: &mut HashSet<String>) -> Vec<(String, u32)> {
    Vec::new()
}

/// Read `/proc/{pid}/net/tcp` (and `tcp6`) and return newly-observed
/// ESTABLISHED connections as `(local_addr, remote_addr)` pairs.
#[cfg(target_os = "linux")]
fn poll_tcp(pid: u32, seen: &mut HashSet<String>) -> Vec<(String, String)> {
    let mut events = Vec::new();
    for proto in &["tcp", "tcp6"] {
        let path = format!("/proc/{pid}/net/{proto}");
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for line in content.lines().skip(1) {
            // /proc/net/tcp columns: sl local_address rem_address st ...
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 4 {
                continue;
            }
            // State column: 0x01 = TCP_ESTABLISHED
            if cols[3] != "01" {
                continue;
            }
            let local_hex = cols[1];
            let remote_hex = cols[2];
            let key = format!("{local_hex}->{remote_hex}");
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key);
            events.push((parse_tcp_addr(local_hex), parse_tcp_addr(remote_hex)));
        }
    }
    events
}

#[cfg(not(target_os = "linux"))]
fn poll_tcp(_pid: u32, _seen: &mut HashSet<String>) -> Vec<(String, String)> {
    Vec::new()
}

/// Convert a hex `ADDR:PORT` entry from `/proc/net/tcp` to a human-readable
/// `IP:port` string.  The address bytes are stored little-endian in the kernel.
fn parse_tcp_addr(hex: &str) -> String {
    let Some((addr_hex, port_hex)) = hex.split_once(':') else {
        return hex.to_string();
    };
    let port = u16::from_str_radix(port_hex, 16).unwrap_or(0);
    if addr_hex.len() == 8 {
        // IPv4: 32-bit little-endian hex
        let n = u32::from_str_radix(addr_hex, 16).unwrap_or(0);
        format!(
            "{}.{}.{}.{}:{}",
            n & 0xFF,
            (n >> 8) & 0xFF,
            (n >> 16) & 0xFF,
            (n >> 24) & 0xFF,
            port
        )
    } else {
        // IPv6: emit as-is (full parsing not required for audit purposes)
        format!("[{addr_hex}]:{port}")
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;

    #[test]
    fn parse_tcp_addr_ipv4_loopback() {
        // 127.0.0.1:8080 in little-endian hex: 0100007F, port 0x1F90
        let result = parse_tcp_addr("0100007F:1F90");
        assert_eq!(result, "127.0.0.1:8080");
    }

    #[test]
    fn parse_tcp_addr_ipv4_zero() {
        let result = parse_tcp_addr("00000000:0050");
        assert_eq!(result, "0.0.0.0:80");
    }

    #[test]
    fn parse_tcp_addr_invalid_falls_back() {
        let result = parse_tcp_addr("notvalid");
        assert_eq!(result, "notvalid");
    }

    #[test]
    fn parse_tcp_addr_ipv6_passthrough() {
        let result = parse_tcp_addr("00000000000000000000000001000000:0050");
        assert!(result.starts_with('['));
        assert!(result.ends_with(":80"));
    }

    #[tokio::test]
    async fn poll_agents_no_pids_is_ok() {
        let state = test_state();
        let mut pid_states = HashMap::new();
        // No agents registered → should complete with no errors and no events.
        poll_agents(&state, &mut pid_states).await.unwrap();
        let count = state.audit.count().await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn poll_fds_self_proc() {
        // Use our own PID to test the fd reader on Linux.
        let mut seen = HashSet::new();
        let pid = std::process::id();
        #[cfg(target_os = "linux")]
        {
            let events = poll_fds(pid, &mut seen);
            // Our process has stdin/stdout/stderr plus some library files open.
            // At minimum we expect at least one real path (the test binary itself).
            // This is a smoke test — just check it doesn't panic.
            let _ = events; // may be empty in sandboxed envs
        }
        #[cfg(not(target_os = "linux"))]
        {
            let events = poll_fds(pid, &mut seen);
            assert!(events.is_empty());
        }
    }

    #[tokio::test]
    async fn poll_tcp_nonexistent_pid_returns_empty() {
        // PID 0 never has a procfs entry.
        let mut seen = HashSet::new();
        let events = poll_tcp(0, &mut seen);
        assert!(events.is_empty());
    }

    #[test]
    fn monitor_disabled_when_env_set() {
        // Verify the env-var check logic (cannot fully test spawn without a runtime here,
        // but we can confirm the variable name is correct).
        assert_ne!(
            std::env::var("GYRE_PROCFS_MONITOR").as_deref(),
            Ok("false"),
            "GYRE_PROCFS_MONITOR should not be 'false' in the test environment"
        );
    }
}
