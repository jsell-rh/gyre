//! Simulated audit event generator for testing without eBPF.
//!
//! Enabled via `GYRE_AUDIT_SIMULATE=true`. Generates realistic audit events
//! (file access, network connect, process exec) for active agents every 30 seconds.

use std::sync::Arc;
use tracing::info;

use gyre_common::Id;
use gyre_domain::{AuditEvent, AuditEventType};

use crate::AppState;

/// Spawn the audit simulator background task.
/// Only runs if `GYRE_AUDIT_SIMULATE=true` environment variable is set.
pub fn spawn_audit_simulator(state: Arc<AppState>) {
    if std::env::var("GYRE_AUDIT_SIMULATE").as_deref() != Ok("true") {
        return;
    }
    info!("Audit simulator enabled — generating synthetic audit events");
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            if let Err(e) = run_simulation_tick(&state).await {
                tracing::error!("audit simulator error: {:#}", e);
            }
        }
    });
}

async fn run_simulation_tick(state: &AppState) -> anyhow::Result<()> {
    let agents = state.agents.list().await?;
    let active_agents: Vec<_> = agents
        .into_iter()
        .filter(|a| matches!(a.status, gyre_domain::AgentStatus::Active))
        .collect();

    if active_agents.is_empty() {
        return Ok(());
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Generate a few events per active agent
    let sample_events: &[(AuditEventType, Option<&str>, u32)] = &[
        (AuditEventType::FileAccess, Some("/proc/self/status"), 1),
        (AuditEventType::NetworkConnect, Some("0.0.0.0:443"), 2),
        (AuditEventType::ProcessExec, Some("/usr/bin/git"), 3),
        (AuditEventType::Syscall, None, 4),
    ];

    for agent in &active_agents {
        // Pick one sample event per agent per tick to avoid flooding
        let idx = (now as usize + agent.id.as_str().len()) % sample_events.len();
        let (event_type, path, rel_pid) = &sample_events[idx];

        let event = AuditEvent::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            agent.id.clone(),
            event_type.clone(),
            path.map(|p| p.to_string()),
            serde_json::json!({ "simulated": true }),
            Some(1000 + rel_pid),
            now,
        );

        state.audit.record(&event).await?;
        let _ = state.audit_broadcast_tx.send(
            serde_json::to_string(&serde_json::json!({
                "id": event.id.as_str(),
                "agent_id": event.agent_id.as_str(),
                "event_type": event.event_type.as_str(),
                "path": event.path,
                "timestamp": event.timestamp,
                "simulated": true,
            }))
            .unwrap_or_default(),
        );
    }

    info!(
        agents = active_agents.len(),
        "Simulated audit events generated"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;

    #[tokio::test]
    async fn simulator_tick_no_agents_is_ok() {
        let state = test_state();
        // No active agents — should succeed with no events
        run_simulation_tick(&state).await.unwrap();
        let count = state.audit.count().await.unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn simulator_disabled_by_default() {
        // GYRE_AUDIT_SIMULATE is not set in test env
        assert_ne!(std::env::var("GYRE_AUDIT_SIMULATE").as_deref(), Ok("true"));
    }
}
