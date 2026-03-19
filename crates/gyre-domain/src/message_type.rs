use gyre_common::Id;
use serde::{Deserialize, Serialize};

/// Typed message variants for structured agent-to-agent communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageType {
    TaskAssignment {
        task_id: Id,
        spec_ref: Option<String>,
    },
    ReviewRequest {
        mr_id: Id,
    },
    StatusUpdate {
        status: String,
        summary: String,
    },
    Escalation {
        reason: String,
        context: Option<String>,
    },
    FreeText {
        body: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_assignment_serializes() {
        let mt = MessageType::TaskAssignment {
            task_id: Id::new("task-1"),
            spec_ref: Some("spec.md".to_string()),
        };
        let json = serde_json::to_value(&mt).unwrap();
        assert_eq!(json["type"], "task_assignment");
        assert_eq!(json["task_id"], "task-1");
        assert_eq!(json["spec_ref"], "spec.md");
    }

    #[test]
    fn test_review_request_serializes() {
        let mt = MessageType::ReviewRequest {
            mr_id: Id::new("mr-1"),
        };
        let json = serde_json::to_value(&mt).unwrap();
        assert_eq!(json["type"], "review_request");
        assert_eq!(json["mr_id"], "mr-1");
    }

    #[test]
    fn test_status_update_serializes() {
        let mt = MessageType::StatusUpdate {
            status: "active".to_string(),
            summary: "working on it".to_string(),
        };
        let json = serde_json::to_value(&mt).unwrap();
        assert_eq!(json["type"], "status_update");
        assert_eq!(json["status"], "active");
    }

    #[test]
    fn test_escalation_serializes() {
        let mt = MessageType::Escalation {
            reason: "blocked".to_string(),
            context: None,
        };
        let json = serde_json::to_value(&mt).unwrap();
        assert_eq!(json["type"], "escalation");
    }

    #[test]
    fn test_free_text_serializes() {
        let mt = MessageType::FreeText {
            body: "hello".to_string(),
        };
        let json = serde_json::to_value(&mt).unwrap();
        assert_eq!(json["type"], "free_text");
        assert_eq!(json["body"], "hello");
    }

    #[test]
    fn test_roundtrip_deserialization() {
        let mt = MessageType::TaskAssignment {
            task_id: Id::new("t1"),
            spec_ref: None,
        };
        let json = serde_json::to_string(&mt).unwrap();
        let decoded: MessageType = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, MessageType::TaskAssignment { .. }));
    }
}
