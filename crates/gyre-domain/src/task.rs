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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskPriority {
    Low,
    Medium,
    High,
    Critical,
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
        }
    }

    /// Enforce valid status transitions:
    /// Backlog → InProgress
    /// InProgress → Review | Blocked
    /// Review → Done | InProgress
    /// Blocked → InProgress
    /// Done is terminal
    pub fn transition_status(&mut self, new_status: TaskStatus) -> Result<(), TaskError> {
        let valid = matches!(
            (&self.status, &new_status),
            (TaskStatus::Backlog, TaskStatus::InProgress)
                | (TaskStatus::InProgress, TaskStatus::Review)
                | (TaskStatus::InProgress, TaskStatus::Blocked)
                | (TaskStatus::Review, TaskStatus::Done)
                | (TaskStatus::Review, TaskStatus::InProgress)
                | (TaskStatus::Blocked, TaskStatus::InProgress)
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
