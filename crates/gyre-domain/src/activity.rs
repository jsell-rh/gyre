use gyre_common::Id;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEvent {
    pub id: Id,
    pub agent_id: String,
    pub event_type: String,
    pub description: String,
    pub timestamp: u64,
}

impl ActivityEvent {
    pub fn new(
        id: Id,
        agent_id: impl Into<String>,
        event_type: impl Into<String>,
        description: impl Into<String>,
        timestamp: u64,
    ) -> Self {
        Self {
            id,
            agent_id: agent_id.into(),
            event_type: event_type.into(),
            description: description.into(),
            timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_activity_event() {
        let ev = ActivityEvent::new(
            Id::new("ev1"),
            "agent-a",
            "task_started",
            "Agent started task",
            1000,
        );
        assert_eq!(ev.agent_id, "agent-a");
        assert_eq!(ev.event_type, "task_started");
        assert_eq!(ev.description, "Agent started task");
        assert_eq!(ev.timestamp, 1000);
    }
}
