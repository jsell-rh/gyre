use gyre_common::Id;
use serde::{Deserialize, Serialize};

/// A product analytics event, e.g. "task.completed", "mr.merged", "agent.spawned".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsEvent {
    pub id: Id,
    pub event_name: String,
    pub agent_id: Option<String>,
    pub properties: serde_json::Value,
    pub timestamp: u64,
}

impl AnalyticsEvent {
    pub fn new(
        id: Id,
        event_name: impl Into<String>,
        agent_id: Option<String>,
        properties: serde_json::Value,
        timestamp: u64,
    ) -> Self {
        Self {
            id,
            event_name: event_name.into(),
            agent_id,
            properties,
            timestamp,
        }
    }
}

/// A cost record for tracking agent/task resource consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEntry {
    pub id: Id,
    pub agent_id: Id,
    pub task_id: Option<Id>,
    /// e.g. "llm_tokens", "compute_minutes"
    pub cost_type: String,
    pub amount: f64,
    /// e.g. "tokens", "minutes", "usd"
    pub currency: String,
    pub timestamp: u64,
}

impl CostEntry {
    pub fn new(
        id: Id,
        agent_id: Id,
        task_id: Option<Id>,
        cost_type: impl Into<String>,
        amount: f64,
        currency: impl Into<String>,
        timestamp: u64,
    ) -> Self {
        Self {
            id,
            agent_id,
            task_id,
            cost_type: cost_type.into(),
            amount,
            currency: currency.into(),
            timestamp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analytics_event_new() {
        let e = AnalyticsEvent::new(
            Id::new("e1"),
            "task.completed",
            Some("agent-1".to_string()),
            serde_json::json!({ "task_id": "t1" }),
            1000,
        );
        assert_eq!(e.event_name, "task.completed");
        assert_eq!(e.agent_id.as_deref(), Some("agent-1"));
        assert_eq!(e.timestamp, 1000);
    }

    #[test]
    fn cost_entry_new() {
        let c = CostEntry::new(
            Id::new("c1"),
            Id::new("a1"),
            Some(Id::new("t1")),
            "llm_tokens",
            1500.0,
            "tokens",
            2000,
        );
        assert_eq!(c.cost_type, "llm_tokens");
        assert_eq!(c.amount, 1500.0);
        assert_eq!(c.currency, "tokens");
    }
}
