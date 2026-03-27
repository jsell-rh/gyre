use gyre_common::Id;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MrError {
    #[error("invalid status transition from {from:?} to {to:?}")]
    InvalidTransition { from: MrStatus, to: MrStatus },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MrStatus {
    Open,
    Approved,
    Merged,
    Closed,
    /// MR was merged then reverted via the recovery protocol.
    Reverted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffStats {
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeRequest {
    pub id: Id,
    pub repository_id: Id,
    pub title: String,
    pub source_branch: String,
    pub target_branch: String,
    pub status: MrStatus,
    pub author_agent_id: Option<Id>,
    pub reviewers: Vec<Id>,
    pub diff_stats: Option<DiffStats>,
    pub has_conflicts: Option<bool>,
    /// Optional spec reference "path/to/spec.md@<40-char-sha>" for cryptographic binding.
    pub spec_ref: Option<String>,
    /// MR IDs that must be merged before this MR can be processed.
    pub depends_on: Vec<Id>,
    /// Atomic group identifier — all members of the group must merge together.
    pub atomic_group: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    /// Workspace that governs this MR (ABAC boundary). Non-optional per M34 hierarchy enforcement.
    pub workspace_id: Id,
    /// Unix timestamp when this MR was reverted via recovery protocol.
    pub reverted_at: Option<u64>,
    /// ID of the revert MR that undid this MR's changes.
    pub revert_mr_id: Option<Id>,
}

impl MergeRequest {
    pub fn new(
        id: Id,
        repository_id: Id,
        title: impl Into<String>,
        source_branch: impl Into<String>,
        target_branch: impl Into<String>,
        created_at: u64,
    ) -> Self {
        Self {
            id,
            repository_id,
            title: title.into(),
            source_branch: source_branch.into(),
            target_branch: target_branch.into(),
            status: MrStatus::Open,
            author_agent_id: None,
            reviewers: Vec::new(),
            diff_stats: None,
            has_conflicts: None,
            spec_ref: None,
            depends_on: Vec::new(),
            atomic_group: None,
            created_at,
            updated_at: created_at,
            workspace_id: Id::new("default"),
            reverted_at: None,
            revert_mr_id: None,
        }
    }

    /// Valid transitions:
    /// Open → Approved | Closed
    /// Approved → Merged | Closed
    /// Merged → Reverted (recovery protocol)
    /// Closed and Reverted are terminal
    pub fn transition_status(&mut self, new_status: MrStatus) -> Result<(), MrError> {
        let valid = matches!(
            (&self.status, &new_status),
            (MrStatus::Open, MrStatus::Approved)
                | (MrStatus::Open, MrStatus::Closed)
                | (MrStatus::Approved, MrStatus::Merged)
                | (MrStatus::Approved, MrStatus::Closed)
                | (MrStatus::Merged, MrStatus::Reverted)
        );
        if valid {
            self.status = new_status;
            Ok(())
        } else {
            Err(MrError::InvalidTransition {
                from: self.status.clone(),
                to: new_status,
            })
        }
    }

    /// Mark this MR as reverted via the recovery protocol.
    pub fn revert(&mut self, revert_mr_id: Id, now: u64) -> Result<(), MrError> {
        self.transition_status(MrStatus::Reverted)?;
        self.reverted_at = Some(now);
        self.revert_mr_id = Some(revert_mr_id);
        self.updated_at = now;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mr() -> MergeRequest {
        MergeRequest::new(
            Id::new("mr1"),
            Id::new("repo1"),
            "Add feature",
            "feat/thing",
            "main",
            1000,
        )
    }

    #[test]
    fn test_new_mr_is_open() {
        let mr = make_mr();
        assert_eq!(mr.status, MrStatus::Open);
        assert!(mr.author_agent_id.is_none());
        assert!(mr.reviewers.is_empty());
    }

    #[test]
    fn test_open_to_approved() {
        let mut mr = make_mr();
        assert!(mr.transition_status(MrStatus::Approved).is_ok());
        assert_eq!(mr.status, MrStatus::Approved);
    }

    #[test]
    fn test_open_to_closed() {
        let mut mr = make_mr();
        assert!(mr.transition_status(MrStatus::Closed).is_ok());
    }

    #[test]
    fn test_approved_to_merged() {
        let mut mr = make_mr();
        mr.transition_status(MrStatus::Approved).unwrap();
        assert!(mr.transition_status(MrStatus::Merged).is_ok());
    }

    #[test]
    fn test_approved_to_closed() {
        let mut mr = make_mr();
        mr.transition_status(MrStatus::Approved).unwrap();
        assert!(mr.transition_status(MrStatus::Closed).is_ok());
    }

    #[test]
    fn test_merged_is_terminal() {
        let mut mr = make_mr();
        mr.transition_status(MrStatus::Approved).unwrap();
        mr.transition_status(MrStatus::Merged).unwrap();
        assert!(mr.transition_status(MrStatus::Closed).is_err());
    }

    #[test]
    fn test_open_to_merged_invalid() {
        let mut mr = make_mr();
        assert!(mr.transition_status(MrStatus::Merged).is_err());
    }

    #[test]
    fn test_spec_ref_field() {
        let mut mr = make_mr();
        assert!(mr.spec_ref.is_none());
        mr.spec_ref = Some("specs/system/agent-gates.md@abc1234".to_string());
        assert!(mr.spec_ref.is_some());
    }
}
