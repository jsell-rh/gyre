use gyre_common::Id;
use serde::{Deserialize, Serialize};

/// A unit of work assigned to an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Id,
    pub title: String,
    pub status: TaskStatus,
    pub assigned_to: Option<Id>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Backlog,
    InProgress,
    Review,
    Done,
    Blocked,
}

impl Task {
    pub fn new(id: Id, title: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            status: TaskStatus::Backlog,
            assigned_to: None,
        }
    }
}
