use gyre_common::Id;
use serde::{Deserialize, Serialize};

/// An autonomous agent managed by the Gyre platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: Id,
    pub name: String,
    pub status: AgentStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    Idle,
    Active,
    Done,
    Blocked,
}

impl Agent {
    pub fn new(id: Id, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            status: AgentStatus::Idle,
        }
    }
}
