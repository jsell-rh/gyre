use serde::{Deserialize, Serialize};

/// Domain events broadcast over the event bus to all connected WebSocket clients.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum DomainEvent {
    AgentCreated { id: String },
    AgentStatusChanged { id: String, status: String },
    TaskCreated { id: String },
    TaskTransitioned { id: String, status: String },
    MrCreated { id: String },
    MrStatusChanged { id: String, status: String },
    ActivityRecorded { id: String, event_type: String },
    QueueUpdated,
    DataSeeded,
}
