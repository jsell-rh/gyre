use crate::budget::BudgetConfig;
use gyre_common::Id;
use gyre_common::NotificationType;
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
    /// Strictly parse a caller-supplied trust level string (API input).
    ///
    /// Unlike [`from_db_str`], unknown strings are NOT coerced to
    /// `Supervised` — a typo like `"Autonomus"` must be rejected by the
    /// caller (400), not silently transition the workspace to Supervised
    /// and rewrite its trust policies. Mirrors
    /// `ComputeTargetType::from_db_str`'s Option contract.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Supervised" => Some(TrustLevel::Supervised),
            "Guided" => Some(TrustLevel::Guided),
            "Autonomous" => Some(TrustLevel::Autonomous),
            "Custom" => Some(TrustLevel::Custom),
            _ => None,
        }
    }

    /// Parse a stored trust level string. Unknown/legacy values fall back to
    /// `Supervised` (HSI §2 default — safest level on ambiguity).
    pub fn from_db_str(s: &str) -> Self {
        Self::parse(s).unwrap_or_default()
    }

    /// HSI §2 "What Each Level Controls" > Notifications row: whether a
    /// notification of this type is delivered to the inbox at this trust
    /// level.
    ///
    /// - **Supervised / Custom** deliver every state change (Custom's
    ///   differentiation is in the ABAC policies, not the inbox filter —
    ///   HSI §2 "Custom trust Inbox").
    /// - **Guided** delivers "failures and approvals": failures the agent
    ///   owns, every approval-flow request, and the exception set below.
    /// - **Autonomous** delivers "exceptions only": events that require
    ///   human action, record a security/consent event, or gate an
    ///   approval flow — plus the explicit budget carve-out (HSI §2
    ///   Mechanical Implementation: "Budget warnings … remain visible at
    ///   Autonomous trust because `platform-model.md` §5 defines budget
    ///   exhaustion as requiring human action").
    ///
    /// Failures are suppressed at Autonomous because the agent owns
    /// remediation there; an unresolvable failure escalates
    /// (`AgentEscalation` / `AgentNeedsClarification`), which IS delivered.
    pub fn delivers_notification(&self, notification_type: &NotificationType) -> bool {
        match self {
            TrustLevel::Supervised | TrustLevel::Custom => true,
            TrustLevel::Guided => !GUIDED_SUPPRESSED.contains(notification_type),
            TrustLevel::Autonomous => !AUTONOMOUS_SUPPRESSED.contains(notification_type),
        }
    }

    /// Canonical type strings this trust level suppresses (empty at
    /// Supervised/Custom). Used to build the inbox `exclude_types` filter —
    /// exact-match strings on the persisted `notification_type` column.
    pub fn suppressed_notification_types(&self) -> Vec<&'static str> {
        let suppressed: &[NotificationType] = match self {
            TrustLevel::Supervised | TrustLevel::Custom => &[],
            TrustLevel::Guided => GUIDED_SUPPRESSED,
            TrustLevel::Autonomous => AUTONOMOUS_SUPPRESSED,
        };
        suppressed.iter().map(|t| t.as_str()).collect()
    }

    /// HSI §2 "Inbox priorities shown" row: canonical type strings the
    /// inbox RANGE excludes at this trust level (empty at Supervised/Custom
    /// — 1-10, everything).
    ///
    /// The HSI §8 priority table lists exactly one rank-10 type
    /// (`SuggestedSpecLink` — "excludes suggested links"). The unified
    /// notification taxonomy (user-management.md §Notification Entity)
    /// later placed `InvitationReceived`/`MembershipChanged` at rank 10
    /// too; a blanket `max_priority=9` rank cap would therefore also hide
    /// invitations — action-required events the volume row deliberately
    /// delivers at every level. The range is consequently enforced as a
    /// TYPE-SET exclusion: rank>10-cap types that are not always-delivered
    /// (i.e. not delivered at Autonomous, the most restrictive level). With
    /// the current taxonomy this is exactly `SuggestedSpecLink`, matching
    /// the HSI table's parenthetical.
    pub fn inbox_range_excluded_types(&self) -> Vec<&'static str> {
        match self {
            TrustLevel::Supervised | TrustLevel::Custom => Vec::new(),
            TrustLevel::Guided | TrustLevel::Autonomous => NotificationType::all()
                .iter()
                .filter(|t| t.default_priority_rank() > 9)
                .filter(|t| !TrustLevel::Autonomous.delivers_notification(t))
                .map(|t| t.as_str())
                .collect(),
        }
    }

    /// Combined inbox exclusion set at this trust level: the volume row's
    /// suppressed types plus the range row's excluded types, de-duplicated.
    /// This is what the inbox listing AND the badge counts pass as
    /// `exclude_types`, so the two can never disagree.
    pub fn inbox_excluded_types(&self) -> Vec<&'static str> {
        let mut excluded = self.suppressed_notification_types();
        for t in self.inbox_range_excluded_types() {
            if !excluded.contains(&t) {
                excluded.push(t);
            }
        }
        excluded
    }
}

/// State-change chatter suppressed at Guided ("alert me on failures") —
/// neither a failure, an approval, nor an exception.
///
/// - `MrMerged` / `GatePassed` / `AgentCompleted`: successful state changes.
/// - `SuggestedSpecLink` (rank 10): knowledge-graph suggestions.
/// - `TrustSuggestion`: the trust system's own suggestions (the operator
///   already chose this level).
/// - `CrossWorkspaceSpecChange` / `MetaSpecDrift` / `AbandonedBranch` /
///   `CascadeTestTriggered` / `DependencyChainTooDeep`: informational
///   activity, not failures.
const GUIDED_SUPPRESSED: &[NotificationType] = &[
    NotificationType::MrMerged,
    NotificationType::GatePassed,
    NotificationType::AgentCompleted,
    NotificationType::SuggestedSpecLink,
    NotificationType::TrustSuggestion,
    NotificationType::CrossWorkspaceSpecChange,
    NotificationType::MetaSpecDrift,
    NotificationType::AbandonedBranch,
    NotificationType::CascadeTestTriggered,
    NotificationType::DependencyChainTooDeep,
];

/// Everything Autonomous suppresses: the Guided set PLUS the failures the
/// agent owns remediation for ("only interrupt me for exceptions"). An
/// unresolvable failure escalates via `AgentEscalation`, which stays
/// delivered. Delivered at Autonomous: `AgentNeedsClarification`,
/// `AgentEscalation`, `AgentBudgetWarning` (explicit HSI §2 carve-out),
/// `AgentBudgetExhausted`, `SecurityFinding`, `MergeQueueEscalation`,
/// `MergeQueuePaused` (broken main), `ComputeTargetUnavailable`, all
/// approval-flow requests (`SpecApprovalRequested`,
/// `PersonaApprovalRequested`, `PersonaChangePendingApproval`),
/// `SpecConflict`, `ImpersonationRequest`/`ImpersonationAlert` (consent),
/// `WorkspaceOwnerless`, `SecretExpiring`, `InvitationReceived`,
/// `MembershipChanged`, `SystemAlert`.
const AUTONOMOUS_SUPPRESSED: &[NotificationType] = &[
    // Guided-suppressed state-change chatter
    NotificationType::MrMerged,
    NotificationType::GatePassed,
    NotificationType::AgentCompleted,
    NotificationType::SuggestedSpecLink,
    NotificationType::TrustSuggestion,
    NotificationType::CrossWorkspaceSpecChange,
    NotificationType::MetaSpecDrift,
    NotificationType::AbandonedBranch,
    NotificationType::CascadeTestTriggered,
    NotificationType::DependencyChainTooDeep,
    // Failures the agent owns at Autonomous
    NotificationType::GateFailure,
    NotificationType::AgentFailed,
    NotificationType::CascadeTestFailed,
    NotificationType::SpecAssertionFailure,
    NotificationType::ConstraintViolation,
    NotificationType::AtomicGroupFailure,
    NotificationType::MrReverted,
    NotificationType::BreakingChangeDetected,
    NotificationType::SpecRejected,
    NotificationType::SpecDriftDetected,
    NotificationType::DependencyStale,
];

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
        let personas = builtin_personas(&tenant, 1_700_000_000);
        assert_eq!(personas.len(), 4);
        for (p, def) in personas.iter().zip(BUILTIN_PERSONA_DEFS) {
            assert_eq!(p.slug, def.slug);
            assert_eq!(p.scope, PersonaScope::Tenant(tenant.clone()));
            assert_eq!(p.system_prompt, def.system_prompt);
            assert_eq!(p.approval_status, PersonaApprovalStatus::Approved);
            assert_eq!(p.approved_by.as_deref(), Some("system"));
            assert_eq!(p.approved_at, Some(1_700_000_000));
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

    // ── TASK-077 (F8): TrustLevel::from_db_str round-trips and fallback ──

    #[test]
    fn from_db_str_parses_all_four_levels() {
        assert_eq!(
            TrustLevel::from_db_str("Supervised"),
            TrustLevel::Supervised
        );
        assert_eq!(TrustLevel::from_db_str("Guided"), TrustLevel::Guided);
        assert_eq!(
            TrustLevel::from_db_str("Autonomous"),
            TrustLevel::Autonomous
        );
        assert_eq!(TrustLevel::from_db_str("Custom"), TrustLevel::Custom);
    }

    #[test]
    fn from_db_str_unknown_falls_back_to_supervised() {
        // Unknown/legacy values must fall back to Supervised (HSI §2 —
        // safest level on ambiguity).
        assert_eq!(TrustLevel::from_db_str("bogus"), TrustLevel::Supervised);
        assert_eq!(TrustLevel::from_db_str(""), TrustLevel::Supervised);
        // Case-sensitive: "supervised" is not a stored value.
        assert_eq!(
            TrustLevel::from_db_str("supervised"),
            TrustLevel::Supervised
        );
    }

    // ── TASK-077 revision: strict parse for caller-supplied API input ──

    #[test]
    fn parse_accepts_exactly_the_four_spec_levels() {
        assert_eq!(
            TrustLevel::parse("Supervised"),
            Some(TrustLevel::Supervised)
        );
        assert_eq!(TrustLevel::parse("Guided"), Some(TrustLevel::Guided));
        assert_eq!(
            TrustLevel::parse("Autonomous"),
            Some(TrustLevel::Autonomous)
        );
        assert_eq!(TrustLevel::parse("Custom"), Some(TrustLevel::Custom));
    }

    #[test]
    fn parse_rejects_typos_case_and_empty() {
        // Typos, wrong case, and empty must NOT coerce to a level — the
        // API layer 400s on `None` instead of silently transitioning.
        assert_eq!(TrustLevel::parse("Autonomus"), None);
        assert_eq!(TrustLevel::parse("autonomous"), None);
        assert_eq!(TrustLevel::parse(""), None);
    }

    // ── HSI §2 "Notifications" / "Inbox priorities shown" rows ──

    #[test]
    fn supervised_and_custom_deliver_every_notification_type() {
        // "Every state change" at Supervised; Custom's differentiation is
        // in the ABAC policies, not the inbox filter (HSI §2).
        for t in NotificationType::all() {
            assert!(
                TrustLevel::Supervised.delivers_notification(&t),
                "Supervised must deliver {}",
                t.as_str()
            );
            assert!(
                TrustLevel::Custom.delivers_notification(&t),
                "Custom must deliver {}",
                t.as_str()
            );
        }
        assert!(TrustLevel::Supervised
            .suppressed_notification_types()
            .is_empty());
        assert!(TrustLevel::Custom
            .suppressed_notification_types()
            .is_empty());
        assert!(TrustLevel::Supervised.inbox_excluded_types().is_empty());
        assert!(TrustLevel::Custom.inbox_excluded_types().is_empty());
    }

    #[test]
    fn guided_suppresses_state_change_chatter_but_delivers_failures_and_approvals() {
        use NotificationType::*;
        let guided = TrustLevel::Guided;
        // "Failures and approvals": failures deliver.
        for failure in [
            GateFailure,
            AgentFailed,
            CascadeTestFailed,
            SpecAssertionFailure,
            ConstraintViolation,
            AtomicGroupFailure,
        ] {
            assert!(
                guided.delivers_notification(&failure),
                "{}",
                failure.as_str()
            );
        }
        // Approvals deliver (spec approval is always human).
        assert!(guided.delivers_notification(&SpecApprovalRequested));
        assert!(guided.delivers_notification(&PersonaApprovalRequested));
        // Exceptions deliver.
        assert!(guided.delivers_notification(&AgentEscalation));
        assert!(guided.delivers_notification(&AgentNeedsClarification));
        assert!(guided.delivers_notification(&SecurityFinding));
        // Successful state changes are suppressed.
        for chatter in [
            MrMerged,
            GatePassed,
            AgentCompleted,
            SuggestedSpecLink,
            TrustSuggestion,
        ] {
            assert!(
                !guided.delivers_notification(&chatter),
                "Guided must suppress {}",
                chatter.as_str()
            );
        }
        // "Inbox priorities shown: 1-9 (excludes suggested links)" — the
        // suggested link is excluded from the Guided inbox.
        assert!(guided.inbox_excluded_types().contains(&"SuggestedSpecLink"));
    }

    #[test]
    fn autonomous_delivers_exceptions_only() {
        use NotificationType::*;
        let autonomous = TrustLevel::Autonomous;
        // Exceptions: human action required now.
        for exception in [
            AgentNeedsClarification,
            AgentEscalation,
            AgentBudgetExhausted,
            SecurityFinding,
            MergeQueueEscalation,
            MergeQueuePaused,
            ComputeTargetUnavailable,
            WorkspaceOwnerless,
            SecretExpiring,
            ImpersonationRequest,
            ImpersonationAlert,
        ] {
            assert!(
                autonomous.delivers_notification(&exception),
                "{}",
                exception.as_str()
            );
        }
        // Explicit HSI §2 carve-out: budget warnings remain visible at
        // Autonomous (platform-model.md §5 — budget exhaustion requires
        // human action).
        assert!(autonomous.delivers_notification(&AgentBudgetWarning));
        // Spec approval is always human — the approval REQUEST must
        // reach a human at every trust level, else approval stalls.
        assert!(autonomous.delivers_notification(&SpecApprovalRequested));
        assert!(autonomous.delivers_notification(&PersonaApprovalRequested));
        assert!(autonomous.delivers_notification(&PersonaChangePendingApproval));
        // Failures the agent owns are suppressed — escalation covers the
        // human-needed cases.
        for failure in [
            GateFailure,
            AgentFailed,
            CascadeTestFailed,
            SpecAssertionFailure,
            ConstraintViolation,
            MrMerged,
            GatePassed,
            AgentCompleted,
            SuggestedSpecLink,
        ] {
            assert!(
                !autonomous.delivers_notification(&failure),
                "Autonomous must suppress {}",
                failure.as_str()
            );
        }
        // "Inbox priorities shown: 1-9 (excludes suggested links)".
        assert!(autonomous
            .inbox_excluded_types()
            .contains(&"SuggestedSpecLink"));
    }

    #[test]
    fn suppressed_type_strings_round_trip_through_parse() {
        // The suppressed sets feed the inbox exclude_types filter as
        // exact-match strings on the persisted notification_type column —
        // every string must parse back to the variant it names.
        for level in [TrustLevel::Guided, TrustLevel::Autonomous] {
            for s in level.suppressed_notification_types() {
                assert_eq!(
                    NotificationType::parse(s)
                        .as_ref()
                        .map(NotificationType::as_str),
                    Some(s),
                    "suppressed type string {s} must round-trip"
                );
            }
        }
    }

    #[test]
    fn inbox_range_excludes_rank_ten_chatter_but_not_action_required_types() {
        // The unified taxonomy ranks InvitationReceived/MembershipChanged
        // at 10 alongside SuggestedSpecLink. The range row's stated intent
        // ("excludes suggested links") must NOT hide action-required
        // invitations: only rank>9 types that are not always-delivered are
        // range-excluded.
        for level in [TrustLevel::Guided, TrustLevel::Autonomous] {
            let excluded = level.inbox_range_excluded_types();
            assert!(
                excluded.contains(&"SuggestedSpecLink"),
                "suggested links must be range-excluded at {level:?}"
            );
            assert!(
                !excluded.contains(&"InvitationReceived"),
                "invitations are action-required and must stay visible at {level:?}"
            );
            assert!(
                !excluded.contains(&"MembershipChanged"),
                "membership changes concern the user's own access and must stay visible at {level:?}"
            );
            // Every range-excluded type is also in the combined inbox
            // exclusion set (list and badge share it).
            let combined = level.inbox_excluded_types();
            for t in excluded {
                assert!(combined.contains(&t));
            }
        }
    }
}
