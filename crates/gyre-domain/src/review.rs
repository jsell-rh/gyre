use gyre_common::Id;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewComment {
    pub id: Id,
    pub merge_request_id: Id,
    pub author_agent_id: String,
    pub body: String,
    pub file_path: Option<String>,
    pub line_number: Option<u32>,
    pub created_at: u64,
}

impl ReviewComment {
    pub fn new(
        id: Id,
        merge_request_id: Id,
        author_agent_id: impl Into<String>,
        body: impl Into<String>,
        created_at: u64,
    ) -> Self {
        Self {
            id,
            merge_request_id,
            author_agent_id: author_agent_id.into(),
            body: body.into(),
            file_path: None,
            line_number: None,
            created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub id: Id,
    pub merge_request_id: Id,
    pub reviewer_agent_id: String,
    pub decision: ReviewDecision,
    pub body: Option<String>,
    pub created_at: u64,
    /// Persona attribution for gate-agent reviews (agent-gates.md §Gate
    /// Agent Lifecycle: "gate agent reviews are attributed — the review
    /// shows which gate persona produced it"). Set by the server from the
    /// scoped reviewer token's gate (the JWT `task_id` claim), never from
    /// caller-supplied body fields. `None` for human/agent reviews that
    /// were not produced by a gate.
    #[serde(default)]
    pub reviewer_persona: Option<String>,
    /// Id of the gate whose agent produced this review, when the reviewer
    /// authenticated with a gate-agent scoped token. Binds the review to
    /// the gate chain entry that spawned the reviewer.
    #[serde(default)]
    pub gate_id: Option<Id>,
}

impl Review {
    pub fn new(
        id: Id,
        merge_request_id: Id,
        reviewer_agent_id: impl Into<String>,
        decision: ReviewDecision,
        created_at: u64,
    ) -> Self {
        Self {
            id,
            merge_request_id,
            reviewer_agent_id: reviewer_agent_id.into(),
            decision,
            body: None,
            created_at,
            reviewer_persona: None,
            gate_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_review_comment_new() {
        let comment =
            ReviewComment::new(Id::new("c1"), Id::new("mr1"), "agent-1", "Looks good", 1000);
        assert_eq!(comment.body, "Looks good");
        assert!(comment.file_path.is_none());
        assert!(comment.line_number.is_none());
    }

    #[test]
    fn test_review_new() {
        let review = Review::new(
            Id::new("r1"),
            Id::new("mr1"),
            "agent-1",
            ReviewDecision::Approved,
            1000,
        );
        assert_eq!(review.decision, ReviewDecision::Approved);
        assert!(review.body.is_none());
        assert!(
            review.reviewer_persona.is_none(),
            "attribution starts unset; only the server sets it from a scoped gate token"
        );
        assert!(review.gate_id.is_none());
    }

    #[test]
    fn test_review_decision_variants() {
        assert_ne!(ReviewDecision::Approved, ReviewDecision::ChangesRequested);
    }
}
