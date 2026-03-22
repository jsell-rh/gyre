use gyre_common::Id;
use serde::{Deserialize, Serialize};

/// The type of audit event captured.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventType {
    FileAccess,
    NetworkConnect,
    ProcessExec,
    Syscall,
    // Container lifecycle events (M23)
    ContainerStarted,
    ContainerStopped,
    ContainerCrashed,
    ContainerOom,
    ContainerNetworkBlocked,
    Custom(String),
}

impl AuditEventType {
    pub fn as_str(&self) -> String {
        match self {
            Self::FileAccess => "file_access".to_string(),
            Self::NetworkConnect => "network_connect".to_string(),
            Self::ProcessExec => "process_exec".to_string(),
            Self::Syscall => "syscall".to_string(),
            Self::ContainerStarted => "container_started".to_string(),
            Self::ContainerStopped => "container_stopped".to_string(),
            Self::ContainerCrashed => "container_crashed".to_string(),
            Self::ContainerOom => "container_oom".to_string(),
            Self::ContainerNetworkBlocked => "container_network_blocked".to_string(),
            Self::Custom(s) => s.clone(),
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "file_access" => Self::FileAccess,
            "network_connect" => Self::NetworkConnect,
            "process_exec" => Self::ProcessExec,
            "syscall" => Self::Syscall,
            "container_started" => Self::ContainerStarted,
            "container_stopped" => Self::ContainerStopped,
            "container_crashed" => Self::ContainerCrashed,
            "container_oom" => Self::ContainerOom,
            "container_network_blocked" => Self::ContainerNetworkBlocked,
            other => Self::Custom(other.to_string()),
        }
    }
}

/// An audit event recording agent activity for security and compliance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: Id,
    pub agent_id: Id,
    pub event_type: AuditEventType,
    pub path: Option<String>,
    pub details: serde_json::Value,
    pub pid: Option<u32>,
    pub timestamp: u64,
}

impl AuditEvent {
    pub fn new(
        id: Id,
        agent_id: Id,
        event_type: AuditEventType,
        path: Option<String>,
        details: serde_json::Value,
        pid: Option<u32>,
        timestamp: u64,
    ) -> Self {
        Self {
            id,
            agent_id,
            event_type,
            path,
            details,
            pid,
            timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_event_type_round_trip() {
        let types = [
            AuditEventType::FileAccess,
            AuditEventType::NetworkConnect,
            AuditEventType::ProcessExec,
            AuditEventType::Syscall,
            AuditEventType::ContainerStarted,
            AuditEventType::ContainerStopped,
            AuditEventType::ContainerCrashed,
            AuditEventType::ContainerOom,
            AuditEventType::ContainerNetworkBlocked,
            AuditEventType::Custom("custom_event".to_string()),
        ];
        for t in &types {
            assert_eq!(AuditEventType::from_str(&t.as_str()), *t);
        }
    }

    #[test]
    fn audit_event_new() {
        let e = AuditEvent::new(
            Id::new("e1"),
            Id::new("agent-1"),
            AuditEventType::FileAccess,
            Some("/etc/passwd".to_string()),
            serde_json::json!({ "mode": "read" }),
            Some(1234),
            1000,
        );
        assert_eq!(e.event_type, AuditEventType::FileAccess);
        assert_eq!(e.path.as_deref(), Some("/etc/passwd"));
        assert_eq!(e.pid, Some(1234));
        assert_eq!(e.timestamp, 1000);
    }

    #[test]
    fn container_lifecycle_event_types_roundtrip() {
        let types = [
            (AuditEventType::ContainerStarted, "container_started"),
            (AuditEventType::ContainerStopped, "container_stopped"),
            (AuditEventType::ContainerCrashed, "container_crashed"),
            (AuditEventType::ContainerOom, "container_oom"),
            (
                AuditEventType::ContainerNetworkBlocked,
                "container_network_blocked",
            ),
        ];
        for (t, expected_str) in &types {
            assert_eq!(t.as_str(), *expected_str);
            assert_eq!(AuditEventType::from_str(expected_str), *t);
        }
    }

    #[test]
    fn audit_event_type_serializes() {
        let t = AuditEventType::FileAccess;
        let s = serde_json::to_string(&t).unwrap();
        let back: AuditEventType = serde_json::from_str(&s).unwrap();
        assert_eq!(back, t);
    }
}
