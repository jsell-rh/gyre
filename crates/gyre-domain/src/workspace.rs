use crate::budget::BudgetConfig;
use gyre_common::Id;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Trust level controlling how much autonomy agents have within a workspace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum TrustLevel {
    /// Human reviews everything before merge.
    Supervised,
    /// Agents merge if gates pass, alert on failures (default).
    #[default]
    Guided,
    /// Only interrupt for exceptions.
    Autonomous,
    /// Direct ABAC policy manipulation.
    Custom,
}

impl std::fmt::Display for TrustLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrustLevel::Supervised => write!(f, "Supervised"),
            TrustLevel::Guided => write!(f, "Guided"),
            TrustLevel::Autonomous => write!(f, "Autonomous"),
            TrustLevel::Custom => write!(f, "Custom"),
        }
    }
}

impl TrustLevel {
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "Supervised" => TrustLevel::Supervised,
            "Autonomous" => TrustLevel::Autonomous,
            "Custom" => TrustLevel::Custom,
            _ => TrustLevel::Guided,
        }
    }
}

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
    /// How much autonomy agents have in this workspace (default: Guided).
    pub trust_level: TrustLevel,
    /// LLM model override for workspace queries (default: GYRE_LLM_MODEL env).
    pub llm_model: Option<String>,
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
            trust_level: TrustLevel::Guided,
            llm_model: None,
            created_at,
        }
    }
}

/// Approval lifecycle for a persona definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum PersonaApprovalStatus {
    #[default]
    Pending,
    Approved,
    Deprecated,
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
    /// Increments on each update.
    pub version: u32,
    /// SHA-256 of system_prompt + capabilities joined.
    pub content_hash: String,
    /// Owner identity (user or agent id).
    pub owner: Option<String>,
    pub approval_status: PersonaApprovalStatus,
    pub approved_by: Option<String>,
    pub approved_at: Option<u64>,
    pub updated_at: u64,
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
        let system_prompt = system_prompt.into();
        let content_hash = Self::hash_content(&system_prompt, &[]);
        Self {
            id,
            name: name.into(),
            slug: slug.into(),
            scope,
            system_prompt,
            capabilities: vec![],
            protocols: vec![],
            model: None,
            temperature: None,
            max_tokens: None,
            budget: None,
            created_at,
            version: 1,
            content_hash,
            owner: None,
            approval_status: PersonaApprovalStatus::Pending,
            approved_by: None,
            approved_at: None,
            updated_at: created_at,
        }
    }

    /// Recompute and store the content hash from current system_prompt + capabilities.
    pub fn refresh_content_hash(&mut self) {
        self.content_hash = Self::hash_content(&self.system_prompt, &self.capabilities);
    }

    fn hash_content(system_prompt: &str, capabilities: &[String]) -> String {
        let input = format!("{}{}", system_prompt, capabilities.join(","));
        format!("{:x}", Sha256::digest(input.as_bytes()))
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
        assert_eq!(ws.trust_level, TrustLevel::Guided);
        assert!(ws.llm_model.is_none());
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
