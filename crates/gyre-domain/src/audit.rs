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
    // Source control events (observability.md §Audit Event Types)
    GitPushAccepted,
    GitPushRejected,
    GitClone,
    SpecChanged,
    // Access control events (observability.md §Audit Event Types)
    AuthSuccess,
    AuthFailure,
    AbacDenied,
    RbacDenied,
    // Impersonation lifecycle (identity-security.md §Impersonation,
    // observability.md §Audit Event Types)
    ImpersonationStarted,
    ImpersonationEnded,
    // Agent lifecycle events (observability.md §Audit Event Types)
    AgentSpawned,
    AgentCompleted,
    AgentKilled,
    AgentDead,
    AgentHeartbeat,
    AgentTokenRevoked,
    AgentReassigned,
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
            Self::GitPushAccepted => "git_push_accepted".to_string(),
            Self::GitPushRejected => "git_push_rejected".to_string(),
            Self::GitClone => "git_clone".to_string(),
            Self::SpecChanged => "spec_changed".to_string(),
            Self::AuthSuccess => "auth_success".to_string(),
            Self::AuthFailure => "auth_failure".to_string(),
            Self::AbacDenied => "abac_denied".to_string(),
            Self::RbacDenied => "rbac_denied".to_string(),
            Self::ImpersonationStarted => "impersonation_started".to_string(),
            Self::ImpersonationEnded => "impersonation_ended".to_string(),
            Self::AgentSpawned => "agent_spawned".to_string(),
            Self::AgentCompleted => "agent_completed".to_string(),
            Self::AgentKilled => "agent_killed".to_string(),
            Self::AgentDead => "agent_dead".to_string(),
            Self::AgentHeartbeat => "agent_heartbeat".to_string(),
            Self::AgentTokenRevoked => "agent_token_revoked".to_string(),
            Self::AgentReassigned => "agent_reassigned".to_string(),
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
            "git_push_accepted" => Self::GitPushAccepted,
            "git_push_rejected" => Self::GitPushRejected,
            "git_clone" => Self::GitClone,
            "spec_changed" => Self::SpecChanged,
            "auth_success" => Self::AuthSuccess,
            "auth_failure" => Self::AuthFailure,
            "abac_denied" => Self::AbacDenied,
            "rbac_denied" => Self::RbacDenied,
            "impersonation_started" => Self::ImpersonationStarted,
            "impersonation_ended" => Self::ImpersonationEnded,
            "agent_spawned" => Self::AgentSpawned,
            "agent_completed" => Self::AgentCompleted,
            "agent_killed" => Self::AgentKilled,
            "agent_dead" => Self::AgentDead,
            "agent_heartbeat" => Self::AgentHeartbeat,
            "agent_token_revoked" => Self::AgentTokenRevoked,
            "agent_reassigned" => Self::AgentReassigned,
            other => Self::Custom(other.to_string()),
        }
    }
}

/// Outcome of the audited action (observability.md §Audit Event Schema).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditOutcome {
    Success,
    Failure,
    Blocked,
}

impl AuditOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Blocked => "blocked",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "success" => Some(Self::Success),
            "failure" => Some(Self::Failure),
            "blocked" => Some(Self::Blocked),
            _ => None,
        }
    }
}

/// An audit event recording agent activity for security and compliance.
///
/// Spec envelope (observability.md §Audit Event Schema): every event shares
/// these 14 fields; event-specific data lives in `detail`. The old `path`
/// and `pid` fields are subsumed by `detail` JSON payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: Id,
    pub event_type: AuditEventType,
    /// Null for server-initiated events.
    pub agent_id: Option<Id>,
    /// Null for agent-only events.
    pub user_id: Option<Id>,
    /// WebSocket or HTTP session.
    pub session_id: Option<String>,
    pub workspace_id: Option<Id>,
    pub repo_id: Option<Id>,
    /// "agent", "task", "mr", "repo", "container", etc.
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub outcome: AuditOutcome,
    pub detail: serde_json::Value,
    pub source_ip: Option<String>,
    pub user_agent: Option<String>,
    pub timestamp: u64,
}

impl AuditEvent {
    pub fn new(
        id: Id,
        event_type: AuditEventType,
        agent_id: Option<Id>,
        user_id: Option<Id>,
        session_id: Option<String>,
        workspace_id: Option<Id>,
        repo_id: Option<Id>,
        resource_type: String,
        resource_id: Option<String>,
        outcome: AuditOutcome,
        detail: serde_json::Value,
        source_ip: Option<String>,
        user_agent: Option<String>,
        timestamp: u64,
    ) -> Self {
        Self {
            id,
            event_type,
            agent_id,
            user_id,
            session_id,
            workspace_id,
            repo_id,
            resource_type,
            resource_id,
            outcome,
            detail,
            source_ip,
            user_agent,
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
            AuditEventType::GitPushAccepted,
            AuditEventType::GitPushRejected,
            AuditEventType::GitClone,
            AuditEventType::SpecChanged,
            AuditEventType::AuthSuccess,
            AuditEventType::AuthFailure,
            AuditEventType::AbacDenied,
            AuditEventType::RbacDenied,
            AuditEventType::ImpersonationStarted,
            AuditEventType::ImpersonationEnded,
            AuditEventType::AgentSpawned,
            AuditEventType::AgentCompleted,
            AuditEventType::AgentKilled,
            AuditEventType::AgentDead,
            AuditEventType::AgentHeartbeat,
            AuditEventType::AgentTokenRevoked,
            AuditEventType::AgentReassigned,
            AuditEventType::Custom("custom_event".to_string()),
        ];
        for t in &types {
            assert_eq!(AuditEventType::from_str(&t.as_str()), *t);
        }
    }

    #[test]
    fn source_control_and_access_control_event_types_roundtrip() {
        // observability.md §Audit Event Types: the 4 source control and 6
        // access control event types must round-trip through as_str/from_str.
        let types = [
            (AuditEventType::GitPushAccepted, "git_push_accepted"),
            (AuditEventType::GitPushRejected, "git_push_rejected"),
            (AuditEventType::GitClone, "git_clone"),
            (AuditEventType::SpecChanged, "spec_changed"),
            (AuditEventType::AuthSuccess, "auth_success"),
            (AuditEventType::AuthFailure, "auth_failure"),
            (AuditEventType::AbacDenied, "abac_denied"),
            (AuditEventType::RbacDenied, "rbac_denied"),
            (
                AuditEventType::ImpersonationStarted,
                "impersonation_started",
            ),
            (AuditEventType::ImpersonationEnded, "impersonation_ended"),
        ];
        for (t, expected_str) in &types {
            assert_eq!(t.as_str(), *expected_str);
            assert_eq!(AuditEventType::from_str(expected_str), *t);
        }
    }

    #[test]
    fn audit_event_new_matches_spec_envelope() {
        let e = AuditEvent::new(
            Id::new("e1"),
            AuditEventType::FileAccess,
            Some(Id::new("agent-1")),
            None,
            Some("ws-session-1".to_string()),
            Some(Id::new("ws-1")),
            Some(Id::new("repo-1")),
            "agent".to_string(),
            Some("agent-1".to_string()),
            AuditOutcome::Success,
            serde_json::json!({ "path": "/etc/passwd", "operation": "read", "pid": 1234 }),
            Some("127.0.0.1".to_string()),
            Some("gyre-agent/1.0".to_string()),
            1000,
        );
        assert_eq!(e.agent_id, Some(Id::new("agent-1")));
        assert_eq!(e.user_id, None);
        assert_eq!(e.session_id.as_deref(), Some("ws-session-1"));
        assert_eq!(e.workspace_id, Some(Id::new("ws-1")));
        assert_eq!(e.repo_id, Some(Id::new("repo-1")));
        assert_eq!(e.resource_type, "agent");
        assert_eq!(e.resource_id.as_deref(), Some("agent-1"));
        assert_eq!(e.outcome, AuditOutcome::Success);
        assert_eq!(e.detail["path"], "/etc/passwd");
        assert_eq!(e.source_ip.as_deref(), Some("127.0.0.1"));
        assert_eq!(e.user_agent.as_deref(), Some("gyre-agent/1.0"));
        assert_eq!(e.timestamp, 1000);
    }

    #[test]
    fn audit_event_server_initiated_has_no_agent() {
        // Server-initiated events carry no agent_id (spec: "Null for
        // server-initiated events") - e.g. an admin action.
        let e = AuditEvent::new(
            Id::new("e2"),
            AuditEventType::Custom("auth_failure".to_string()),
            None,
            Some(Id::new("user-1")),
            None,
            None,
            None,
            "user".to_string(),
            Some("user-1".to_string()),
            AuditOutcome::Failure,
            serde_json::json!({ "rejection_reason": "revoked" }),
            Some("10.0.0.1".to_string()),
            None,
            2000,
        );
        assert_eq!(e.agent_id, None);
        assert_eq!(e.user_id, Some(Id::new("user-1")));
        assert_eq!(e.outcome, AuditOutcome::Failure);
    }

    #[test]
    fn audit_outcome_round_trip() {
        for (variant, s) in [
            (AuditOutcome::Success, "success"),
            (AuditOutcome::Failure, "failure"),
            (AuditOutcome::Blocked, "blocked"),
        ] {
            assert_eq!(variant.as_str(), s);
            assert_eq!(AuditOutcome::from_str(s), Some(variant));
        }
        assert_eq!(AuditOutcome::from_str("nonsense"), None);
    }

    #[test]
    fn audit_outcome_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&AuditOutcome::Blocked).unwrap(),
            "\"blocked\""
        );
        let back: AuditOutcome = serde_json::from_str("\"failure\"").unwrap();
        assert_eq!(back, AuditOutcome::Failure);
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

    #[test]
    fn audit_event_envelope_serializes_all_14_fields() {
        // Spec SSE example (observability.md) shows the full envelope as JSON;
        // every field must round-trip.
        let e = AuditEvent::new(
            Id::new("e3"),
            AuditEventType::ContainerStarted,
            Some(Id::new("agent-9")),
            Some(Id::new("user-9")),
            None,
            None,
            None,
            "container".to_string(),
            Some("abc123".to_string()),
            AuditOutcome::Success,
            serde_json::json!({ "container_id": "abc123" }),
            None,
            None,
            1711036800,
        );
        let j = serde_json::to_value(&e).unwrap();
        for key in [
            "id",
            "event_type",
            "agent_id",
            "user_id",
            "session_id",
            "workspace_id",
            "repo_id",
            "resource_type",
            "resource_id",
            "outcome",
            "detail",
            "source_ip",
            "user_agent",
            "timestamp",
        ] {
            assert!(j.get(key).is_some(), "envelope missing field {key}");
        }
        let back: AuditEvent = serde_json::from_value(j).unwrap();
        assert_eq!(back.id, e.id);
        assert_eq!(back.outcome, e.outcome);
        assert_eq!(back.detail, e.detail);
    }
}
