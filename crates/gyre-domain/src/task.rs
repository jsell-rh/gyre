use gyre_common::Id;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TaskError {
    #[error("invalid status transition from {from:?} to {to:?}")]
    InvalidTransition { from: TaskStatus, to: TaskStatus },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Backlog,
    InProgress,
    Review,
    Done,
    Blocked,
    /// Task cancelled because a spec was rejected or it's no longer needed. Terminal.
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskPriority {
    Low,
    Medium,
    High,
    Critical,
}

/// Discriminates task purpose in the spec→orchestrator→task→agent signal chain.
///
/// - `Implementation`: triggers mechanical agent spawning (worker agents).
/// - `Delegation`: triggers repo orchestrator spawning for spec decomposition.
/// - `Coordination`: triggers repo orchestrator for cross-repo dependency changes.
///
/// Tasks without a `task_type` (e.g. push-hook spec-lifecycle tasks) do NOT
/// trigger agent spawning — they are informational until spec approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskType {
    Implementation,
    Delegation,
    Coordination,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Id,
    pub title: String,
    pub description: Option<String>,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    pub assigned_to: Option<Id>,
    pub parent_task_id: Option<Id>,
    pub labels: Vec<String>,
    pub branch: Option<String>,
    pub pr_link: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    /// Workspace that governs this task (ABAC boundary). Non-optional per M34 hierarchy enforcement.
    pub workspace_id: Id,
    /// Repo that owns the spec this task implements. Non-optional per M34 hierarchy enforcement.
    pub repo_id: Id,
    /// Spec path this task was created to implement/review (e.g. "specs/system/agent-gates.md").
    pub spec_path: Option<String>,
    /// Unix timestamp when this task was cancelled. Set when status → Cancelled.
    pub cancelled_at: Option<u64>,
    /// Human-readable reason for cancellation.
    pub cancelled_reason: Option<String>,
    /// Task type discriminator for the signal chain. None for pre-approval push-hook tasks.
    pub task_type: Option<TaskType>,
    /// Execution priority (lower = first). Tasks with the same order can run in parallel.
    pub order: Option<u32>,
    /// Task IDs that must complete before this task starts.
    pub depends_on: Vec<Id>,
}

impl Task {
    pub fn new(id: Id, title: impl Into<String>, created_at: u64) -> Self {
        Self {
            id,
            title: title.into(),
            description: None,
            status: TaskStatus::Backlog,
            priority: TaskPriority::Medium,
            assigned_to: None,
            parent_task_id: None,
            labels: Vec::new(),
            branch: None,
            pr_link: None,
            created_at,
            updated_at: created_at,
            workspace_id: Id::new("default"),
            repo_id: Id::new(""),
            spec_path: None,
            cancelled_at: None,
            cancelled_reason: None,
            task_type: None,
            order: None,
            depends_on: Vec::new(),
        }
    }

    /// Cancel this task with an optional reason. Transitions status to Cancelled.
    pub fn cancel(&mut self, reason: Option<String>, now: u64) -> Result<(), TaskError> {
        self.transition_status(TaskStatus::Cancelled)?;
        self.cancelled_at = Some(now);
        self.cancelled_reason = reason;
        self.updated_at = now;
        Ok(())
    }

    /// Enforce valid status transitions:
    /// Backlog → InProgress | Cancelled
    /// InProgress → Review | Blocked | Cancelled
    /// Review → Done | InProgress | Cancelled
    /// Blocked → InProgress | Cancelled
    /// Done and Cancelled are terminal
    pub fn transition_status(&mut self, new_status: TaskStatus) -> Result<(), TaskError> {
        let valid = matches!(
            (&self.status, &new_status),
            (TaskStatus::Backlog, TaskStatus::InProgress)
                | (TaskStatus::Backlog, TaskStatus::Cancelled)
                | (TaskStatus::InProgress, TaskStatus::Review)
                | (TaskStatus::InProgress, TaskStatus::Blocked)
                | (TaskStatus::InProgress, TaskStatus::Cancelled)
                | (TaskStatus::Review, TaskStatus::Done)
                | (TaskStatus::Review, TaskStatus::InProgress)
                | (TaskStatus::Review, TaskStatus::Cancelled)
                | (TaskStatus::Blocked, TaskStatus::InProgress)
                | (TaskStatus::Blocked, TaskStatus::Cancelled)
        );
        if valid {
            self.status = new_status;
            Ok(())
        } else {
            Err(TaskError::InvalidTransition {
                from: self.status.clone(),
                to: new_status,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_task() -> Task {
        Task::new(Id::new("t1"), "Test Task", 1000)
    }

    #[test]
    fn test_new_task_in_backlog() {
        let task = make_task();
        assert_eq!(task.status, TaskStatus::Backlog);
        assert_eq!(task.priority, TaskPriority::Medium);
    }

    #[test]
    fn test_backlog_to_in_progress() {
        let mut task = make_task();
        assert!(task.transition_status(TaskStatus::InProgress).is_ok());
        assert_eq!(task.status, TaskStatus::InProgress);
    }

    #[test]
    fn test_in_progress_to_review() {
        let mut task = make_task();
        task.transition_status(TaskStatus::InProgress).unwrap();
        assert!(task.transition_status(TaskStatus::Review).is_ok());
    }

    #[test]
    fn test_in_progress_to_blocked() {
        let mut task = make_task();
        task.transition_status(TaskStatus::InProgress).unwrap();
        assert!(task.transition_status(TaskStatus::Blocked).is_ok());
    }

    #[test]
    fn test_review_to_done() {
        let mut task = make_task();
        task.transition_status(TaskStatus::InProgress).unwrap();
        task.transition_status(TaskStatus::Review).unwrap();
        assert!(task.transition_status(TaskStatus::Done).is_ok());
    }

    #[test]
    fn test_review_to_in_progress() {
        let mut task = make_task();
        task.transition_status(TaskStatus::InProgress).unwrap();
        task.transition_status(TaskStatus::Review).unwrap();
        assert!(task.transition_status(TaskStatus::InProgress).is_ok());
    }

    #[test]
    fn test_blocked_to_in_progress() {
        let mut task = make_task();
        task.transition_status(TaskStatus::InProgress).unwrap();
        task.transition_status(TaskStatus::Blocked).unwrap();
        assert!(task.transition_status(TaskStatus::InProgress).is_ok());
    }

    #[test]
    fn test_done_is_terminal() {
        let mut task = make_task();
        task.transition_status(TaskStatus::InProgress).unwrap();
        task.transition_status(TaskStatus::Review).unwrap();
        task.transition_status(TaskStatus::Done).unwrap();
        assert!(task.transition_status(TaskStatus::InProgress).is_err());
    }

    #[test]
    fn test_backlog_to_done_invalid() {
        let mut task = make_task();
        assert!(task.transition_status(TaskStatus::Done).is_err());
    }

    #[test]
    fn test_backlog_to_review_invalid() {
        let mut task = make_task();
        assert!(task.transition_status(TaskStatus::Review).is_err());
    }
}
