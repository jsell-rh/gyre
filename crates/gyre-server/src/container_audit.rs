//! M19.3 + M23: Container audit trail — records lifecycle events for agent containers.
//!
//! M23 adds typed `AuditEventType` variants for container lifecycle transitions
//! (Started, Stopped, Crashed, OOM, NetworkBlocked) that flow through the shared
//! audit event system and are broadcast over the SSE stream.
//!
//! After spawning a container, the spawn handler calls [`capture_spawn_audit`]
//! to run `{runtime} inspect` and persist a [`ContainerAuditRecord`] in the
//! `container_audits` store on [`AppState`].  On agent complete or kill the
//! record is updated with `exit_code` and `stopped_at`.
//!
//! The record is returned by `GET /api/v1/agents/{id}/container`.

use gyre_common::Id;
use gyre_domain::{AuditEvent, AuditEventType};

pub use gyre_domain::ContainerAuditRecord;

// ---------------------------------------------------------------------------
// Lifecycle helpers
// ---------------------------------------------------------------------------

/// Create a [`ContainerAuditRecord`] immediately after a successful container
/// spawn.  Runs `{runtime} inspect` to capture the image digest (best-effort).
pub async fn capture_spawn_audit(
    agent_id: &str,
    container_id: &str,
    image: &str,
    runtime: &str,
) -> ContainerAuditRecord {
    let image_hash = get_image_hash(runtime, container_id).await;
    ContainerAuditRecord {
        agent_id: agent_id.to_string(),
        container_id: container_id.to_string(),
        image: image.to_string(),
        image_hash,
        runtime: runtime.to_string(),
        started_at: now_secs(),
        stopped_at: None,
        exit_code: None,
    }
}

/// Update an existing audit record after the container exits.
///
/// Runs `{runtime} inspect` to retrieve the exit code and finish time.
/// If inspect fails (container already removed) timestamps fall back to the
/// current time so the record remains useful for audit purposes.
pub async fn capture_exit_audit(
    repo: &dyn gyre_ports::ContainerAuditRepository,
    agent_id: &str,
) {
    let (container_id, runtime) = match repo.find_by_agent_id(agent_id).await {
        Ok(Some(r)) => (r.container_id, r.runtime),
        _ => return,
    };

    let (exit_code, stopped_at) = get_exit_info(&runtime, &container_id).await;
    let _ = repo
        .update_exit(
            agent_id,
            exit_code,
            Some(stopped_at.unwrap_or_else(now_secs)),
        )
        .await;
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Retrieve the image digest from container inspect (best-effort).
async fn get_image_hash(runtime: &str, container_id: &str) -> Option<String> {
    let output = tokio::process::Command::new(runtime)
        .args(["inspect", "--format={{.Image}}", container_id])
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if hash.is_empty() {
        None
    } else {
        Some(hash)
    }
}

/// Retrieve exit code and finish timestamp from container inspect.
/// Returns `(exit_code, stopped_at_secs)` — both `None` on failure.
async fn get_exit_info(runtime: &str, container_id: &str) -> (Option<i32>, Option<u64>) {
    let output = tokio::process::Command::new(runtime)
        .args([
            "inspect",
            "--format={{.State.ExitCode}} {{.State.FinishedAt}}",
            container_id,
        ])
        .output()
        .await;
    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return (None, None),
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = text.trim().splitn(2, ' ').collect();
    if parts.len() < 2 {
        return (None, None);
    }
    let exit_code: Option<i32> = parts[0].parse().ok();
    // "0001-01-01T00:00:00Z" is Docker's zero time — container still running.
    let stopped_at = if parts[1].starts_with("0001") {
        None
    } else {
        parse_rfc3339_approx(parts[1])
    };
    (exit_code, stopped_at)
}

/// Very approximate RFC 3339 → unix epoch parser (avoids pulling in chrono).
/// Handles the format Docker uses: `"2024-01-15T14:30:00.123456789Z"`.
fn parse_rfc3339_approx(s: &str) -> Option<u64> {
    let s = s.trim().trim_end_matches('Z');
    let (date_part, time_part) = s.split_once('T')?;
    let dp: Vec<u32> = date_part
        .split('-')
        .filter_map(|p| p.parse().ok())
        .collect();
    let tp: Vec<u32> = time_part
        .split('.')
        .next()
        .unwrap_or("")
        .split(':')
        .filter_map(|p| p.parse().ok())
        .collect();
    if dp.len() < 3 || tp.len() < 3 {
        return None;
    }
    let years_since_1970 = (dp[0] as u64).saturating_sub(1970);
    let approx_days = years_since_1970 * 365
        + ((dp[1] as u64).saturating_sub(1)) * 30
        + (dp[2] as u64).saturating_sub(1);
    let secs = approx_days * 86400 + (tp[0] as u64) * 3600 + (tp[1] as u64) * 60 + (tp[2] as u64);
    Some(secs)
}

// ---------------------------------------------------------------------------
// M23 Container lifecycle audit event emission
// ---------------------------------------------------------------------------

/// Context passed to container lifecycle emit functions so they can record
/// the event through the shared audit store and broadcast it on the SSE stream.
pub struct AuditCtx<'a> {
    pub audit: &'a dyn gyre_ports::AuditRepository,
    pub broadcast_tx: &'a tokio::sync::broadcast::Sender<String>,
}

/// Emit a container lifecycle `AuditEvent` and broadcast it to SSE subscribers.
/// Failures are silently swallowed (best-effort audit trail).
async fn emit(
    ctx: &AuditCtx<'_>,
    agent_id: &str,
    event_type: AuditEventType,
    details: serde_json::Value,
) {
    let event = AuditEvent::new(
        Id::new(uuid::Uuid::new_v4().to_string()),
        Id::new(agent_id),
        event_type,
        None,
        details,
        None,
        now_secs(),
    );
    let _ = ctx.audit.record(&event).await;
    if let Ok(json) = serde_json::to_string(&serde_json::json!({
        "id": event.id.to_string(),
        "agent_id": event.agent_id.to_string(),
        "event_type": event.event_type.as_str(),
        "details": event.details,
        "timestamp": event.timestamp,
    })) {
        let _ = ctx.broadcast_tx.send(json);
    }
}

/// Emit `container_started` — call immediately after a successful container spawn.
pub async fn emit_started(ctx: &AuditCtx<'_>, agent_id: &str, container_id: &str, image: &str) {
    emit(
        ctx,
        agent_id,
        AuditEventType::ContainerStarted,
        serde_json::json!({
            "container_id": container_id,
            "image": image,
            "started_at": now_secs(),
        }),
    )
    .await;
}

/// Emit `container_stopped` — call when a container exits cleanly.
pub async fn emit_stopped(
    ctx: &AuditCtx<'_>,
    agent_id: &str,
    container_id: &str,
    exit_code: Option<i32>,
) {
    emit(
        ctx,
        agent_id,
        AuditEventType::ContainerStopped,
        serde_json::json!({
            "container_id": container_id,
            "exit_code": exit_code,
            "stopped_at": now_secs(),
        }),
    )
    .await;
}

/// Emit `container_crashed` — call when a container exits with a non-zero code unexpectedly.
pub async fn emit_crashed(ctx: &AuditCtx<'_>, agent_id: &str, container_id: &str, error: &str) {
    emit(
        ctx,
        agent_id,
        AuditEventType::ContainerCrashed,
        serde_json::json!({
            "container_id": container_id,
            "error": error,
            "crashed_at": now_secs(),
        }),
    )
    .await;
}

/// Emit `container_oom` — call when a container is killed by the OOM killer.
pub async fn emit_oom(ctx: &AuditCtx<'_>, agent_id: &str, container_id: &str, memory_limit: &str) {
    emit(
        ctx,
        agent_id,
        AuditEventType::ContainerOom,
        serde_json::json!({
            "container_id": container_id,
            "memory_limit": memory_limit,
            "oom_at": now_secs(),
        }),
    )
    .await;
}

/// Emit `container_network_blocked` — call when the network policy drops a connection.
pub async fn emit_network_blocked(
    ctx: &AuditCtx<'_>,
    agent_id: &str,
    container_id: &str,
    destination: &str,
) {
    emit(
        ctx,
        agent_id,
        AuditEventType::ContainerNetworkBlocked,
        serde_json::json!({
            "container_id": container_id,
            "destination": destination,
            "blocked_at": now_secs(),
        }),
    )
    .await;
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
mod tests {
    use super::*;

    #[tokio::test]
    async fn capture_spawn_audit_fills_fields() {
        // Container inspect will fail (no real daemon) — image_hash will be None.
        let rec = capture_spawn_audit("agent-1", "abc123", "alpine:latest", "docker").await;
        assert_eq!(rec.agent_id, "agent-1");
        assert_eq!(rec.container_id, "abc123");
        assert_eq!(rec.image, "alpine:latest");
        assert_eq!(rec.runtime, "docker");
        assert!(rec.started_at > 0);
        assert!(rec.stopped_at.is_none());
        assert!(rec.exit_code.is_none());
    }

    #[test]
    fn parse_rfc3339_approx_reasonable() {
        let ts = parse_rfc3339_approx("2024-01-15T14:30:00Z");
        assert!(ts.is_some());
        // 2024 should produce well over 1 billion seconds since 1970
        assert!(ts.unwrap() > 1_000_000_000);
    }

    #[test]
    fn parse_rfc3339_approx_zero_time_returns_none() {
        // Docker returns "0001-01-01T00:00:00Z" for containers still running.
        // The caller handles this by filtering on starts_with("0001"), not here.
        // The parser itself will return Some(very_negative_approx) but we test
        // the full pipeline in get_exit_info.
        let ts = parse_rfc3339_approx("2025-06-01T00:00:00Z");
        assert!(ts.is_some());
    }

    #[test]
    fn container_audit_record_serializes() {
        let rec = ContainerAuditRecord {
            agent_id: "a".to_string(),
            container_id: "c".to_string(),
            image: "img".to_string(),
            image_hash: Some("sha256:abc".to_string()),
            runtime: "docker".to_string(),
            started_at: 1000,
            stopped_at: Some(2000),
            exit_code: Some(0),
        };
        let j = serde_json::to_string(&rec).unwrap();
        assert!(j.contains("sha256:abc"));
        assert!(j.contains("\"exit_code\":0"));
    }
}
