use gyre_common::Id;
use crate::budget::BudgetConfig;
use serde::{Deserialize, Serialize};


/// Governance and coordination boundary. Groups related repos with shared budgets and policies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: Id,
    pub tenant_id: Id,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub budget: Option<BudgetConfig>,
    pub max_repos: Option<u32>,
    pub max_agents_per_repo: Option<u32>,
    pub created_at: u64,
}

impl Workspace {
    pub fn new(
        id: Id,
        tenant_id: Id,
        name: impl Into<String>,
        slug: impl Into<String>,
        created_at: u64,
    ) -> Self {
        Self {
            id,
            tenant_id,
            name: name.into(),
            slug: slug.into(),
            description: None,
            budget: None,
            max_repos: None,
            max_agents_per_repo: None,
            created_at,
        }
    }
}

/// Scope of a persona — determines resolution priority.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "id")]
pub enum PersonaScope {
    Tenant(Id),
    Workspace(Id),
    Repo(Id),
}

/// Named agent behavioral definition. Personas define judgment, system prompt, and constraints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Persona {
    pub id: Id,
    pub name: String,
    pub slug: String,
    pub scope: PersonaScope,
    pub system_prompt: String,
    pub capabilities: Vec<String>,
    pub protocols: Vec<String>,
    pub model: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub budget: Option<BudgetConfig>,
    pub created_at: u64,
}

impl Persona {
    pub fn new(
        id: Id,
        name: impl Into<String>,
        slug: impl Into<String>,
        scope: PersonaScope,
        system_prompt: impl Into<String>,
        created_at: u64,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            slug: slug.into(),
            scope,
            system_prompt: system_prompt.into(),
            capabilities: vec![],
            protocols: vec![],
            model: None,
            temperature: None,
            max_tokens: None,
            budget: None,
            created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_new() {
        let ws = Workspace::new(
            Id::new("ws1"),
            Id::new("t1"),
            "My Workspace",
            "my-workspace",
            1000,
        );
        assert_eq!(ws.name, "My Workspace");
        assert_eq!(ws.slug, "my-workspace");
        assert!(ws.description.is_none());
        assert!(ws.budget.is_none());
    }

    #[test]
    fn test_persona_new() {
        let p = Persona::new(
            Id::new("p1"),
            "security",
            "security",
            PersonaScope::Tenant(Id::new("t1")),
            "You are a security reviewer...",
            2000,
        );
        assert_eq!(p.name, "security");
        assert!(p.capabilities.is_empty());
        assert!(p.model.is_none());
    }

    #[test]
    fn test_budget_config_default() {
        let b = BudgetConfig::default();
        assert!(b.max_tokens_per_day.is_none());
        assert!(b.max_cost_per_day.is_none());
    }
}
