//! ABAC (Attribute-Based Access Control) policy domain types.
//!
//! Policies evaluate subject/resource/action/environment attributes to produce
//! an Allow or Deny decision. Policies are scoped (Tenant > Workspace > Repo),
//! prioritised (higher number = evaluated first), and composable (first match wins).
//!
//! **Immutable Deny policies** are a special class: they are evaluated BEFORE all
//! priority-based evaluation and cannot be overridden by any Allow policy regardless
//! of priority. See `human-system-interface.md` §2 and `abac-policy-engine.md`.

use gyre_common::Id;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Policy entity
// ---------------------------------------------------------------------------

/// Scope at which a policy applies.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PolicyScope {
    Tenant,
    Workspace,
    Repo,
}

/// Effect when a policy matches: allow or deny the request.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PolicyEffect {
    Allow,
    Deny,
}

/// Comparison operator for a `Condition`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConditionOp {
    Equals,
    NotEquals,
    In,
    NotIn,
    GreaterThan,
    LessThan,
    Contains,
    Exists,
}

/// The value side of a condition.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ConditionValue {
    StringList(Vec<String>),
    String(String),
    Number(i64),
    Bool(bool),
    Null,
}

/// A single attribute match condition. All conditions in a policy are ANDed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Condition {
    /// Dot-separated attribute path, e.g. `"subject.workspace_role"`.
    pub attribute: String,
    pub operator: ConditionOp,
    pub value: ConditionValue,
}

/// A declarative access control policy evaluated against request attributes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Policy {
    pub id: Id,
    pub name: String,
    pub description: String,
    /// Scope: Tenant, Workspace, or Repo.
    pub scope: PolicyScope,
    /// ID of the scoped entity (tenant_id, workspace_id, or repo_id).
    /// `None` means the policy applies to ALL entities at this scope level.
    pub scope_id: Option<String>,
    /// Higher number = evaluated first. Ties broken by scope specificity.
    pub priority: u32,
    /// Effect when all conditions match.
    pub effect: PolicyEffect,
    /// All conditions must match (AND logic). Empty = always matches.
    pub conditions: Vec<Condition>,
    /// Action names this policy applies to. `["*"]` = all actions.
    pub actions: Vec<String>,
    /// Resource types this policy applies to. `["*"]` = all resource types.
    pub resource_types: Vec<String>,
    pub enabled: bool,
    /// True for built-in system policies that cannot be deleted.
    pub built_in: bool,
    /// Immutable Deny policies are evaluated before all priority-based evaluation
    /// and cannot be overridden by any Allow regardless of priority.
    /// Only meaningful when `effect == Deny`. See HSI §2.
    pub immutable: bool,
    pub created_by: String,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Policy {
    /// Returns `true` if this policy applies to the given action and resource type.
    pub fn applies_to(&self, action: &str, resource_type: &str) -> bool {
        let action_match = self.actions.iter().any(|a| a == "*" || a == action);
        let resource_match = self
            .resource_types
            .iter()
            .any(|r| r == "*" || r == resource_type);
        action_match && resource_match
    }
}

// ---------------------------------------------------------------------------
// Audit record
// ---------------------------------------------------------------------------

/// Audit record for a single policy evaluation decision.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PolicyDecision {
    pub request_id: String,
    pub subject_id: String,
    pub subject_type: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub decision: PolicyEffect,
    /// Which policy produced the match (None = default deny / no match).
    pub matched_policy: Option<Id>,
    pub evaluated_policies: u32,
    pub evaluation_ms: f64,
    pub evaluated_at: u64,
}

// ---------------------------------------------------------------------------
// Built-in policy helpers
// ---------------------------------------------------------------------------

/// Create the set of built-in tenant-level policies that Gyre ships with.
///
/// These enforce fundamental invariants and cannot be deleted.
///
/// # Key design decisions (HSI §2)
///
/// - `system-full-access` matches on `subject.id == "gyre-system-token"` (NOT
///   `subject.type == "system"`). This means the merge processor (`subject.type:
///   "system"`, `subject.id: "merge-processor"`) is subject to ABAC and can be
///   blocked by trust-preset policies such as `trust:require-human-mr-review`.
///
/// - `builtin:require-human-spec-approval` is `immutable: true`. Immutable Deny
///   policies are evaluated before all priority-based evaluation — including the
///   `system-full-access` Allow at priority 1000 — and cannot be overridden.
pub fn builtin_policies(created_by: impl Into<String>) -> Vec<Policy> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let by = created_by.into();

    vec![
        // The global GYRE_AUTH_TOKEN identity gets unconditional access.
        // Matched by subject.id (the specific token identity), NOT subject.type,
        // so the merge processor (subject.type: "system") is NOT included here
        // and remains subject to trust-preset policies.
        Policy {
            id: Id::new("builtin-system-access"),
            name: "system-full-access".to_string(),
            description:
                "Global GYRE_AUTH_TOKEN identity has full access (matched by subject.id, not type)"
                    .to_string(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 1000,
            effect: PolicyEffect::Allow,
            conditions: vec![Condition {
                attribute: "subject.id".to_string(),
                operator: ConditionOp::Equals,
                value: ConditionValue::String("gyre-system-token".to_string()),
            }],
            actions: vec!["*".to_string()],
            resource_types: vec!["*".to_string()],
            enabled: true,
            built_in: true,
            immutable: false,
            created_by: by.clone(),
            created_at: now,
            updated_at: now,
        },
        // Spec approval is always human. Immutable — evaluated before ALL
        // priority-based policies including system-full-access at priority 1000.
        // Cannot be overridden by any Allow regardless of priority.
        Policy {
            id: Id::new("builtin-require-human-spec-approval"),
            name: "builtin:require-human-spec-approval".to_string(),
            description: "Spec approval is always human, regardless of trust level or subject type"
                .to_string(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 999,
            effect: PolicyEffect::Deny,
            conditions: vec![Condition {
                attribute: "subject.type".to_string(),
                operator: ConditionOp::NotEquals,
                value: ConditionValue::String("user".to_string()),
            }],
            actions: vec!["approve".to_string()],
            resource_types: vec!["spec".to_string()],
            enabled: true,
            built_in: true,
            immutable: true,
            created_by: by.clone(),
            created_at: now,
            updated_at: now,
        },
        // Agents can only access resources in the repo they were spawned against.
        Policy {
            id: Id::new("builtin-agent-repo-scope"),
            name: "agent-repo-scope".to_string(),
            description: "Agents can only access resources in their scoped repo".to_string(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 100,
            effect: PolicyEffect::Deny,
            conditions: vec![
                Condition {
                    attribute: "subject.type".to_string(),
                    operator: ConditionOp::Equals,
                    value: ConditionValue::String("agent".to_string()),
                },
                Condition {
                    attribute: "subject.repo_scope".to_string(),
                    operator: ConditionOp::Exists,
                    value: ConditionValue::Null,
                },
            ],
            actions: vec!["*".to_string()],
            resource_types: vec!["*".to_string()],
            enabled: false, // Off by default; enable to enforce strict agent scoping.
            built_in: true,
            immutable: false,
            created_by: by.clone(),
            created_at: now,
            updated_at: now,
        },
        // Viewers cannot spawn agents.
        Policy {
            id: Id::new("builtin-viewer-no-spawn"),
            name: "viewer-no-spawn".to_string(),
            description: "Viewers cannot spawn agents".to_string(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 90,
            effect: PolicyEffect::Deny,
            conditions: vec![Condition {
                attribute: "subject.workspace_role".to_string(),
                operator: ConditionOp::Equals,
                value: ConditionValue::String("Viewer".to_string()),
            }],
            actions: vec!["spawn".to_string()],
            resource_types: vec!["agent".to_string()],
            enabled: true,
            built_in: true,
            immutable: false,
            created_by: by.clone(),
            created_at: now,
            updated_at: now,
        },
        // Developers (and above) can generate explorer views and specs via LLM.
        // Priority 800 — below system-full-access (1000) but above user policies (200-299).
        Policy {
            id: Id::new("builtin-developer-generate-access"),
            name: "developer-generate-access".to_string(),
            description: "Developers and above can generate explorer views and specs via LLM"
                .to_string(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 800,
            effect: PolicyEffect::Allow,
            conditions: vec![Condition {
                attribute: "subject.workspace_role".to_string(),
                operator: ConditionOp::In,
                value: ConditionValue::StringList(vec![
                    "Developer".to_string(),
                    "Admin".to_string(),
                ]),
            }],
            actions: vec!["generate".to_string()],
            resource_types: vec!["explorer_view".to_string(), "spec".to_string()],
            enabled: true,
            built_in: true,
            immutable: false,
            created_by: by.clone(),
            created_at: now,
            updated_at: now,
        },
    ]
}

// ---------------------------------------------------------------------------
// Trust preset policy helpers
// ---------------------------------------------------------------------------

/// Generate the `trust:` prefixed ABAC policies for a given trust level.
///
/// These are created when a workspace transitions to the given trust level
/// and deleted (by prefix) when transitioning away. They are scoped to the
/// specific workspace (`scope: Workspace`, `scope_id: workspace_id`).
///
/// Priority range: 100-199 (below user-created policies at 200-299 so user-
/// created Allow policies can intentionally override trust Deny policies).
pub fn trust_policies_for_level(
    trust_level: &crate::TrustLevel,
    workspace_id: &str,
    created_by: &str,
) -> Vec<Policy> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    match trust_level {
        crate::TrustLevel::Supervised => {
            // Supervised: block the merge processor from autonomous merge.
            // The merge processor uses subject.type: "system", subject.id: "merge-processor".
            // The system-full-access builtin matches subject.id == "gyre-system-token" only,
            // so the merge processor is NOT covered by that Allow and IS subject to this Deny.
            vec![Policy {
                id: Id::new(format!("trust-supervised-{workspace_id}")),
                name: "trust:require-human-mr-review".to_string(),
                description:
                    "trust: Block autonomous merge processor — require human MR approval first"
                        .to_string(),
                scope: PolicyScope::Workspace,
                scope_id: Some(workspace_id.to_string()),
                priority: 150,
                effect: PolicyEffect::Deny,
                conditions: vec![Condition {
                    attribute: "subject.type".to_string(),
                    operator: ConditionOp::Equals,
                    value: ConditionValue::String("system".to_string()),
                }],
                actions: vec!["merge".to_string()],
                resource_types: vec!["mr".to_string()],
                enabled: true,
                built_in: false,
                immutable: false,
                created_by: created_by.to_string(),
                created_at: now,
                updated_at: now,
            }]
        }
        // Guided: no trust: policies — relies on built-in policies only.
        // The delta from Supervised is the REMOVAL of trust:require-human-mr-review.
        crate::TrustLevel::Guided => vec![],
        // Autonomous: no trust: policies needed — built-in immutable spec approval
        // policy handles the only remaining constraint.
        crate::TrustLevel::Autonomous => vec![],
        // Custom: no trust: policies created; user manages ABAC directly.
        crate::TrustLevel::Custom => vec![],
    }
}
