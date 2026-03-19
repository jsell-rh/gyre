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
    }

    #[test]
    fn test_review_decision_variants() {
        assert_ne!(ReviewDecision::Approved, ReviewDecision::ChangesRequested);
    }
}
