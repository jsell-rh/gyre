//! ABAC (Attribute-Based Access Control) policy domain types.
//!
//! Policies evaluate subject/resource/action/environment attributes to produce
//! an Allow or Deny decision. Policies are scoped (Tenant > Workspace > Repo),
//! prioritised (higher number = evaluated first), and composable (first match wins).

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
pub fn builtin_policies(created_by: impl Into<String>) -> Vec<Policy> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let by = created_by.into();

    vec![
        // System token gets unconditional access.
        Policy {
            id: Id::new("builtin-system-access"),
            name: "system-access".to_string(),
            description: "System token has full access".to_string(),
            scope: PolicyScope::Tenant,
            scope_id: None,
            priority: 1000,
            effect: PolicyEffect::Allow,
            conditions: vec![Condition {
                attribute: "subject.type".to_string(),
                operator: ConditionOp::Equals,
                value: ConditionValue::String("system".to_string()),
            }],
            actions: vec!["*".to_string()],
            resource_types: vec!["*".to_string()],
            enabled: true,
            built_in: true,
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
            created_by: by.clone(),
            created_at: now,
            updated_at: now,
        },
    ]
}
