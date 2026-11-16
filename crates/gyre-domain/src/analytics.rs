use gyre_common::Id;
use serde::{Deserialize, Serialize};

/// A product analytics event, e.g. "task.completed", "mr.merged", "agent.spawned".
///
/// Scope fields (`user_id`, `session_id`, `workspace_id`, `repo_id`) follow the
/// spec schema (analytics.md §Event Schema) and enable per-scope filtering in
/// the query API. They are optional: platform-internal events may lack a
/// human user or session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsEvent {
    pub id: Id,
    pub event_name: String,
    pub agent_id: Option<String>,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repo_id: Option<String>,
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
            user_id: None,
            session_id: None,
            workspace_id: None,
            repo_id: None,
            properties,
            timestamp,
        }
    }

    /// Attach scope identifiers to an event (builder-style).
    /// `Id` inputs are stringified; analytics stores ids as text.
    pub fn with_scope(
        mut self,
        user_id: Option<&Id>,
        session_id: Option<String>,
        workspace_id: Option<&Id>,
        repo_id: Option<&Id>,
    ) -> Self {
        self.user_id = user_id.map(|id| id.to_string());
        self.session_id = session_id;
        self.workspace_id = workspace_id.map(|id| id.to_string());
        self.repo_id = repo_id.map(|id| id.to_string());
        self
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

    /// analytics.md §Event Schema: the struct must carry every spec field —
    /// id, event_name, agent_id, user_id, session_id, workspace_id, repo_id,
    /// properties, timestamp — and with_scope() must populate the scope ids.
    #[test]
    fn analytics_event_matches_spec_schema() {
        let e = AnalyticsEvent::new(
            Id::new("e-spec"),
            "mr.merged",
            Some("agent-1".to_string()),
            serde_json::json!({ "mr_id": "mr-1" }),
            1000,
        )
        .with_scope(
            Some(&Id::new("user-7")),
            Some("session-42".to_string()),
            Some(&Id::new("ws-3")),
            Some(&Id::new("repo-9")),
        );

        assert_eq!(e.id, Id::new("e-spec"));
        assert_eq!(e.event_name, "mr.merged");
        assert_eq!(e.agent_id.as_deref(), Some("agent-1"));
        assert_eq!(e.user_id.as_deref(), Some("user-7"));
        assert_eq!(e.session_id.as_deref(), Some("session-42"));
        assert_eq!(e.workspace_id.as_deref(), Some("ws-3"));
        assert_eq!(e.repo_id.as_deref(), Some("repo-9"));
        assert_eq!(e.properties["mr_id"], "mr-1");
        assert_eq!(e.timestamp, 1000);

        // Round-trips through serde with all fields intact (storage layer
        // serializes events as JSON).
        let json = serde_json::to_value(&e).unwrap();
        let back: AnalyticsEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back.user_id, e.user_id);
        assert_eq!(back.session_id, e.session_id);
        assert_eq!(back.workspace_id, e.workspace_id);
        assert_eq!(back.repo_id, e.repo_id);
    }
}
