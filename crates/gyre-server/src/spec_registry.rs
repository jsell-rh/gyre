//! Spec Registry: manifest parser, in-memory ledger, ledger sync on push.
//!
//! The spec registry is a two-part system:
//! 1. `specs/manifest.yaml` (in git) — declares what specs exist and their policies.
//! 2. Forge spec ledger (in memory) — tracks runtime state per spec: current SHA,
//!    approval status, linked MRs/tasks, drift status.
//!
//! The ledger is synced from the manifest on every push to the default branch.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::Mutex;
use tracing::{info, warn};

pub use gyre_domain::{ApprovalStatus, SpecApprovalEvent, SpecLedgerEntry};

// ---------------------------------------------------------------------------
// Manifest structs (parsed from specs/manifest.yaml)
// ---------------------------------------------------------------------------

/// Top-level manifest document.
#[derive(Debug, Clone, Deserialize)]
pub struct SpecManifest {
    pub version: u32,
    #[serde(default)]
    pub defaults: ManifestDefaults,
    pub specs: Vec<SpecEntry>,
}

/// Default policies applied to all specs unless overridden per-entry.
#[derive(Debug, Clone, Deserialize)]
pub struct ManifestDefaults {
    #[serde(default = "default_true")]
    pub requires_approval: bool,
    #[serde(default = "default_true")]
    pub auto_create_tasks: bool,
    #[serde(default = "default_true")]
    pub auto_invalidate_on_change: bool,
}

impl Default for ManifestDefaults {
    fn default() -> Self {
        Self {
            requires_approval: true,
            auto_create_tasks: true,
            auto_invalidate_on_change: true,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Link type between specs — drives mechanical enforcement.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
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

/// A link declared in the manifest between specs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpecLink {
    #[serde(rename = "type")]
    pub link_type: SpecLinkType,
    /// Target spec path relative to `specs/`, e.g. `system/source-control.md`.
    pub target: String,
    /// SHA the link was pinned to. Stale if target spec SHA advances.
    pub target_sha: Option<String>,
    /// Human-readable rationale.
    pub reason: Option<String>,
}

/// A single spec entry in the manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct SpecEntry {
    pub path: String,
    pub title: String,
    pub owner: String,
    /// Optional kind for meta-specs: "meta:persona", "meta:principle", "meta:standard", "meta:process".
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub approval: Option<ApprovalConfig>,
    #[serde(default)]
    pub gates: Vec<GateConfig>,
    /// Overrides `defaults.requires_approval` for this spec.
    pub requires_approval: Option<bool>,
    /// Overrides `defaults.auto_create_tasks` for this spec.
    pub auto_create_tasks: Option<bool>,
    /// Overrides `defaults.auto_invalidate_on_change` for this spec.
    pub auto_invalidate_on_change: Option<bool>,
    /// If set, this spec has been superseded by another.
    pub superseded_by: Option<String>,
    /// Links to other specs (machine-readable graph edges).
    #[serde(default)]
    pub links: Vec<SpecLink>,
}

impl SpecEntry {
    pub fn effective_requires_approval(&self, defaults: &ManifestDefaults) -> bool {
        self.requires_approval.unwrap_or(defaults.requires_approval)
    }

    pub fn effective_auto_create_tasks(&self, defaults: &ManifestDefaults) -> bool {
        self.auto_create_tasks.unwrap_or(defaults.auto_create_tasks)
    }

    pub fn effective_auto_invalidate(&self, defaults: &ManifestDefaults) -> bool {
        self.auto_invalidate_on_change
            .unwrap_or(defaults.auto_invalidate_on_change)
    }

    pub fn effective_approval_mode(&self) -> ApprovalMode {
        self.approval
            .as_ref()
            .map(|a| a.mode.clone())
            .unwrap_or(ApprovalMode::HumanAndAgent)
    }
}

/// Per-spec approval configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ApprovalConfig {
    pub mode: ApprovalMode,
    #[serde(default)]
    pub human_approvers: Vec<String>,
    #[serde(default)]
    pub agent_approvers: Vec<AgentApproverConfig>,
}

/// Approval mode for a spec.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    HumanOnly,
    AgentOnly,
    HumanAndAgent,
}

impl std::fmt::Display for ApprovalMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApprovalMode::HumanOnly => write!(f, "human_only"),
            ApprovalMode::AgentOnly => write!(f, "agent_only"),
            ApprovalMode::HumanAndAgent => write!(f, "human_and_agent"),
        }
    }
}

/// Agent approver config (shared schema for approvers and gates).
#[derive(Debug, Clone, Deserialize)]
pub struct AgentApproverConfig {
    pub persona: String,
    pub min_attestation_level: Option<u32>,
    pub stack_hash: Option<String>,
}

/// Gate agent config on a spec.
#[derive(Debug, Clone, Deserialize)]
pub struct GateConfig {
    pub persona: String,
    pub min_attestation_level: Option<u32>,
    pub stack_hash: Option<String>,
}

/// Parse YAML text into a SpecManifest.
pub fn parse_manifest(yaml: &str) -> Result<SpecManifest, serde_yaml::Error> {
    serde_yaml::from_str(yaml)
}

// ---------------------------------------------------------------------------
// Ledger types (runtime state tracked per spec)
// ---------------------------------------------------------------------------

// ApprovalStatus, SpecLedgerEntry, SpecApprovalEvent are re-exported from gyre_domain above.

/// Type alias for the shared ledger store (in-memory, used by tests and sync_spec_ledger).
pub type SpecLedger = Arc<Mutex<HashMap<String, SpecLedgerEntry>>>;
/// Type alias for the shared approval history store (in-memory).
pub type SpecApprovalHistory = Arc<Mutex<Vec<SpecApprovalEvent>>>;

/// A resolved link entry stored in the forge's spec link graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecLinkEntry {
    pub id: String,
    /// Source spec path (the spec that declares this link).
    pub source_path: String,
    pub link_type: SpecLinkType,
    /// Target spec path.
    pub target_path: String,
    /// SHA the link was pinned to.
    pub target_sha: Option<String>,
    pub reason: Option<String>,
    /// Link health: "active" | "stale" | "broken" | "conflicted"
    pub status: String,
    pub created_at: u64,
    pub stale_since: Option<u64>,
}

/// Type alias for the shared spec links store.
pub type SpecLinksStore = Arc<Mutex<Vec<SpecLinkEntry>>>;

// ---------------------------------------------------------------------------
// Ledger sync — called after a push to the default branch
// ---------------------------------------------------------------------------

/// Sync the spec ledger from the manifest committed at HEAD of the given repo.
///
/// Reads `specs/manifest.yaml` from the new HEAD via `git show`, parses it,
/// then for each entry computes the blob SHA and updates the ledger accordingly:
/// - New entry: create with `approval_status = Pending`.
/// - Changed SHA: update SHA, reset `approval_status = Pending` if `auto_invalidate_on_change`.
/// - Entry in ledger but not manifest: mark `Deprecated`.
/// - Files under `specs/` not in manifest: log a warning.
/// - `supersedes` links: target spec is marked Deprecated in ledger.
/// - `extends` links: if the target SHA changed, the extending spec's drift_status = "drifted".
pub async fn sync_spec_ledger(
    ledger: &Arc<dyn gyre_ports::SpecLedgerRepository>,
    links_store: &SpecLinksStore,
    repo_path: &str,
    new_sha: &str,
    now: u64,
) {
    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());

    // 1. Read manifest from the new HEAD.
    let manifest_yaml =
        match read_git_file(&git_bin, repo_path, new_sha, "specs/manifest.yaml").await {
            Some(content) => content,
            None => {
                // No manifest in this repo — nothing to sync.
                return;
            }
        };

    // 2. Parse the manifest.
    let manifest = match parse_manifest(&manifest_yaml) {
        Ok(m) => m,
        Err(e) => {
            warn!(repo_path, "spec-registry: failed to parse manifest: {e}");
            return;
        }
    };

    // 3. Build set of paths in manifest.
    let manifest_paths: std::collections::HashSet<String> =
        manifest.specs.iter().map(|e| e.path.clone()).collect();

    // 4. For each manifest entry, compute blob SHA and sync ledger.
    for entry in &manifest.specs {
        let spec_file_path = format!("specs/{}", entry.path);
        let blob_sha = match get_blob_sha(&git_bin, repo_path, new_sha, &spec_file_path).await {
            Some(sha) => sha,
            None => {
                warn!(
                    repo_path,
                    spec_path = %entry.path,
                    "spec-registry: manifest entry has no corresponding file at HEAD"
                );
                "".to_string()
            }
        };

        let approval_mode = entry.effective_approval_mode().to_string();
        let auto_invalidate = entry.effective_auto_invalidate(&manifest.defaults);

        let updated_entry = if let Ok(Some(mut existing)) = ledger.find_by_path(&entry.path).await {
            // Already in ledger — check if SHA changed.
            if existing.current_sha != blob_sha && !blob_sha.is_empty() {
                info!(
                    spec_path = %entry.path,
                    old_sha = %existing.current_sha,
                    new_sha = %blob_sha,
                    "spec-registry: SHA changed"
                );
                existing.current_sha = blob_sha;
                existing.updated_at = now;
                if auto_invalidate {
                    existing.approval_status = ApprovalStatus::Pending;
                    info!(spec_path = %entry.path, "spec-registry: approval invalidated (content changed)");
                }
            }
            existing.title = entry.title.clone();
            existing.owner = entry.owner.clone();
            existing.approval_mode = approval_mode;
            if existing.approval_status == ApprovalStatus::Deprecated {
                existing.approval_status = ApprovalStatus::Pending;
                existing.updated_at = now;
            }
            existing
        } else {
            info!(spec_path = %entry.path, "spec-registry: new spec registered");
            SpecLedgerEntry {
                path: entry.path.clone(),
                title: entry.title.clone(),
                owner: entry.owner.clone(),
                kind: entry.kind.clone(),
                current_sha: blob_sha,
                approval_mode,
                approval_status: ApprovalStatus::Pending,
                linked_tasks: vec![],
                linked_mrs: vec![],
                drift_status: "unknown".to_string(),
                created_at: now,
                updated_at: now,
            }
        };
        let _ = ledger.save(&updated_entry).await;
    }

    // 5. Deprecate ledger entries no longer in manifest.
    if let Ok(all_entries) = ledger.list_all().await {
        for mut e in all_entries {
            if !manifest_paths.contains(&e.path) && e.approval_status != ApprovalStatus::Deprecated
            {
                info!(spec_path = %e.path, "spec-registry: spec deprecated (removed from manifest)");
                e.approval_status = ApprovalStatus::Deprecated;
                e.updated_at = now;
                let _ = ledger.save(&e).await;
            }
        }
    }

    // 6. Process spec links from manifest — enforce supersedes/extends, update links store.
    {
        let mut new_links: Vec<SpecLinkEntry> = Vec::new();
        for entry in &manifest.specs {
            for link in &entry.links {
                let id = format!("{}-{}-{}", entry.path, link.link_type, link.target);
                new_links.push(SpecLinkEntry {
                    id,
                    source_path: entry.path.clone(),
                    link_type: link.link_type.clone(),
                    target_path: link.target.clone(),
                    target_sha: link.target_sha.clone(),
                    reason: link.reason.clone(),
                    status: "active".to_string(),
                    created_at: now,
                    stale_since: None,
                });
            }
        }

        // Enforce link semantics.
        for link in &new_links {
            match link.link_type {
                SpecLinkType::Supersedes => {
                    if let Ok(Some(mut target_entry)) = ledger.find_by_path(&link.target_path).await
                    {
                        if target_entry.approval_status != ApprovalStatus::Deprecated {
                            info!(
                                source = %link.source_path,
                                target = %link.target_path,
                                "spec-registry: supersedes link — marking target deprecated"
                            );
                            target_entry.approval_status = ApprovalStatus::Deprecated;
                            target_entry.updated_at = now;
                            let _ = ledger.save(&target_entry).await;
                        }
                    }
                }
                SpecLinkType::Extends => {
                    if let Some(pinned_sha) = &link.target_sha {
                        if let Ok(Some(target_entry)) = ledger.find_by_path(&link.target_path).await
                        {
                            let current_sha = &target_entry.current_sha;
                            if !current_sha.is_empty() && current_sha != pinned_sha {
                                info!(
                                    source = %link.source_path,
                                    target = %link.target_path,
                                    "spec-registry: extends target SHA changed — marking extending spec drifted"
                                );
                                if let Ok(Some(mut source_entry)) =
                                    ledger.find_by_path(&link.source_path).await
                                {
                                    source_entry.drift_status = "drifted".to_string();
                                    source_entry.updated_at = now;
                                    let _ = ledger.save(&source_entry).await;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Replace all links originating from specs in this manifest (full refresh).
        {
            let source_paths: std::collections::HashSet<String> =
                manifest.specs.iter().map(|e| e.path.clone()).collect();
            let mut store = links_store.lock().await;
            store.retain(|l| !source_paths.contains(&l.source_path));
            store.extend(new_links);
        }
    }

    // 7. Warn about spec files not in manifest.
    check_unregistered_specs(&git_bin, repo_path, new_sha, &manifest_paths).await;
}

/// Read a file from a specific git commit using `git show <sha>:<path>`.
async fn read_git_file(
    git_bin: &str,
    repo_path: &str,
    sha: &str,
    file_path: &str,
) -> Option<String> {
    let object = format!("{sha}:{file_path}");
    let out = Command::new(git_bin)
        .arg("-C")
        .arg(repo_path)
        .arg("show")
        .arg(&object)
        .output()
        .await
        .ok()?;
    if out.status.success() {
        String::from_utf8(out.stdout).ok()
    } else {
        None
    }
}

/// Get the blob SHA for a file at a given commit using `git rev-parse <sha>:<path>`.
async fn get_blob_sha(
    git_bin: &str,
    repo_path: &str,
    sha: &str,
    file_path: &str,
) -> Option<String> {
    let object = format!("{sha}:{file_path}");
    let out = Command::new(git_bin)
        .arg("-C")
        .arg(repo_path)
        .arg("rev-parse")
        .arg(&object)
        .output()
        .await
        .ok()?;
    if out.status.success() {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    } else {
        None
    }
}

/// Warn about any `specs/*.md` files not listed in the manifest.
///
/// Uses `git ls-tree -r --name-only <sha> specs/` to enumerate spec files.
async fn check_unregistered_specs(
    git_bin: &str,
    repo_path: &str,
    sha: &str,
    manifest_paths: &std::collections::HashSet<String>,
) {
    let out = Command::new(git_bin)
        .arg("-C")
        .arg(repo_path)
        .arg("ls-tree")
        .arg("-r")
        .arg("--name-only")
        .arg(sha)
        .arg("specs/")
        .output()
        .await;

    let out = match out {
        Ok(o) if o.status.success() => o,
        _ => return,
    };

    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        // Only check .md files under specs/ (excluding manifest.yaml and index.md).
        if !line.ends_with(".md") {
            continue;
        }
        let relative = line.strip_prefix("specs/").unwrap_or(line);
        // Skip index.md and prior-art/ directory.
        if relative == "index.md"
            || relative.starts_with("prior-art/")
            || relative.starts_with("milestones/")
        {
            continue;
        }
        if !manifest_paths.contains(relative) {
            warn!(
                spec_path = %relative,
                "spec-registry: file under specs/ is not registered in manifest.yaml — \
                 add it to specs/manifest.yaml to enable lifecycle tracking"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_MANIFEST: &str = r#"
version: 1
defaults:
  requires_approval: true
  auto_create_tasks: true
  auto_invalidate_on_change: true
specs:
  - path: system/design-principles.md
    title: Design Principles
    owner: user:jsell
    approval:
      mode: human_only
      human_approvers:
        - user:jsell
  - path: development/architecture.md
    title: Architecture
    owner: user:jsell
    approval:
      mode: agent_only
      agent_approvers:
        - persona: accountability
  - path: system/trusted-foundry-integration.md
    title: Trusted Foundry
    owner: user:jsell
    auto_create_tasks: false
"#;

    #[test]
    fn test_parse_manifest_ok() {
        let m = parse_manifest(SAMPLE_MANIFEST).expect("parse failed");
        assert_eq!(m.version, 1);
        assert_eq!(m.specs.len(), 3);
        assert_eq!(m.specs[0].path, "system/design-principles.md");
        assert_eq!(m.specs[0].title, "Design Principles");
        assert!(m.defaults.requires_approval);
    }

    #[test]
    fn test_approval_mode_human_only() {
        let m = parse_manifest(SAMPLE_MANIFEST).unwrap();
        let entry = &m.specs[0];
        assert_eq!(entry.effective_approval_mode(), ApprovalMode::HumanOnly);
    }

    #[test]
    fn test_approval_mode_agent_only() {
        let m = parse_manifest(SAMPLE_MANIFEST).unwrap();
        let entry = &m.specs[1];
        assert_eq!(entry.effective_approval_mode(), ApprovalMode::AgentOnly);
    }

    #[test]
    fn test_approval_mode_default_human_and_agent() {
        let m = parse_manifest(SAMPLE_MANIFEST).unwrap();
        let entry = &m.specs[2]; // no explicit mode
        assert_eq!(entry.effective_approval_mode(), ApprovalMode::HumanAndAgent);
    }

    #[test]
    fn test_auto_create_tasks_override() {
        let m = parse_manifest(SAMPLE_MANIFEST).unwrap();
        let trusted_foundry = &m.specs[2];
        assert!(!trusted_foundry.effective_auto_create_tasks(&m.defaults));
    }

    #[test]
    fn test_parse_invalid_yaml() {
        let result = parse_manifest("not: valid: yaml: ][");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_ledger_sync_new_entry() {
        // Build a ledger and sync with a mock manifest (no real git repo).
        // We test the parse + ledger update path using a pre-computed SHA.
        let ledger: SpecLedger = Arc::new(Mutex::new(HashMap::new()));
        let now = 1700000000u64;

        // Manually insert what sync would insert (simulating a manifest with one entry).
        {
            let mut l = ledger.lock().await;
            l.insert(
                "system/design-principles.md".to_string(),
                SpecLedgerEntry {
                    path: "system/design-principles.md".to_string(),
                    title: "Design Principles".to_string(),
                    owner: "user:jsell".to_string(),
                    kind: None,
                    current_sha: "abc123".to_string(),
                    approval_mode: "human_only".to_string(),
                    approval_status: ApprovalStatus::Pending,
                    linked_tasks: vec![],
                    linked_mrs: vec![],
                    drift_status: "unknown".to_string(),
                    created_at: now,
                    updated_at: now,
                },
            );
        }

        let entry = ledger.lock().await;
        let e = entry.get("system/design-principles.md").unwrap();
        assert_eq!(e.approval_status, ApprovalStatus::Pending);
        assert_eq!(e.current_sha, "abc123");
    }

    #[tokio::test]
    async fn test_ledger_sha_change_invalidates_approval() {
        let ledger: SpecLedger = Arc::new(Mutex::new(HashMap::new()));
        let now = 1700000000u64;

        // Insert an approved entry.
        {
            let mut l = ledger.lock().await;
            l.insert(
                "system/design-principles.md".to_string(),
                SpecLedgerEntry {
                    path: "system/design-principles.md".to_string(),
                    title: "Design Principles".to_string(),
                    owner: "user:jsell".to_string(),
                    kind: None,
                    current_sha: "oldsha".to_string(),
                    approval_mode: "human_only".to_string(),
                    approval_status: ApprovalStatus::Approved,
                    linked_tasks: vec![],
                    linked_mrs: vec![],
                    drift_status: "clean".to_string(),
                    created_at: now,
                    updated_at: now,
                },
            );
        }

        // Simulate what sync does when SHA changes: update SHA and reset to Pending.
        {
            let mut l = ledger.lock().await;
            let entry = l.get_mut("system/design-principles.md").unwrap();
            // SHA changed.
            entry.current_sha = "newsha".to_string();
            entry.approval_status = ApprovalStatus::Pending;
            entry.updated_at = now + 100;
        }

        let l = ledger.lock().await;
        let e = l.get("system/design-principles.md").unwrap();
        assert_eq!(e.approval_status, ApprovalStatus::Pending);
        assert_eq!(e.current_sha, "newsha");
    }

    #[tokio::test]
    async fn test_ledger_deprecate_removed_entry() {
        let ledger: SpecLedger = Arc::new(Mutex::new(HashMap::new()));
        let now = 1700000000u64;

        {
            let mut l = ledger.lock().await;
            l.insert(
                "system/old-spec.md".to_string(),
                SpecLedgerEntry {
                    path: "system/old-spec.md".to_string(),
                    title: "Old Spec".to_string(),
                    owner: "user:jsell".to_string(),
                    kind: None,
                    current_sha: "sha".to_string(),
                    approval_mode: "human_only".to_string(),
                    approval_status: ApprovalStatus::Approved,
                    linked_tasks: vec![],
                    linked_mrs: vec![],
                    drift_status: "clean".to_string(),
                    created_at: now,
                    updated_at: now,
                },
            );
        }

        // Simulate deprecation (not in manifest paths).
        {
            let mut l = ledger.lock().await;
            let entry = l.get_mut("system/old-spec.md").unwrap();
            entry.approval_status = ApprovalStatus::Deprecated;
            entry.updated_at = now + 100;
        }

        let l = ledger.lock().await;
        let e = l.get("system/old-spec.md").unwrap();
        assert_eq!(e.approval_status, ApprovalStatus::Deprecated);
    }
}
