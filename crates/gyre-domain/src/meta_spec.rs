//! Meta-spec registry domain types (agent-runtime spec §2).
//!
//! Meta-specs are DB-backed persona/principle/standard/process definitions that
//! govern how agents operate. They are versioned, approval-gated, and scoped
//! to Global, a specific Tenant, or a specific Workspace.

use gyre_common::Id;
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// The category of a meta-spec.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetaSpecKind {
    #[serde(rename = "meta:persona")]
    Persona,
    #[serde(rename = "meta:principle")]
    Principle,
    #[serde(rename = "meta:standard")]
    Standard,
    #[serde(rename = "meta:process")]
    Process,
}

impl MetaSpecKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            MetaSpecKind::Persona => "meta:persona",
            MetaSpecKind::Principle => "meta:principle",
            MetaSpecKind::Standard => "meta:standard",
            MetaSpecKind::Process => "meta:process",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "meta:persona" => Some(MetaSpecKind::Persona),
            "meta:principle" => Some(MetaSpecKind::Principle),
            "meta:standard" => Some(MetaSpecKind::Standard),
            "meta:process" => Some(MetaSpecKind::Process),
            _ => None,
        }
    }
}

impl fmt::Display for MetaSpecKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The scope at which a meta-spec applies.
///
/// The associated entity ID lives in `MetaSpec::scope_id` (the scope enum is
/// stored as a plain string column, mirroring the existing Workspace design):
/// Tenant-scoped meta-specs carry the tenant ID in `scope_id`, Workspace-
/// scoped ones the workspace ID; Global meta-specs have `scope_id = None`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetaSpecScope {
    Global,
    Tenant,
    Workspace,
}

impl MetaSpecScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            MetaSpecScope::Global => "Global",
            MetaSpecScope::Tenant => "Tenant",
            MetaSpecScope::Workspace => "Workspace",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Global" => Some(MetaSpecScope::Global),
            "Tenant" => Some(MetaSpecScope::Tenant),
            "Workspace" => Some(MetaSpecScope::Workspace),
            _ => None,
        }
    }
}

impl fmt::Display for MetaSpecScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Approval lifecycle of a meta-spec.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetaSpecApprovalStatus {
    Pending,
    Approved,
    Rejected,
}

impl MetaSpecApprovalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            MetaSpecApprovalStatus::Pending => "Pending",
            MetaSpecApprovalStatus::Approved => "Approved",
            MetaSpecApprovalStatus::Rejected => "Rejected",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Pending" => Some(MetaSpecApprovalStatus::Pending),
            "Approved" => Some(MetaSpecApprovalStatus::Approved),
            "Rejected" => Some(MetaSpecApprovalStatus::Rejected),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Entities
// ---------------------------------------------------------------------------

/// A DB-backed meta-spec definition.
///
/// Each update creates a new version row (stored in `meta_spec_versions`) and
/// bumps `version`. The `content_hash` is a SHA-256 hex digest of `prompt`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetaSpec {
    pub id: Id,
    pub kind: MetaSpecKind,
    pub name: String,
    pub scope: MetaSpecScope,
    /// The entity the meta-spec is scoped to: the workspace ID for
    /// Workspace-scoped meta-specs, the tenant ID for Tenant-scoped ones.
    /// None for Global.
    pub scope_id: Option<String>,
    /// The prompt content injected into agent context.
    pub prompt: String,
    /// Monotonically increasing version counter. Starts at 1.
    pub version: u32,
    /// SHA-256 hex digest of `prompt`.
    pub content_hash: String,
    /// True for org-wide mandatory meta-specs.
    pub required: bool,
    pub approval_status: MetaSpecApprovalStatus,
    pub approved_by: Option<String>,
    pub approved_at: Option<u64>,
    /// Content hash (`content_hash` value) covered by the current approval.
    /// Set when the spec transitions to Approved; used by the §6
    /// reconciliation trigger to distinguish approving changed content
    /// (trigger) from re-approving unchanged content (no-op). Never
    /// derived from the caller.
    pub approved_content_hash: Option<String>,
    pub created_by: String,
    pub created_at: u64,
    pub updated_at: u64,
}

/// A snapshot of a meta-spec at a specific version.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetaSpecVersion {
    pub id: Id,
    pub meta_spec_id: Id,
    pub version: u32,
    pub prompt: String,
    pub content_hash: String,
    pub created_at: u64,
}

/// A binding that pins a spec (identified by `spec_id`) to a specific version
/// of a meta-spec.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetaSpecBinding {
    pub id: Id,
    /// The spec being bound (e.g. a repo spec path).
    pub spec_id: String,
    pub meta_spec_id: Id,
    pub pinned_version: u32,
    pub created_at: u64,
}

// ---------------------------------------------------------------------------
// §7 Rollout Policy (meta-spec-reconciliation.md)
// ---------------------------------------------------------------------------

/// How a workspace's reconciliation proceeds after a meta-spec change
/// (meta-spec-reconciliation.md §7).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RolloutStrategy {
    /// Create all reconciliation tasks immediately.
    /// Budget limits still apply — tasks queue if budget exhausted.
    Immediate,

    /// Create tasks in batches. Wait for each batch to complete before
    /// starting the next. Batch size = `max_concurrent_agents`.
    Rolling,

    /// Create tasks but leave them in Backlog. Reconciliation competes with
    /// feature work for agent time. Repo orchestrator decides priority.
    Background,
}

impl RolloutStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            RolloutStrategy::Immediate => "immediate",
            RolloutStrategy::Rolling => "rolling",
            RolloutStrategy::Background => "background",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "immediate" => Some(RolloutStrategy::Immediate),
            "rolling" => Some(RolloutStrategy::Rolling),
            "background" => Some(RolloutStrategy::Background),
            _ => None,
        }
    }
}

impl fmt::Display for RolloutStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Scheduling weight of reconciliation tasks relative to feature work
/// (meta-spec-reconciliation.md §7).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReconciliationPriority {
    /// Reconciliation tasks are processed before feature work.
    High,

    /// Reconciliation tasks are interleaved with feature work.
    /// Repo orchestrator decides ordering.
    Normal,

    /// Reconciliation tasks are processed only when no feature work is pending.
    Low,
}

impl ReconciliationPriority {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReconciliationPriority::High => "high",
            ReconciliationPriority::Normal => "normal",
            ReconciliationPriority::Low => "low",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "high" => Some(ReconciliationPriority::High),
            "normal" => Some(ReconciliationPriority::Normal),
            "low" => Some(ReconciliationPriority::Low),
            _ => None,
        }
    }

    /// Map to the `Task` priority the repo orchestrator sees when ordering
    /// work: High → High (before feature work), Normal → Medium (interleaved),
    /// Low → Low (only when no feature work is pending).
    pub fn task_priority(&self) -> crate::TaskPriority {
        match self {
            ReconciliationPriority::High => crate::TaskPriority::High,
            ReconciliationPriority::Normal => crate::TaskPriority::Medium,
            ReconciliationPriority::Low => crate::TaskPriority::Low,
        }
    }
}

impl fmt::Display for ReconciliationPriority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Default batch size for the Rolling strategy. The spec pins the strategy
/// defaults (Rolling / Normal / auto_merge false) but leaves the concurrent
/// batch size to configuration; 5 reconciles progressively without letting
/// reconciliation dominate feature work.
pub const DEFAULT_MAX_CONCURRENT_AGENTS: u32 = 5;

/// A workspace's rollout policy — governs how reconciliation proceeds after
/// a meta-spec change (meta-spec-reconciliation.md §7).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RolloutPolicy {
    pub strategy: RolloutStrategy,
    /// Rolling-strategy batch size: tasks created per batch.
    pub max_concurrent_agents: u32,
    pub priority: ReconciliationPriority,
    /// When true, reconciliation MRs still require gate passage but merge
    /// automatically once gates pass (§7 default: false).
    pub auto_merge: bool,
}

impl Default for RolloutPolicy {
    /// §7: "Default: `Rolling` strategy, `Normal` priority, `auto_merge: false`".
    fn default() -> Self {
        Self {
            strategy: RolloutStrategy::Rolling,
            max_concurrent_agents: DEFAULT_MAX_CONCURRENT_AGENTS,
            priority: ReconciliationPriority::Normal,
            auto_merge: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollout_policy_default_matches_spec() {
        // §7: "Default: `Rolling` strategy, `Normal` priority, `auto_merge: false`."
        let p = RolloutPolicy::default();
        assert_eq!(p.strategy, RolloutStrategy::Rolling);
        assert_eq!(p.priority, ReconciliationPriority::Normal);
        assert!(!p.auto_merge);
        assert!(
            p.max_concurrent_agents >= 1,
            "rolling batch size must be non-zero"
        );
    }

    #[test]
    fn rollout_strategy_round_trips_wire_names() {
        for s in [
            &RolloutStrategy::Immediate,
            &RolloutStrategy::Rolling,
            &RolloutStrategy::Background,
        ] {
            let json = serde_json::to_string(s).unwrap();
            assert_eq!(serde_json::from_str::<RolloutStrategy>(&json).unwrap(), *s);
            assert_eq!(RolloutStrategy::parse(s.as_str()), Some(s.clone()));
        }
        // Wire names are the lowercase strategy names from the §7 table.
        assert_eq!(
            serde_json::to_string(&RolloutStrategy::Immediate).unwrap(),
            "\"immediate\""
        );
        assert_eq!(
            serde_json::to_string(&RolloutStrategy::Rolling).unwrap(),
            "\"rolling\""
        );
        assert_eq!(
            serde_json::to_string(&RolloutStrategy::Background).unwrap(),
            "\"background\""
        );
        assert!(RolloutStrategy::parse("staged").is_none());
    }

    #[test]
    fn reconciliation_priority_round_trips_and_maps() {
        use crate::TaskPriority;
        for p in [
            &ReconciliationPriority::High,
            &ReconciliationPriority::Normal,
            &ReconciliationPriority::Low,
        ] {
            let json = serde_json::to_string(p).unwrap();
            assert_eq!(
                serde_json::from_str::<ReconciliationPriority>(&json).unwrap(),
                *p
            );
            assert_eq!(ReconciliationPriority::parse(p.as_str()), Some(p.clone()));
        }
        assert_eq!(
            ReconciliationPriority::High.task_priority(),
            TaskPriority::High
        );
        assert_eq!(
            ReconciliationPriority::Normal.task_priority(),
            TaskPriority::Medium
        );
        assert_eq!(
            ReconciliationPriority::Low.task_priority(),
            TaskPriority::Low
        );
        assert!(ReconciliationPriority::parse("urgent").is_none());
    }

    #[test]
    fn rollout_policy_serializes_full_shape() {
        let p = RolloutPolicy {
            strategy: RolloutStrategy::Rolling,
            max_concurrent_agents: 3,
            priority: ReconciliationPriority::High,
            auto_merge: true,
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"strategy\":\"rolling\""));
        assert!(json.contains("\"max_concurrent_agents\":3"));
        assert!(json.contains("\"priority\":\"high\""));
        assert!(json.contains("\"auto_merge\":true"));
        let back: RolloutPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }
}
