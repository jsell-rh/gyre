//! Spec-link graph domain types (spec-links.md §Forge-Maintained Spec Graph).
//!
//! `SpecLinkEntry` is the forge's tenant-wide directed graph of spec links.
//! It lives in `gyre-domain` (alongside `SpecLedgerEntry`) so the
//! `gyre-ports::SpecLinkRepository` port and its SQLite/Postgres/Mem adapters
//! can persist it; `gyre-server::spec_registry` re-exports it.

use serde::{Deserialize, Serialize};

/// Link type between specs — drives mechanical enforcement.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SpecLinkType {
    Implements,
    Supersedes,
    DependsOn,
    ConflictsWith,
    Extends,
    References,
}

impl std::fmt::Display for SpecLinkType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpecLinkType::Implements => write!(f, "implements"),
            SpecLinkType::Supersedes => write!(f, "supersedes"),
            SpecLinkType::DependsOn => write!(f, "depends_on"),
            SpecLinkType::ConflictsWith => write!(f, "conflicts_with"),
            SpecLinkType::Extends => write!(f, "extends"),
            SpecLinkType::References => write!(f, "references"),
        }
    }
}

impl std::str::FromStr for SpecLinkType {
    type Err = String;
    /// Inverse of `Display` / snake_case serde — parses the value stored in
    /// the `spec_links.link_type` column back into the typed variant.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "implements" => Ok(SpecLinkType::Implements),
            "supersedes" => Ok(SpecLinkType::Supersedes),
            "depends_on" => Ok(SpecLinkType::DependsOn),
            "conflicts_with" => Ok(SpecLinkType::ConflictsWith),
            "extends" => Ok(SpecLinkType::Extends),
            "references" => Ok(SpecLinkType::References),
            other => Err(format!("unknown spec link type: {other}")),
        }
    }
}

/// A resolved link entry stored in the forge's spec link graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecLinkEntry {
    pub id: String,
    /// Source spec path (the spec that declares this link).
    pub source_path: String,
    /// Repo ID that owns the source spec (for cross-workspace link scoping).
    pub source_repo_id: Option<String>,
    /// Blob SHA of the source spec at the revision this link was recorded from
    /// (spec-links.md: `source_sha TEXT NOT NULL`).
    pub source_sha: String,
    pub link_type: SpecLinkType,
    /// Target spec path (within the target repo, without leading @workspace/repo prefix).
    pub target_path: String,
    /// Resolved target repo UUID. None for unresolved cross-workspace links.
    pub target_repo_id: Option<String>,
    /// Human-readable composite path preserved from the manifest `target` field
    /// (e.g. "@platform-core/api-svc/system/auth.md"). Used for display and staleness checking.
    /// None for same-repo links.
    pub target_display: Option<String>,
    /// SHA the link was pinned to.
    pub target_sha: Option<String>,
    pub reason: Option<String>,
    /// Link health: "active" | "stale" | "broken" | "conflicted" | "unresolved"
    pub status: String,
    pub created_at: u64,
    pub stale_since: Option<u64>,
}
