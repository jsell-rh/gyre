use crate::budget::BudgetConfig;
use gyre_common::Id;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Trust level controlling how much autonomy agents have within a workspace.
///
/// Default is `Supervised` (HSI §2): a new workspace starts at Supervised —
/// human reviews everything before merge — and is promoted from there.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum TrustLevel {
    /// Human reviews everything before merge (default — HSI §2).
    #[default]
    Supervised,
    /// Agents merge if gates pass, alert on failures.
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
    /// Parse a stored trust level string. Unknown/legacy values fall back to
    /// `Supervised` (HSI §2 default — safest level on ambiguity).
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "Guided" => TrustLevel::Guided,
            "Autonomous" => TrustLevel::Autonomous,
            "Custom" => TrustLevel::Custom,
            _ => TrustLevel::Supervised,
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
    /// How much autonomy agents have in this workspace (default: Supervised).
    pub trust_level: TrustLevel,
    /// LLM model override for workspace queries (default: GYRE_LLM_MODEL env).
    pub llm_model: Option<String>,
    pub created_at: u64,
    /// Optional compute target for agent spawning within this workspace.
    pub compute_target_id: Option<Id>,
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
            trust_level: TrustLevel::default(),
            llm_model: None,
            created_at,
            compute_target_id: None,
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

/// A built-in persona definition (platform-model.md §2 "Built-In Personas").
///
/// Static data: the slug, prompt, capabilities, and protocols Gyre ships
/// with. `Id` and timestamps are attached at seed time by
/// [`builtin_personas`].
pub struct BuiltinPersonaDef {
    pub name: &'static str,
    pub slug: &'static str,
    /// Persona system prompt — content of the canonical
    /// `specs/personas/<slug>.md` spec file, embedded at compile time.
    pub system_prompt: &'static str,
    pub capabilities: &'static [&'static str],
    pub protocols: &'static [&'static str],
}

/// The four built-in personas (platform-model.md §2 table, exact order):
/// `workspace-orchestrator`, `repo-orchestrator`, `accountability`,
/// `security` — all pre-approved at tenant scope ("ships with Gyre").
///
/// Prompt content is embedded from `specs/personas/` — the same canonical
/// spec files the CLI bootstrap reads — so the shipped prompts are the spec
/// files, byte for byte.
pub const BUILTIN_PERSONA_DEFS: &[BuiltinPersonaDef] = &[
    BuiltinPersonaDef {
        name: "Workspace Orchestrator",
        slug: "workspace-orchestrator",
        system_prompt: include_str!("../../../specs/personas/workspace-orchestrator.md"),
        capabilities: &["task.create", "spec.read", "cross-repo-analysis"],
        protocols: &["mcp", "escalation", "handoff"],
    },
    BuiltinPersonaDef {
        name: "Repo Orchestrator",
        slug: "repo-orchestrator",
        system_prompt: include_str!("../../../specs/personas/repo-orchestrator.md"),
        capabilities: &[
            "task.create",
            "task.decompose",
            "agent.dispatch",
            "merge.queue",
        ],
        protocols: &["mcp", "ralph-loop", "escalation", "handoff"],
    },
    BuiltinPersonaDef {
        name: "Accountability Agent",
        slug: "accountability",
        system_prompt: include_str!("../../../specs/personas/accountability.md"),
        capabilities: &["spec.read", "code.read", "drift.report"],
        protocols: &["mcp", "patrol"],
    },
    BuiltinPersonaDef {
        name: "Security Agent",
        slug: "security",
        system_prompt: include_str!("../../../specs/personas/security.md"),
        capabilities: &["code.read", "threat.report", "dependency.audit"],
        protocols: &["mcp", "patrol", "escalation"],
    },
];

/// Build the set of built-in personas as tenant-scoped `Persona` entities
/// (platform-model.md §2). Each persona gets a fresh `Id` and is
/// pre-approved (`Approved`, approved_by = the system identity) because the
/// spec table marks all four "Pre-approved (ships with Gyre)".
///
/// Callers seed these via `PersonaRepository::create`, keyed by
/// `find_by_slug_and_scope(slug, PersonaScope::Tenant(tenant_id))` for
/// idempotency — an existing persona at that slug+scope is never
/// overwritten (the user may have customized it, and §2 allows
/// workspace/repo-scope overrides).
pub fn builtin_personas(tenant_id: &Id, now: u64) -> Vec<Persona> {
    BUILTIN_PERSONA_DEFS
        .iter()
        .map(|def| {
            let mut p = Persona::new(
                Id::new(uuid::Uuid::new_v4().to_string()),
                def.name,
                def.slug,
                PersonaScope::Tenant(tenant_id.clone()),
                def.system_prompt,
                now,
            );
            p.capabilities = def.capabilities.iter().map(|s| s.to_string()).collect();
            p.protocols = def.protocols.iter().map(|s| s.to_string()).collect();
            p.refresh_content_hash();
            p.owner = Some("system".to_string());
            p.approval_status = PersonaApprovalStatus::Approved;
            p.approved_by = Some("system".to_string());
            p.approved_at = Some(now);
            p
        })
        .collect()
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
        assert_eq!(ws.trust_level, TrustLevel::Supervised); // HSI §2 default
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

    /// platform-model.md §2 table: exactly these four personas, in order,
    /// each with the spec-file prompt embedded.
    #[test]
    fn builtin_persona_defs_cover_spec_table() {
        let slugs: Vec<&str> = BUILTIN_PERSONA_DEFS.iter().map(|p| p.slug).collect();
        assert_eq!(
            slugs,
            vec![
                "workspace-orchestrator",
                "repo-orchestrator",
                "accountability",
                "security"
            ]
        );
        for def in BUILTIN_PERSONA_DEFS {
            // Each prompt is the embedded specs/personas/<slug>.md content —
            // a missing or truncated file fails here.
            assert!(
                def.system_prompt.len() > 500,
                "persona {} prompt too short: {} bytes",
                def.slug,
                def.system_prompt.len()
            );
            assert!(
                def.system_prompt.starts_with('#'),
                "persona {} prompt must be the markdown spec file",
                def.slug
            );
            assert!(
                !def.capabilities.is_empty(),
                "persona {} needs capabilities",
                def.slug
            );
            assert!(
                !def.protocols.is_empty(),
                "persona {} needs protocols",
                def.slug
            );
        }
    }

    /// §2 "Pre-approved (ships with Gyre)": all four built-ins are Approved
    /// at Tenant scope with the spec-file prompt and a real content hash.
    #[test]
    fn builtin_personas_are_pre_approved_tenant_scoped() {
        let tenant = Id::new("t1");
        let personas = builtin_personas(&tenant, 1700_000_000);
        assert_eq!(personas.len(), 4);
        for (p, def) in personas.iter().zip(BUILTIN_PERSONA_DEFS) {
            assert_eq!(p.slug, def.slug);
            assert_eq!(p.scope, PersonaScope::Tenant(tenant.clone()));
            assert_eq!(p.system_prompt, def.system_prompt);
            assert_eq!(p.approval_status, PersonaApprovalStatus::Approved);
            assert_eq!(p.approved_by.as_deref(), Some("system"));
            assert_eq!(p.approved_at, Some(1700_000_000));
            assert_eq!(p.version, 1);
            // content_hash is SHA-256 of system_prompt + capabilities —
            // recomputing independently must match.
            let expected = format!(
                "{:x}",
                Sha256::digest(
                    format!("{}{}", p.system_prompt, p.capabilities.join(",")).as_bytes()
                )
            );
            assert_eq!(p.content_hash, expected);
        }
        // Distinct ids — four entities, not one reused four times.
        let ids: std::collections::HashSet<_> = personas.iter().map(|p| p.id.clone()).collect();
        assert_eq!(ids.len(), 4);
    }

    #[test]
    fn test_budget_config_default() {
        let b = BudgetConfig::default();
        assert!(b.max_tokens_per_day.is_none());
        assert!(b.max_cost_per_day.is_none());
    }
}
