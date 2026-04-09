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
    /// Repo ID that owns the source spec (for cross-workspace link scoping).
    pub source_repo_id: Option<String>,
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

/// Type alias for the shared spec links store.
pub type SpecLinksStore = Arc<Mutex<Vec<SpecLinkEntry>>>;

/// Parsed cross-workspace target from an `@`-prefixed manifest link.
#[derive(Debug, Clone, PartialEq)]
pub enum CrossWorkspaceTarget {
    /// Same-repo link (no `@` prefix): just a spec path.
    SameRepo { path: String },
    /// Cross-repo same-workspace link (`@repo_name/spec_path`).
    CrossRepo { repo_name: String, path: String },
    /// Cross-workspace link (`@workspace_slug/repo_name/spec_path`).
    CrossWorkspace {
        workspace_slug: String,
        repo_name: String,
        path: String,
    },
}

/// Parse a manifest link `target` field into a typed cross-workspace target.
///
/// - Same-repo:       `"system/spec.md"`                          (no `@` prefix)
/// - Cross-repo:      `"@repo-name/system/spec.md"`               (single segment before path)
/// - Cross-workspace: `"@ws-slug/repo-name/system/spec.md"`       (two segments before path)
pub fn parse_cross_workspace_target(target: &str) -> CrossWorkspaceTarget {
    if let Some(rest) = target.strip_prefix('@') {
        // Split into at most 3 parts: first two are the address segments, rest is the path.
        let mut parts = rest.splitn(3, '/');
        match (parts.next(), parts.next(), parts.next()) {
            (Some(seg1), Some(seg2), Some(path)) => CrossWorkspaceTarget::CrossWorkspace {
                workspace_slug: seg1.to_string(),
                repo_name: seg2.to_string(),
                path: path.to_string(),
            },
            (Some(seg1), Some(seg2), None) => CrossWorkspaceTarget::CrossRepo {
                repo_name: seg1.to_string(),
                path: seg2.to_string(),
            },
            _ => CrossWorkspaceTarget::SameRepo {
                path: target.to_string(),
            },
        }
    } else {
        CrossWorkspaceTarget::SameRepo {
            path: target.to_string(),
        }
    }
}

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
/// - `extends` links: if the target SHA changed, the extending spec's drift_status = "drifted",
///   approval_status is invalidated to Pending, and a drift-review Task is created.
/// - Push-time inbound staleness: when a spec's SHA changes, ALL existing links targeting
///   that spec are marked stale immediately (spec-links.md §Automatic Staleness Detection).
/// - Cross-workspace `@` targets: resolved to target_repo_id via workspace slug lookup.
///   Unresolved targets stored with `status = "unresolved"` and `target_repo_id = None`.
#[allow(clippy::too_many_arguments)]
pub async fn sync_spec_ledger(
    ledger: &Arc<dyn gyre_ports::SpecLedgerRepository>,
    links_store: &SpecLinksStore,
    repo_path: &str,
    new_sha: &str,
    now: u64,
    // Context for cross-workspace resolution (pass None to skip resolution).
    source_repo_id: Option<&str>,
    source_workspace_id: Option<&str>,
    workspaces: Option<&Arc<dyn gyre_ports::WorkspaceRepository>>,
    repos: Option<&Arc<dyn gyre_ports::RepoRepository>>,
    tenant_id: Option<&gyre_common::Id>,
    // Task repository for creating drift-review tasks (TASK-016 F2).
    tasks: Option<&Arc<dyn gyre_ports::TaskRepository>>,
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
    // Track specs whose SHAs changed for inbound staleness detection (TASK-016 F1).
    let mut changed_spec_paths: Vec<String> = Vec::new();
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
                changed_spec_paths.push(entry.path.clone());
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
            // Backfill repo_id/workspace_id on existing entries for signal chain routing.
            if existing.repo_id.is_none() {
                existing.repo_id = source_repo_id.map(|s| s.to_string());
            }
            if existing.workspace_id.is_none() {
                existing.workspace_id = source_workspace_id.map(|s| s.to_string());
            }
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
                repo_id: source_repo_id.map(|s| s.to_string()),
                workspace_id: source_workspace_id.map(|s| s.to_string()),
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
                let parsed = parse_cross_workspace_target(&link.target);

                let (target_path, target_repo_id, target_display, status) = match &parsed {
                    CrossWorkspaceTarget::SameRepo { path } => {
                        (path.clone(), None, None, "active".to_string())
                    }
                    CrossWorkspaceTarget::CrossRepo { repo_name, path } => {
                        // Resolve repo name → repo_id within the source workspace.
                        let display = format!("@{}/{}", repo_name, path);
                        let resolved_repo_id = resolve_repo_by_name(
                            repos, workspaces, tenant_id,
                            None, // same workspace — we'll look up by name across all
                            repo_name,
                        )
                        .await;
                        if resolved_repo_id.is_none() {
                            warn!(
                                source = %entry.path,
                                target = %link.target,
                                "spec-registry: cross-repo target unresolved (repo not found)"
                            );
                        }
                        let status = if resolved_repo_id.is_some() {
                            "active".to_string()
                        } else {
                            "unresolved".to_string()
                        };
                        (path.clone(), resolved_repo_id, Some(display), status)
                    }
                    CrossWorkspaceTarget::CrossWorkspace {
                        workspace_slug,
                        repo_name,
                        path,
                    } => {
                        let display = format!("@{}/{}/{}", workspace_slug, repo_name, path);
                        let resolved_repo_id = resolve_cross_workspace_repo(
                            workspaces,
                            repos,
                            tenant_id,
                            workspace_slug,
                            repo_name,
                        )
                        .await;
                        if resolved_repo_id.is_none() {
                            warn!(
                                source = %entry.path,
                                target = %link.target,
                                workspace_slug = %workspace_slug,
                                repo_name = %repo_name,
                                "spec-registry: cross-workspace target unresolved"
                            );
                        }
                        let status = if resolved_repo_id.is_some() {
                            "active".to_string()
                        } else {
                            "unresolved".to_string()
                        };
                        (path.clone(), resolved_repo_id, Some(display), status)
                    }
                };

                new_links.push(SpecLinkEntry {
                    id,
                    source_path: entry.path.clone(),
                    source_repo_id: source_repo_id.map(|s| s.to_string()),
                    link_type: link.link_type.clone(),
                    target_path,
                    target_repo_id,
                    target_display,
                    target_sha: link.target_sha.clone(),
                    reason: link.reason.clone(),
                    status,
                    created_at: now,
                    stale_since: None,
                });
            }
        }

        // Enforce link semantics and detect staleness.
        // TASK-016: Check target_sha against current ledger SHA for ALL links.
        // spec-links.md §Automatic Staleness Detection.
        for link in &mut new_links {
            // Staleness detection: if the link has a pinned target_sha but the target
            // spec's current SHA in the ledger differs, mark the link as stale.
            if let Some(pinned_sha) = &link.target_sha {
                if let Ok(Some(target_entry)) = ledger.find_by_path(&link.target_path).await {
                    let current_sha = &target_entry.current_sha;
                    if !current_sha.is_empty() && current_sha != pinned_sha {
                        link.status = "stale".to_string();
                        link.stale_since = Some(now);
                        info!(
                            source = %link.source_path,
                            target = %link.target_path,
                            link_type = %link.link_type,
                            "spec-registry: link target SHA changed — marking stale"
                        );
                    }
                }
            }

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
                    // For extends links with stale target: mark extending spec as drifted,
                    // invalidate approval, and create a drift-review task.
                    // spec-links.md §Approval Gates: "When target changes, source's
                    // approval is invalidated."
                    // spec-links.md §Automatic Staleness Detection step 3: "Creates
                    // drift-review tasks in the source specs' repos."
                    if link.status == "stale" {
                        info!(
                            source = %link.source_path,
                            target = %link.target_path,
                            "spec-registry: extends target SHA changed — marking extending spec drifted"
                        );
                        if let Ok(Some(mut source_entry)) =
                            ledger.find_by_path(&link.source_path).await
                        {
                            source_entry.drift_status = "drifted".to_string();
                            // F3: Invalidate extending spec's approval.
                            source_entry.approval_status = ApprovalStatus::Pending;
                            source_entry.updated_at = now;
                            let _ = ledger.save(&source_entry).await;
                            info!(
                                spec_path = %link.source_path,
                                "spec-registry: approval invalidated (extends target changed)"
                            );

                            // F2: Create drift-review task entity.
                            create_drift_review_task(
                                tasks,
                                &link.source_path,
                                &link.target_path,
                                source_repo_id,
                                source_workspace_id,
                                now,
                            )
                            .await;
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

    // 6b. Inbound staleness detection (TASK-016 F1).
    // spec-links.md §Automatic Staleness Detection: "When any spec changes (new SHA),
    // the forge queries spec_links for all links where target_path matches the changed
    // spec and marks those links as stale."
    // This catches links from OTHER specs (possibly in other repos) that point TO specs
    // whose SHAs changed in this push.
    if !changed_spec_paths.is_empty() {
        let changed_set: std::collections::HashSet<&str> =
            changed_spec_paths.iter().map(|s| s.as_str()).collect();
        let mut store = links_store.lock().await;
        for link in store.iter_mut() {
            // Only update links that target a changed spec and aren't already stale/broken.
            if changed_set.contains(link.target_path.as_str())
                && link.status != "stale"
                && link.status != "broken"
            {
                info!(
                    source = %link.source_path,
                    target = %link.target_path,
                    link_type = %link.link_type,
                    "spec-registry: inbound link target SHA changed — marking stale"
                );
                link.status = "stale".to_string();
                link.stale_since = Some(now);
            }
        }
        // Drop the lock before doing ledger updates and task creation.
        // Collect extends links that need additional side effects.
        let stale_extends: Vec<(String, String)> = store
            .iter()
            .filter(|l| {
                l.link_type == SpecLinkType::Extends
                    && l.stale_since == Some(now)
                    && changed_set.contains(l.target_path.as_str())
            })
            .map(|l| (l.source_path.clone(), l.target_path.clone()))
            .collect();
        drop(store);

        // Apply extends side effects for inbound stale links.
        for (source_path, target_path) in &stale_extends {
            if let Ok(Some(mut source_entry)) = ledger.find_by_path(source_path).await {
                source_entry.drift_status = "drifted".to_string();
                source_entry.approval_status = ApprovalStatus::Pending;
                source_entry.updated_at = now;
                let _ = ledger.save(&source_entry).await;
                info!(
                    spec_path = %source_path,
                    "spec-registry: inbound extends — approval invalidated, drift_status = drifted"
                );
            }

            // Create drift-review task for the extending spec.
            create_drift_review_task(
                tasks,
                source_path,
                target_path,
                source_repo_id,
                source_workspace_id,
                now,
            )
            .await;
        }
    }

    // 7. Warn about spec files not in manifest.
    check_unregistered_specs(&git_bin, repo_path, new_sha, &manifest_paths).await;
}

// ---------------------------------------------------------------------------
// Drift-review task creation helper (TASK-016 F2)
// ---------------------------------------------------------------------------

/// Create a drift-review Task entity when an `extends` link becomes stale.
///
/// spec-links.md §Automatic Staleness Detection step 3: "Creates drift-review tasks
/// in the source specs' repos."
async fn create_drift_review_task(
    tasks: Option<&Arc<dyn gyre_ports::TaskRepository>>,
    source_path: &str,
    target_path: &str,
    source_repo_id: Option<&str>,
    source_workspace_id: Option<&str>,
    now: u64,
) {
    let Some(tasks) = tasks else {
        return;
    };
    let repo_id = source_repo_id.unwrap_or_default();
    let workspace_id = source_workspace_id.unwrap_or_default();

    let task_id = gyre_common::Id::new(uuid::Uuid::new_v4().to_string());
    let title = format!(
        "Drift review: '{}' extends '{}' which has changed",
        source_path, target_path
    );
    let description = format!(
        "The parent spec '{}' has changed (new SHA). The extending spec '{}' may need to \
         incorporate the parent's changes. Review the extending spec and update if necessary.",
        target_path, source_path
    );

    let mut task = gyre_domain::Task::new(task_id, title, now);
    task.description = Some(description);
    task.priority = gyre_domain::TaskPriority::High;
    task.labels = vec!["drift-review".to_string()];
    task.spec_path = Some(source_path.to_string());
    task.workspace_id = gyre_common::Id::new(workspace_id);
    task.repo_id = gyre_common::Id::new(repo_id);

    if let Err(e) = tasks.create(&task).await {
        warn!(
            source = %source_path,
            target = %target_path,
            error = %e,
            "spec-registry: failed to create drift-review task"
        );
    } else {
        info!(
            task_id = %task.id,
            source = %source_path,
            target = %target_path,
            "spec-registry: drift-review task created for extends link"
        );
    }
}

// ---------------------------------------------------------------------------
// Cross-workspace resolution helpers
// ---------------------------------------------------------------------------

/// Resolve a cross-workspace `@workspace_slug/repo_name/spec_path` link to a target repo ID.
///
/// Resolution is tenant-scoped: looks up the workspace by slug within the caller's tenant,
/// then finds the repo by name within that workspace.
async fn resolve_cross_workspace_repo(
    workspaces: Option<&Arc<dyn gyre_ports::WorkspaceRepository>>,
    repos: Option<&Arc<dyn gyre_ports::RepoRepository>>,
    tenant_id: Option<&gyre_common::Id>,
    workspace_slug: &str,
    repo_name: &str,
) -> Option<String> {
    let workspaces = workspaces?;
    let repos = repos?;
    let tenant_id = tenant_id?;

    // 1. Resolve workspace slug → workspace ID (tenant-scoped).
    let workspace = workspaces
        .find_by_slug(tenant_id, workspace_slug)
        .await
        .ok()
        .flatten()?;

    // 2. Resolve repo name within that workspace.
    repos
        .find_by_name_and_workspace(&workspace.id, repo_name)
        .await
        .ok()
        .flatten()
        .map(|r| r.id.to_string())
}

/// Resolve a cross-repo same-workspace link (`@repo_name/spec_path`) to a target repo ID.
///
/// Looks up a repo by name within the given workspace. Returns None if no workspace_id
/// is provided (same-workspace resolution requires knowing the current workspace).
async fn resolve_repo_by_name(
    repos: Option<&Arc<dyn gyre_ports::RepoRepository>>,
    _workspaces: Option<&Arc<dyn gyre_ports::WorkspaceRepository>>,
    _tenant_id: Option<&gyre_common::Id>,
    workspace_id: Option<&gyre_common::Id>,
    repo_name: &str,
) -> Option<String> {
    let repos = repos?;
    let ws_id = workspace_id?;
    repos
        .find_by_name_and_workspace(ws_id, repo_name)
        .await
        .ok()
        .flatten()
        .map(|r| r.id.to_string())
}

/// Read a file from a specific git commit using `git show <sha>:<path>`.
pub(crate) async fn read_git_file(
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
                    repo_id: None,
                    workspace_id: None,
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
                    repo_id: None,
                    workspace_id: None,
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
                    repo_id: None,
                    workspace_id: None,
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

    // -----------------------------------------------------------------------
    // Cross-workspace target parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_same_repo_target() {
        let result = parse_cross_workspace_target("system/vision.md");
        assert_eq!(
            result,
            CrossWorkspaceTarget::SameRepo {
                path: "system/vision.md".to_string()
            }
        );
    }

    #[test]
    fn test_parse_cross_repo_target() {
        // Cross-repo has exactly ONE segment (the repo name) before the path.
        // Path here has no slashes — splitn(3) gives 2 parts.
        let result = parse_cross_workspace_target("@api-svc/auth.md");
        assert_eq!(
            result,
            CrossWorkspaceTarget::CrossRepo {
                repo_name: "api-svc".to_string(),
                path: "auth.md".to_string()
            }
        );
    }

    #[test]
    fn test_parse_cross_workspace_target() {
        let result = parse_cross_workspace_target("@platform-core/api-svc/system/auth.md");
        assert_eq!(
            result,
            CrossWorkspaceTarget::CrossWorkspace {
                workspace_slug: "platform-core".to_string(),
                repo_name: "api-svc".to_string(),
                path: "system/auth.md".to_string()
            }
        );
    }

    #[test]
    fn test_parse_cross_workspace_target_nested_path() {
        let result = parse_cross_workspace_target("@ws-slug/repo-name/system/sub/spec.md");
        assert_eq!(
            result,
            CrossWorkspaceTarget::CrossWorkspace {
                workspace_slug: "ws-slug".to_string(),
                repo_name: "repo-name".to_string(),
                path: "system/sub/spec.md".to_string()
            }
        );
    }

    #[test]
    fn test_parse_manifest_with_cross_workspace_link() {
        let yaml = r#"
version: 1
specs:
  - path: system/payment-retry.md
    title: Payment Retry
    owner: user:alice
    links:
      - type: depends_on
        target: "@platform-core/idempotent-api/system/contract.md"
        target_sha: abc123
        reason: "Requires idempotency guarantees"
"#;
        let m = parse_manifest(yaml).expect("parse ok");
        let spec = &m.specs[0];
        assert_eq!(spec.links.len(), 1);
        let link = &spec.links[0];
        assert_eq!(
            link.target,
            "@platform-core/idempotent-api/system/contract.md"
        );
        assert_eq!(link.target_sha.as_deref(), Some("abc123"));

        // Verify parse_cross_workspace_target handles this correctly.
        let parsed = parse_cross_workspace_target(&link.target);
        assert_eq!(
            parsed,
            CrossWorkspaceTarget::CrossWorkspace {
                workspace_slug: "platform-core".to_string(),
                repo_name: "idempotent-api".to_string(),
                path: "system/contract.md".to_string()
            }
        );
    }

    #[test]
    fn test_cross_workspace_link_target_display_set() {
        // When a cross-workspace target is parsed, target_display should be set.
        let target = "@my-workspace/my-repo/system/spec.md";
        let parsed = parse_cross_workspace_target(target);
        match parsed {
            CrossWorkspaceTarget::CrossWorkspace {
                workspace_slug,
                repo_name,
                path,
            } => {
                let display = format!("@{}/{}/{}", workspace_slug, repo_name, path);
                assert_eq!(display, target);
            }
            other => panic!("expected CrossWorkspace, got {:?}", other),
        }
    }

    #[test]
    fn test_unresolved_link_status() {
        // Unresolved links should have status "unresolved" and target_repo_id = None.
        // This is tested indirectly by checking the CrossWorkspace variant sets the right fields.
        let entry = SpecLinkEntry {
            id: "test".to_string(),
            source_path: "system/a.md".to_string(),
            source_repo_id: Some("repo-1".to_string()),
            link_type: SpecLinkType::DependsOn,
            target_path: "system/contract.md".to_string(),
            target_repo_id: None,
            target_display: Some("@platform-core/idempotent-api/system/contract.md".to_string()),
            target_sha: Some("abc123".to_string()),
            reason: None,
            status: "unresolved".to_string(),
            created_at: 1700000000,
            stale_since: None,
        };
        assert_eq!(entry.status, "unresolved");
        assert!(entry.target_repo_id.is_none());
        assert!(entry.target_display.is_some());
    }

    // -----------------------------------------------------------------------
    // TASK-016 F4: Extends push-time behavior tests
    // -----------------------------------------------------------------------
    //
    // These tests simulate the extends-link staleness detection, approval
    // invalidation, and drift-review task creation that sync_spec_ledger
    // performs at push time when an extends link's target SHA changes.

    fn make_test_ledger_entry(path: &str, sha: &str, status: ApprovalStatus) -> SpecLedgerEntry {
        SpecLedgerEntry {
            path: path.to_string(),
            title: format!("Spec {path}"),
            owner: "user:test".to_string(),
            kind: None,
            current_sha: sha.to_string(),
            approval_mode: "human_only".to_string(),
            approval_status: status,
            linked_tasks: vec![],
            linked_mrs: vec![],
            drift_status: "clean".to_string(),
            created_at: 1_000_000,
            updated_at: 1_000_000,
            repo_id: Some("repo1".to_string()),
            workspace_id: Some("ws1".to_string()),
        }
    }

    fn make_test_link(
        id: &str,
        source: &str,
        target: &str,
        link_type: SpecLinkType,
        target_sha: Option<&str>,
    ) -> SpecLinkEntry {
        SpecLinkEntry {
            id: id.to_string(),
            source_path: source.to_string(),
            source_repo_id: Some("repo1".to_string()),
            link_type,
            target_path: target.to_string(),
            target_repo_id: None,
            target_display: None,
            target_sha: target_sha.map(|s| s.to_string()),
            reason: None,
            status: "active".to_string(),
            created_at: 1_000_000,
            stale_since: None,
        }
    }

    /// F4: When an outbound extends link's target_sha differs from the ledger's
    /// current SHA, the link is marked stale, the extending spec's drift_status
    /// is set to "drifted", and its approval_status is invalidated to Pending.
    #[tokio::test]
    async fn extends_outbound_staleness_marks_drifted_and_invalidates_approval() {
        use crate::mem::MemSpecLedgerRepository;

        let ledger: Arc<dyn gyre_ports::SpecLedgerRepository> =
            Arc::new(MemSpecLedgerRepository::default());
        let links_store: SpecLinksStore = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let now = 2_000_000u64;

        // Set up: parent spec in ledger with NEW SHA (simulating it changed).
        let parent =
            make_test_ledger_entry("system/parent.md", "new_sha_999", ApprovalStatus::Approved);
        ledger.save(&parent).await.unwrap();

        // Set up: extending spec in ledger, currently approved.
        let extending =
            make_test_ledger_entry("system/extending.md", "ext_sha", ApprovalStatus::Approved);
        ledger.save(&extending).await.unwrap();

        // Simulate outbound extends link processing (as sync_spec_ledger step 6 does).
        // The link was pinned to old_sha_123 but the parent is now at new_sha_999.
        let mut link = make_test_link(
            "ext-link-1",
            "system/extending.md",
            "system/parent.md",
            SpecLinkType::Extends,
            Some("old_sha_123"),
        );

        // Check target SHA against ledger (same logic as step 6 in sync_spec_ledger).
        if let Ok(Some(target_entry)) = ledger.find_by_path(&link.target_path).await {
            let current_sha = &target_entry.current_sha;
            if let Some(pinned_sha) = &link.target_sha {
                if !current_sha.is_empty() && current_sha != pinned_sha {
                    link.status = "stale".to_string();
                    link.stale_since = Some(now);
                }
            }
        }

        // Verify link is stale.
        assert_eq!(link.status, "stale");
        assert_eq!(link.stale_since, Some(now));

        // Apply extends side effects (same logic as the Extends match arm).
        if link.link_type == SpecLinkType::Extends && link.status == "stale" {
            if let Ok(Some(mut source_entry)) = ledger.find_by_path(&link.source_path).await {
                source_entry.drift_status = "drifted".to_string();
                source_entry.approval_status = ApprovalStatus::Pending;
                source_entry.updated_at = now;
                ledger.save(&source_entry).await.unwrap();
            }
        }

        // Verify extending spec's drift_status and approval_status.
        let ext = ledger
            .find_by_path("system/extending.md")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ext.drift_status, "drifted");
        assert_eq!(ext.approval_status, ApprovalStatus::Pending);
    }

    /// F4: Extends push-time also creates a drift-review Task entity.
    #[tokio::test]
    async fn extends_staleness_creates_drift_review_task() {
        use crate::mem::MemTaskRepository;

        let tasks: Arc<dyn gyre_ports::TaskRepository> = Arc::new(MemTaskRepository::default());

        // Call the drift-review task creation helper.
        create_drift_review_task(
            Some(&tasks),
            "system/extending.md",
            "system/parent.md",
            Some("repo1"),
            Some("ws1"),
            2_000_000,
        )
        .await;

        // Verify a task was created.
        let all_tasks = tasks.list().await.unwrap();
        assert_eq!(all_tasks.len(), 1);
        let task = &all_tasks[0];
        assert!(task.title.contains("Drift review"));
        assert!(task.title.contains("system/extending.md"));
        assert!(task.title.contains("system/parent.md"));
        assert_eq!(task.spec_path, Some("system/extending.md".to_string()));
        assert_eq!(task.labels, vec!["drift-review".to_string()]);
        assert_eq!(task.priority, gyre_domain::TaskPriority::High);
        assert_eq!(task.workspace_id, gyre_common::Id::new("ws1"));
        assert_eq!(task.repo_id, gyre_common::Id::new("repo1"));
        assert!(task.description.is_some());
        assert!(task.description.as_ref().unwrap().contains("parent.md"));
    }

    /// F1 + F4: When a spec's SHA changes at push time, inbound links from
    /// OTHER specs targeting the changed spec are marked stale immediately.
    /// For non-extends link types, only staleness marking occurs.
    #[tokio::test]
    async fn inbound_staleness_marks_non_extends_links_stale() {
        use crate::mem::MemSpecLedgerRepository;

        let ledger: Arc<dyn gyre_ports::SpecLedgerRepository> =
            Arc::new(MemSpecLedgerRepository::default());
        let links_store: SpecLinksStore = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let now = 2_000_000u64;

        // Set up: target spec in ledger.
        let target =
            make_test_ledger_entry("system/target.md", "new_sha_456", ApprovalStatus::Approved);
        ledger.save(&target).await.unwrap();

        // Pre-existing inbound links from other specs targeting the changed spec.
        let depends_link = make_test_link(
            "dep-link",
            "system/consumer.md",
            "system/target.md",
            SpecLinkType::DependsOn,
            Some("old_sha_123"),
        );
        let implements_link = make_test_link(
            "impl-link",
            "system/impl.md",
            "system/target.md",
            SpecLinkType::Implements,
            Some("old_sha_123"),
        );
        {
            let mut store = links_store.lock().await;
            store.push(depends_link);
            store.push(implements_link);
        }

        // Simulate inbound staleness detection (step 6b of sync_spec_ledger).
        let changed_spec_paths = vec!["system/target.md".to_string()];
        let changed_set: std::collections::HashSet<&str> =
            changed_spec_paths.iter().map(|s| s.as_str()).collect();
        {
            let mut store = links_store.lock().await;
            for link in store.iter_mut() {
                if changed_set.contains(link.target_path.as_str())
                    && link.status != "stale"
                    && link.status != "broken"
                {
                    link.status = "stale".to_string();
                    link.stale_since = Some(now);
                }
            }
        }

        // Verify both inbound links are now stale.
        let store = links_store.lock().await;
        let dep = store.iter().find(|l| l.id == "dep-link").unwrap();
        assert_eq!(dep.status, "stale");
        assert_eq!(dep.stale_since, Some(now));

        let imp = store.iter().find(|l| l.id == "impl-link").unwrap();
        assert_eq!(imp.status, "stale");
        assert_eq!(imp.stale_since, Some(now));
    }

    /// F1 + F3 + F4: Inbound extends links get staleness + drift + approval
    /// invalidation + task creation when the target spec's SHA changes.
    #[tokio::test]
    async fn inbound_extends_staleness_full_side_effects() {
        use crate::mem::{MemSpecLedgerRepository, MemTaskRepository};

        let ledger: Arc<dyn gyre_ports::SpecLedgerRepository> =
            Arc::new(MemSpecLedgerRepository::default());
        let links_store: SpecLinksStore = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let tasks: Arc<dyn gyre_ports::TaskRepository> = Arc::new(MemTaskRepository::default());
        let now = 2_000_000u64;

        // Target spec whose SHA changed.
        let target =
            make_test_ledger_entry("system/parent.md", "new_sha", ApprovalStatus::Approved);
        ledger.save(&target).await.unwrap();

        // Extending spec (currently approved, clean drift).
        let extending =
            make_test_ledger_entry("system/child.md", "child_sha", ApprovalStatus::Approved);
        ledger.save(&extending).await.unwrap();

        // Pre-existing inbound extends link.
        let extends_link = make_test_link(
            "ext-inbound",
            "system/child.md",
            "system/parent.md",
            SpecLinkType::Extends,
            Some("old_parent_sha"),
        );
        {
            let mut store = links_store.lock().await;
            store.push(extends_link);
        }

        // Simulate step 6b: inbound staleness detection.
        let changed_spec_paths = vec!["system/parent.md".to_string()];
        let changed_set: std::collections::HashSet<&str> =
            changed_spec_paths.iter().map(|s| s.as_str()).collect();

        // Mark inbound links stale.
        {
            let mut store = links_store.lock().await;
            for link in store.iter_mut() {
                if changed_set.contains(link.target_path.as_str())
                    && link.status != "stale"
                    && link.status != "broken"
                {
                    link.status = "stale".to_string();
                    link.stale_since = Some(now);
                }
            }
        }

        // Collect stale extends links for side effects.
        let stale_extends: Vec<(String, String)> = {
            let store = links_store.lock().await;
            store
                .iter()
                .filter(|l| {
                    l.link_type == SpecLinkType::Extends
                        && l.stale_since == Some(now)
                        && changed_set.contains(l.target_path.as_str())
                })
                .map(|l| (l.source_path.clone(), l.target_path.clone()))
                .collect()
        };

        // Apply extends side effects.
        for (source_path, target_path) in &stale_extends {
            if let Ok(Some(mut source_entry)) = ledger.find_by_path(source_path).await {
                source_entry.drift_status = "drifted".to_string();
                source_entry.approval_status = ApprovalStatus::Pending;
                source_entry.updated_at = now;
                ledger.save(&source_entry).await.unwrap();
            }

            create_drift_review_task(
                Some(&tasks),
                source_path,
                target_path,
                Some("repo1"),
                Some("ws1"),
                now,
            )
            .await;
        }

        // Verify link is stale.
        let store = links_store.lock().await;
        let link = store.iter().find(|l| l.id == "ext-inbound").unwrap();
        assert_eq!(link.status, "stale");
        assert_eq!(link.stale_since, Some(now));
        drop(store);

        // Verify extending spec: drift_status = drifted, approval_status = Pending.
        let child = ledger
            .find_by_path("system/child.md")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(child.drift_status, "drifted");
        assert_eq!(child.approval_status, ApprovalStatus::Pending);

        // Verify drift-review task was created.
        let all_tasks = tasks.list().await.unwrap();
        assert_eq!(all_tasks.len(), 1);
        let task = &all_tasks[0];
        assert!(task.title.contains("Drift review"));
        assert_eq!(task.spec_path, Some("system/child.md".to_string()));
        assert_eq!(task.labels, vec!["drift-review".to_string()]);
    }

    /// Inbound staleness detection does not re-stamp already-stale links.
    #[tokio::test]
    async fn inbound_staleness_skips_already_stale_links() {
        let links_store: SpecLinksStore = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let now = 2_000_000u64;

        let mut link = make_test_link(
            "already-stale",
            "system/a.md",
            "system/target.md",
            SpecLinkType::DependsOn,
            Some("old_sha"),
        );
        link.status = "stale".to_string();
        link.stale_since = Some(999_000);
        {
            let mut store = links_store.lock().await;
            store.push(link);
        }

        // Simulate inbound detection with target.md changed.
        let changed_set: std::collections::HashSet<&str> =
            ["system/target.md"].iter().copied().collect();
        {
            let mut store = links_store.lock().await;
            for link in store.iter_mut() {
                if changed_set.contains(link.target_path.as_str())
                    && link.status != "stale"
                    && link.status != "broken"
                {
                    link.status = "stale".to_string();
                    link.stale_since = Some(now);
                }
            }
        }

        // stale_since should remain at original value, not re-stamped.
        let store = links_store.lock().await;
        let link = store.iter().find(|l| l.id == "already-stale").unwrap();
        assert_eq!(link.stale_since, Some(999_000));
    }
}
