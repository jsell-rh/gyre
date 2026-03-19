use gyre_common::Id;
use serde::{Deserialize, Serialize};

/// A2A Agent Card: describes an agent's identity and capabilities for discovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCard {
    pub agent_id: Id,
    pub name: String,
    pub description: String,
    /// e.g. ["rust-dev", "testing", "review"]
    pub capabilities: Vec<String>,
    /// e.g. ["mcp", "a2a"]
    pub protocols: Vec<String>,
    /// Where to reach this agent (e.g. WebSocket URL)
    pub endpoint: Option<String>,
}

impl AgentCard {
    pub fn new(agent_id: Id, name: impl Into<String>) -> Self {
        Self {
            agent_id,
            name: name.into(),
            description: String::new(),
            capabilities: Vec::new(),
            protocols: Vec::new(),
            endpoint: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_agent_card() {
        let card = AgentCard::new(Id::new("agent-1"), "my-agent");
        assert_eq!(card.name, "my-agent");
        assert!(card.capabilities.is_empty());
        assert!(card.protocols.is_empty());
        assert!(card.endpoint.is_none());
    }

    #[test]
    fn test_agent_card_with_capabilities() {
        let card = AgentCard {
            agent_id: Id::new("agent-2"),
            name: "rust-agent".to_string(),
            description: "Rust developer".to_string(),
            capabilities: vec!["rust-dev".to_string(), "testing".to_string()],
            protocols: vec!["mcp".to_string(), "a2a".to_string()],
            endpoint: Some("ws://localhost:8080".to_string()),
        };
        assert_eq!(card.capabilities.len(), 2);
        assert_eq!(card.protocols.len(), 2);
        assert!(card.endpoint.is_some());
    }

    #[test]
    fn test_agent_card_serialization() {
        let card = AgentCard::new(Id::new("agent-3"), "test");
        let json = serde_json::to_string(&card).unwrap();
        let decoded: AgentCard = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.name, card.name);
    }
}
