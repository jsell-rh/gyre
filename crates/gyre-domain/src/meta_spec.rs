//! Meta-spec registry domain types (agent-runtime spec §2).
//!
//! Meta-specs are DB-backed persona/principle/standard/process definitions that
//! govern how agents operate. They are versioned, approval-gated, and scoped to
//! Global or a specific Workspace.

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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetaSpecScope {
    Global,
    Workspace,
}

impl MetaSpecScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            MetaSpecScope::Global => "Global",
            MetaSpecScope::Workspace => "Workspace",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Global" => Some(MetaSpecScope::Global),
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
    /// For Workspace-scoped meta-specs: the workspace ID. None for Global.
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
