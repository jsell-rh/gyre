//! Agent completion summary types (HSI §4).
//!
//! The `AgentCompletionSummary` is submitted as part of the `agent.complete` MCP tool call.
//! The server stores it in the MR attestation bundle and emits an `AgentCompleted` message.

use serde::{Deserialize, Serialize};

/// A decision made by an agent during task implementation.
///
/// Captures what was decided, why, how confident the agent was,
/// and what alternatives were considered.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Decision {
    /// What was decided (e.g. "Used exponential backoff for retry").
    pub what: String,
    /// Why this decision was made (e.g. "Spec mentions idempotency requirement").
    pub why: String,
    /// Agent's confidence level: "high", "medium", or "low".
    pub confidence: String,
    /// Alternatives considered but not chosen (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alternatives_considered: Option<Vec<String>>,
}

/// Completion summary submitted by the agent via `agent.complete` (HSI §4).
///
/// The server:
/// 1. Stores this in the MR attestation bundle.
/// 2. Emits an `AgentCompleted` Event-tier message to the workspace bus.
/// 3. Synchronously creates priority-1 `AgentNeedsClarification` notifications
///    for all workspace Admin/Developer members when `uncertainties` is non-empty.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentCompletionSummary {
    /// Spec that this task was implementing (e.g. "specs/system/payment-retry.md").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec_ref: Option<String>,
    /// Decisions made during implementation.
    pub decisions: Vec<Decision>,
    /// Open questions or areas where the spec was ambiguous.
    /// Non-empty triggers synchronous priority-1 notifications.
    pub uncertainties: Vec<String>,
    /// SHA-256 of the full conversation history (uploaded via `conversation.upload`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_sha: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_roundtrip() {
        let d = Decision {
            what: "Used exponential backoff".to_string(),
            why: "Idempotency requirement".to_string(),
            confidence: "high".to_string(),
            alternatives_considered: Some(vec!["fixed interval".to_string()]),
        };
        let json = serde_json::to_string(&d).unwrap();
        let back: Decision = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn decision_without_alternatives_omits_field() {
        let d = Decision {
            what: "Split module".to_string(),
            why: "Hexagonal boundary".to_string(),
            confidence: "medium".to_string(),
            alternatives_considered: None,
        };
        let json = serde_json::to_string(&d).unwrap();
        assert!(!json.contains("alternatives_considered"));
    }

    #[test]
    fn summary_roundtrip() {
        let s = AgentCompletionSummary {
            spec_ref: Some("specs/system/retry.md".to_string()),
            decisions: vec![Decision {
                what: "Used exponential backoff".to_string(),
                why: "Idempotency requirement".to_string(),
                confidence: "high".to_string(),
                alternatives_considered: None,
            }],
            uncertainties: vec!["Timeout behavior undefined".to_string()],
            conversation_sha: Some("abc123".to_string()),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: AgentCompletionSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn empty_uncertainties_does_not_trigger_notifications_precondition() {
        // This is a unit test of the type — the actual notification creation
        // logic lives in the server layer. We just verify the type is correct.
        let s = AgentCompletionSummary {
            spec_ref: None,
            decisions: vec![],
            uncertainties: vec![],
            conversation_sha: None,
        };
        assert!(s.uncertainties.is_empty());
    }
}
