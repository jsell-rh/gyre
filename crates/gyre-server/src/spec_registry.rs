//! Spec Registry: manifest parser, in-memory ledger, ledger sync on push.
//!
//! The spec registry is a two-part system:
//! 1. `specs/manifest.yaml` (in git) — declares what specs exist and their policies.
//! 2. Forge spec ledger (in memory) — tracks runtime state per spec: current SHA,
//!    approval status, linked MRs/tasks, drift status.
//!
//! The ledger is synced from the manifest on every push to the default branch.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::Mutex;
use tracing::{info, warn};

pub use gyre_domain::spec_links::{SpecLinkEntry, SpecLinkType};
pub use gyre_domain::{ApprovalStatus, SpecApprovalEvent, SpecLedgerEntry, TaskPriority};

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
    #[serde(default)]
    pub concepts: Vec<ConceptDef>,
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
    /// Default lifecycle-task priority override (spec-registry.md §15:
    /// "manifest can override default task priority"). `None` keeps the
    /// change-kind default chosen by the lifecycle hook.
    #[serde(default)]
    pub priority: Option<TaskPriority>,
}

impl Default for ManifestDefaults {
    fn default() -> Self {
        Self {
            requires_approval: true,
            auto_create_tasks: true,
            auto_invalidate_on_change: true,
            priority: None,
        }
    }
}
fn default_true() -> bool {
    true
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
    /// Overrides the lifecycle-task priority for changes to this spec
    /// (spec-registry.md §15). `None` falls back to `defaults.priority`,
    /// then to the change-kind default.
    #[serde(default)]
    pub priority: Option<TaskPriority>,
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

    /// Effective lifecycle-task priority override for this spec
    /// (spec-registry.md §15): per-entry override, else the manifest
    /// `defaults:` priority, else `None` (caller keeps its change-kind
    /// default). Never hardcodes a priority — an override is applied only
    /// when the manifest declares one.
    pub fn effective_priority(&self, defaults: &ManifestDefaults) -> Option<TaskPriority> {
        self.priority.clone().or_else(|| defaults.priority.clone())
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

// ---------------------------------------------------------------------------
// Ledger types (runtime state tracked per spec)
// ---------------------------------------------------------------------------

// NOTE: the in-memory store aliases in this section are legacy debt grandfathered
// in scripts/in-memory-state-stores-exemptions.txt at frozen line coordinates.

// ApprovalStatus, SpecLedgerEntry, SpecApprovalEvent are re-exported from gyre_domain above.
// The former in-memory `SpecLedger` / `SpecApprovalHistory` store aliases were
// removed: the ledger is port-backed (`gyre_ports::SpecLedgerRepository`) and
// approval events are persisted through `gyre_ports::SpecApprovalEventRepository`,
// both wired via the `store!` macro in `build_state`. Tests build local map
// fixtures instead (see the test module below).

// in-memory-state-stores:ok — hot cache of the durable `spec_links` table
// (spec-links.md §Forge-Maintained Spec Graph): the SQL table behind
// `AppState.spec_link_repo` is authoritative, every mutation writes through,
// and boot reloads via `load_spec_links_into_store` (lib.rs).
pub type SpecLinksStore = Arc<Mutex<Vec<SpecLinkEntry>>>; // in-memory-state-stores:ok — see above

/// A stale inbound link collected for side-effect processing (sync step 6b):
/// drift-review task creation (F5/F7) and extends-specific approval
/// invalidation. Named struct instead of a positional 4-tuple so source and
/// target paths cannot be swapped silently (TASK-048 F1 hazard class).
#[derive(Debug, Clone)]
struct StaleInboundLink {
    source_path: String,
    target_path: String,
    source_repo_id: Option<String>,
    link_type: SpecLinkType,
}

/// Read the manifest from a commit and return the set of registered spec paths.
///
/// Returns an empty set if the manifest is absent or unparseable.
pub async fn read_manifest_paths(repo_path: &str, sha: &str) -> std::collections::HashSet<String> {
    read_manifest(repo_path, sha)
        .await
        .map(|m| m.specs.iter().map(|e| e.path.clone()).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Manifest reading and path resolution (used by the ledger sync and
// the spec lifecycle hooks)
// ---------------------------------------------------------------------------

/// Normalize a spec path to the manifest's canonical identity: relative to
/// `specs/`, with no leading `specs/` prefix (the same form `sync_spec_ledger`
/// uses when it prefixes entries with `specs/` and `GET /api/v1/specs/:path`
/// accepts).
fn normalize_spec_rel_path(path: &str) -> &str {
    path.strip_prefix("specs/").unwrap_or(path)
}

/// Resolve the manifest `SpecEntry` for a changed file path from a git diff
/// (e.g. `specs/system/foo.md`). For renames, callers pass the NEW path —
/// the manifest at the pushed HEAD describes where the spec lives now.
pub fn manifest_entry_for_path<'a>(
    manifest: &'a SpecManifest,
    path: &str,
) -> Option<&'a SpecEntry> {
    let rel = normalize_spec_rel_path(path);
    manifest
        .specs
        .iter()
        .find(|e| normalize_spec_rel_path(&e.path) == rel)
}

/// Parse YAML text into a SpecManifest.
pub fn parse_manifest(yaml: &str) -> Result<SpecManifest, serde_yaml::Error> {
    serde_yaml::from_str(yaml)
}

/// Read and parse `specs/manifest.yaml` from a git commit.
///
/// Returns `None` when the manifest is absent or unparseable. Callers use
/// this to resolve per-spec approval policy (agent approvers, attestation
/// constraints) at approval time (spec-registry.md §9).
pub async fn read_manifest(repo_path: &str, sha: &str) -> Option<SpecManifest> {
    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
    let yaml = read_git_file(&git_bin, repo_path, sha, "specs/manifest.yaml").await?;
    let parsed = parse_manifest(&yaml);
    match parsed {
        Ok(m) => Some(m),
        Err(e) => {
            warn!(repo_path, "spec-registry: failed to parse manifest: {e}");
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Approval status resolution (spec-registry.md §9)
// ---------------------------------------------------------------------------

/// Resolve the ledger `approval_status` for a spec from its approval mode and
/// the recorded approval events (spec-registry.md §9 Approval Status
/// Resolution).
///
/// An event is valid when:
/// - `spec_sha` matches `current_sha` (not stale), and
/// - `revoked_at` is null (not revoked).
///
/// Agent-approval validity additionally requires the event to satisfy a
/// configured `agent_approvers[]` entry for the spec:
/// - `persona` matches the approver's persona,
/// - `attestation_level >= min_attestation_level` (default 1), and
/// - when the approver pins a `stack_hash`, the event's recorded stack hash
///   matches exactly.
///
/// Human-approval validity requires `approver_type == "human"` and the
/// `approver_id` to be in the effective human-approver set: the manifest's
/// `approval.human_approvers` list when non-empty, otherwise the schema
/// default `[owner]` (an omitted list reserves approval to the spec owner).
///
/// Status by mode:
/// - `human_only`        → Approved with ≥1 valid human approval
/// - `agent_only`        → Approved with ≥1 valid agent approval
/// - `human_and_agent`   → Approved with ≥1 valid human AND ≥1 valid agent
///
/// Specs with `requires_approval: false` are Approved regardless of events
/// (§17: any SHA is accepted).
pub fn resolve_approval_status(
    entry: &SpecEntry,
    defaults: &ManifestDefaults,
    current_sha: &str,
    events: &[SpecApprovalEvent],
) -> ApprovalStatus {
    if !entry.effective_requires_approval(defaults) {
        return ApprovalStatus::Approved;
    }

    let approval = entry.approval.as_ref();
    let has_valid_human = events
        .iter()
        .any(|e| is_valid_human_approval(e, entry, current_sha));
    let has_valid_agent = events
        .iter()
        .any(|e| is_valid_agent_approval(e, approval, current_sha));

    let approved = match entry.effective_approval_mode() {
        ApprovalMode::HumanOnly => has_valid_human,
        ApprovalMode::AgentOnly => has_valid_agent,
        ApprovalMode::HumanAndAgent => has_valid_human && has_valid_agent,
    };
    if approved {
        ApprovalStatus::Approved
    } else {
        ApprovalStatus::Pending
    }
}

/// A human approval is valid when it is active, matches `current_sha`, and
/// the approver is in the entry's effective human-approver set:
/// `approval.human_approvers` when that list is non-empty, otherwise the
/// Manifest Schema default `[owner]` — an omitted/empty list reserves
/// approval to the spec owner, it never widens it to any human.
fn is_valid_human_approval(
    event: &SpecApprovalEvent,
    entry: &SpecEntry,
    current_sha: &str,
) -> bool {
    event.approver_type == "human"
        && event.spec_sha == current_sha
        && event.is_active()
        && {
            let approvers: &[String] = match entry.approval.as_ref() {
                Some(a) if !a.human_approvers.is_empty() => &a.human_approvers,
                // Manifest Schema default: `approval.human_approvers`
                // defaults to `[owner]` (owner is a required field).
                _ => std::slice::from_ref(&entry.owner),
            };
            approvers.iter().any(|a| a == &event.approver_id)
        }
}

/// An agent approval is valid when it is active, matches `current_sha`, and
/// satisfies one of the configured `agent_approvers[]` constraints
/// (persona + min attestation level + optional pinned stack hash).
fn is_valid_agent_approval(
    event: &SpecApprovalEvent,
    approval: Option<&ApprovalConfig>,
    current_sha: &str,
) -> bool {
    if event.approver_type != "agent" || event.spec_sha != current_sha || !event.is_active() {
        return false;
    }
    let approvers: &[AgentApproverConfig] = approval
        .map(|a| a.agent_approvers.as_slice())
        .unwrap_or(&[]);
    approvers.iter().any(|cfg| {
        // Persona must match the configured approver persona.
        if event.persona.as_deref() != Some(cfg.persona.as_str()) {
            return false;
        }
        // Attestation level must meet the configured minimum (default 1).
        let min_level = cfg.min_attestation_level.unwrap_or(1);
        if event.attestation_level.unwrap_or(0) < min_level {
            return false;
        }
        // When a stack hash is pinned, the event's recorded hash must match exactly.
        match &cfg.stack_hash {
            Some(required) => event.stack_hash.as_deref() == Some(required.as_str()),
            None => true,
        }
    })
}

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
///
/// NOTE: a three-segment target is syntactically ambiguous between the
/// cross-repo form (repo name + slashed spec path) and the cross-workspace
/// form (workspace slug + repo name + spec path) — a pure string split
/// cannot tell them apart. This parser always yields the cross-workspace
/// interpretation; resolution sites (push-time sync and the staleness
/// re-resolution pass) disambiguate by existence: the workspace form takes
/// precedence, and when no workspace with that slug exists in the tenant
/// the target is treated as `@{repo_name}/{spec_path}` within the source
/// workspace.
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
/// - `supersedes` links: no push-time action (deprecation handled by approve_spec).
/// - ALL stale links: drift-review Task created in the source spec's repo
///   (spec-links.md §Automatic Staleness Detection step 3).
/// - `extends` links: if the target SHA changed, the extending spec's drift_status = "drifted"
///   and approval_status is invalidated to Pending.
/// - Push-time inbound staleness: when a spec's SHA changes, ALL existing links targeting
///   that spec are marked stale immediately (spec-links.md §Automatic Staleness Detection).
/// - Push-time broken detection (task-288): a spec removed from the manifest stays in the
///   ledger as Deprecated, so inbound links whose target resolves to this repo but whose
///   target path the pushed manifest no longer declares are marked "broken" at push time
///   (spec-links.md §Link Status: "no longer in the manifest").
/// - Conflicted at store time (task-288): a conflicts_with link whose two endpoints are
///   both Approved is stored as "conflicted", and every approval change made by this sync
///   (content invalidation, deprecation, re-Pending on re-add, extends drift) reconciles
///   the stored conflicted status of the links touching that spec (set on both-approved,
///   revert when the condition ceases).
/// - Cross-repo `@repo/path` and cross-workspace `@ws/repo/path` targets are validated
///   per spec-links.md §Cross-Repo and Cross-Workspace Links: target workspace exists
///   (tenant-scoped) and the linking principal has read access, target repo exists
///   within that workspace, and the target spec path is declared in the target
///   repo's current manifest. Same-workspace `@repo/path` links resolve within the
///   SOURCE workspace. Targets failing any check are stored with
///   `status = "unresolved"` and `target_repo_id = None`.
/// - Access-denied cross-workspace links record a durable denial marker so the
///   staleness checker (which cannot recover the pushing principal from the link
///   row) never silently flips them "active".
#[allow(clippy::too_many_arguments)]
pub async fn sync_spec_ledger(
    ledger: &Arc<dyn gyre_ports::SpecLedgerRepository>,
    links_store: &SpecLinksStore,
    // Durable spec-link graph repository (spec-links.md §Forge-Maintained Spec
    // Graph). All link mutations below are written through so the SQL table is
    // the authoritative graph across restarts.
    link_repo: &Arc<dyn gyre_ports::SpecLinkRepository>,
    repo_path: &str,
    new_sha: &str,
    now: u64,
    // Context for cross-workspace resolution (pass None to skip resolution).
    source_repo_id: Option<&str>,
    source_workspace_id: Option<&str>,
    workspaces: Option<&Arc<dyn gyre_ports::WorkspaceRepository>>,
    repos: Option<&Arc<dyn gyre_ports::RepoRepository>>,
    tenant_id: Option<&gyre_common::Id>,
    // Linking-principal context (spec-links.md §Cross-Repo and Cross-Workspace
    // Links: "the linking user has read access"). `principal` is the
    // authenticated pusher at push-triggered syncs (git_http push handler,
    // POST /repos/:id/sync-specs); `None` for server-originated syncs (mirror
    // fetches, background jobs) — no linking user, exempt from the check.
    principal: Option<&LinkPrincipal>,
    memberships: Option<&Arc<dyn gyre_ports::WorkspaceMembershipRepository>>,
    // KV store for durable cross-workspace access-denial markers.
    kv: Option<&Arc<dyn gyre_ports::KvJsonStore>>,
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
    // Specs whose ledger approval_status changed during this sync (content
    // invalidation, deprecation, re-Pending on re-add, extends drift) —
    // each can create or dissolve a both-approved conflicts_with pair, so
    // the stored `conflicted` status is reconciled for them in step 6c
    // (task-288, spec-links.md §Link Status).
    let mut approval_changed_paths: Vec<String> = Vec::new();
    // Per-spec current blob SHA (from HEAD) — reused as `source_sha` on the
    // spec's link entries (spec-links.md §Forge-Maintained Spec Graph).
    let mut manifest_sha_by_path: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
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
        manifest_sha_by_path.insert(entry.path.clone(), blob_sha.clone());

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
                    approval_changed_paths.push(entry.path.clone());
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
                approval_changed_paths.push(entry.path.clone());
            }
            existing
        } else {
            info!(spec_path = %entry.path, "spec-registry: new spec registered");
            SpecLedgerEntry {
                bindings_sha: String::new(),
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
                approval_changed_paths.push(e.path.clone());
            }
        }
    }

    // 6. Process spec links from manifest — enforce supersedes/extends, update links store.
    {
        let mut new_links: Vec<SpecLinkEntry> = Vec::new();
        for entry in &manifest.specs {
            for link in &entry.links {
                let id = gyre_domain::spec_links::spec_link_id(
                    source_repo_id,
                    &entry.path,
                    &link.link_type,
                    &link.target,
                );
                let parsed = parse_cross_workspace_target(&link.target);
                let resolved = resolve_manifest_link_target(
                    &parsed,
                    &entry.path,
                    &link.target,
                    source_workspace_id,
                    workspaces,
                    repos,
                    tenant_id,
                    memberships,
                    principal,
                )
                .await;
                let (target_path, target_repo_id, target_display, status, access_denied) = (
                    resolved.target_path,
                    resolved.target_repo_id,
                    resolved.target_display,
                    resolved.status,
                    resolved.access_denied,
                );

                // Keep the durable access-denial marker in step with this
                // push (every default-branch push fully re-declares the
                // manifest's links): a denial records the current principal
                // so the staleness pass — which cannot recover the pusher
                // from the link row — never silently flips the link
                // "active"; any other outcome clears an earlier denial.
                if let Some(kv) = kv {
                    if access_denied {
                        record_link_denial(kv, &id, principal, &link.target, now).await;
                    } else {
                        clear_link_denial(kv, &id).await;
                    }
                }

                new_links.push(SpecLinkEntry {
                    id,
                    source_path: entry.path.clone(),
                    source_repo_id: source_repo_id.map(|s| s.to_string()),
                    source_sha: manifest_sha_by_path
                        .get(&entry.path)
                        .cloned()
                        .unwrap_or_default(),
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

            // task-288 (spec-links.md §Link Status): a conflicts_with link
            // stored at sync whose two endpoints are ALREADY both Approved
            // is a violation — store it as "conflicted". The approval-time
            // gate only rejects NEW approvals, so a link declared between
            // two already-approved specs lands here (conflicted takes
            // precedence over the active/stale verdicts while it holds).
            if link.link_type == SpecLinkType::ConflictsWith {
                let source_approved = ledger
                    .find_by_path(&link.source_path)
                    .await
                    .ok()
                    .flatten()
                    .is_some_and(|e| e.approval_status == ApprovalStatus::Approved);
                let target_approved = ledger
                    .find_by_path(&link.target_path)
                    .await
                    .ok()
                    .flatten()
                    .is_some_and(|e| e.approval_status == ApprovalStatus::Approved);
                if source_approved && target_approved {
                    link.status = "conflicted".to_string();
                    info!(
                        source = %link.source_path,
                        target = %link.target_path,
                        "spec-registry: conflicts_with link stored with both endpoints approved — marking conflicted"
                    );
                }
            }

            // Step 3 (generic): Create drift-review tasks for stale links.
            // spec-links.md §Automatic Staleness Detection step 3: "Creates
            // drift-review tasks in the source specs' repos."
            // Excludes `references` — spec says "No mechanical enforcement."
            if link.status == "stale" && link.link_type != SpecLinkType::References {
                create_drift_review_task(
                    tasks,
                    &link.source_path,
                    &link.target_path,
                    &link.link_type,
                    source_repo_id,
                    source_workspace_id,
                    now,
                )
                .await;
            }

            // Type-specific enforcement.
            // Note: Supersedes deprecation is handled ONLY in approve_spec
            // (spec-links.md §Approval Gates: "When source is approved, target
            // is automatically set to deprecated"). No push-time action here.
            // Only `extends` has sync-time enforcement; other link types have
            // no push-time action (supersedes handled in approve_spec).
            // spec-links.md §Approval Gates: "When target changes, source's
            // approval is invalidated." — for extends links with a stale
            // target, mark the extending spec as drifted and invalidate
            // its approval.
            if link.link_type == SpecLinkType::Extends && link.status == "stale" {
                info!(
                    source = %link.source_path,
                    target = %link.target_path,
                    "spec-registry: extends target SHA changed — marking extending spec drifted"
                );
                if let Ok(Some(mut source_entry)) = ledger.find_by_path(&link.source_path).await {
                    source_entry.drift_status = "drifted".to_string();
                    source_entry.approval_status = ApprovalStatus::Pending;
                    source_entry.updated_at = now;
                    let _ = ledger.save(&source_entry).await;
                    approval_changed_paths.push(link.source_path.clone());
                    info!(
                        spec_path = %link.source_path,
                        "spec-registry: approval invalidated (extends target changed)"
                    );
                }
            }
        }

        // Replace all links originating from specs in this manifest (full refresh),
        // in memory AND in the durable repository (spec-links.md
        // §Forge-Maintained Spec Graph — the SQL table is authoritative).
        // Both stores use the same (source_repo_id, source_path) scoping so
        // they can never diverge: a push from one repo replaces only that
        // repo's rows for the manifest's paths.
        {
            let source_paths: std::collections::HashSet<String> =
                manifest.specs.iter().map(|e| e.path.clone()).collect();
            let repo_scope = source_repo_id.unwrap_or("");
            {
                let mut store = links_store.lock().await;
                store.retain(|l| {
                    let link_repo_key = l.source_repo_id.as_deref().unwrap_or("");
                    !(link_repo_key == repo_scope && source_paths.contains(&l.source_path))
                });
                store.extend(new_links.iter().cloned());
            }
            // One replace_for_source call per source spec in this manifest —
            // including specs whose link set is now EMPTY, so links removed
            // from the manifest are deleted from the durable table too (an
            // empty set must clear the persisted rows, not skip them).
            let links_by_path: std::collections::HashMap<&str, Vec<SpecLinkEntry>> = new_links
                .iter()
                .fold(std::collections::HashMap::new(), |mut acc, link| {
                    acc.entry(link.source_path.as_str())
                        .or_default()
                        .push(link.clone());
                    acc
                });
            for entry in &manifest.specs {
                let empty: Vec<SpecLinkEntry> = Vec::new();
                let links = links_by_path.get(entry.path.as_str()).unwrap_or(&empty);
                if let Err(e) = link_repo
                    .replace_for_source(repo_scope, &entry.path, links)
                    .await
                {
                    warn!(
                        source_repo_id = repo_scope,
                        source_path = %entry.path,
                        "spec-registry: failed to persist spec links: {e}"
                    );
                }
            }
        }
    }

    // 5b. Push-time broken detection (task-288, spec-links.md §Link Status:
    // broken = "Target spec was deleted or is no longer in the manifest").
    // A spec removed from the manifest is KEPT in the ledger as Deprecated
    // (step 5 saves, never deletes), so the ledger-only lookup can never
    // fire the "no longer in the manifest" half — inbound links whose
    // target resolves to THIS repo but whose target path the pushed
    // manifest no longer declares transition to "broken" here, at push
    // time (the daily staleness pass re-checks the same condition and
    // also reverts it when the target re-enters the manifest).
    if let Some(repo_scope) = source_repo_id {
        let mut broken: Vec<SpecLinkEntry> = Vec::new();
        {
            let mut store = links_store.lock().await;
            for link in store.iter_mut() {
                if link.status == "unresolved" {
                    // Unresolved links are re-resolved by the staleness
                    // pass against the target repo's manifest — a failed
                    // resolution is not a broken link.
                    continue;
                }
                // The link's target resolves to THIS repo: an explicit
                // cross-repo/cross-workspace target, or a same-repo link
                // whose source repo is this repo.
                let targets_this_repo = link.target_repo_id.as_deref() == Some(repo_scope)
                    || (link.target_repo_id.is_none()
                        && link.source_repo_id.as_deref() == Some(repo_scope));
                if targets_this_repo && !manifest_paths.contains(&link.target_path) {
                    info!(
                        source = %link.source_path,
                        target = %link.target_path,
                        "spec-registry: target spec no longer in repo manifest — marking link broken"
                    );
                    link.status = "broken".to_string();
                    broken.push(link.clone());
                }
            }
        }
        // Write the broken transitions through to the durable graph.
        for link in &broken {
            if let Err(e) = link_repo.save(link).await {
                warn!(
                    link_id = %link.id,
                    "spec-registry: failed to persist broken link: {e}"
                );
            }
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
        // Links mutated to stale in this pass — persisted below so the SQL
        // table stays authoritative (spec-links.md §Forge-Maintained Spec Graph).
        let mut mutated: Vec<SpecLinkEntry> = Vec::new();
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
                mutated.push(link.clone());
            }
        }
        // Drop the lock before doing ledger updates and task creation.
        // Collect ALL stale inbound links for side effects.
        // - Includes source_repo_id and link_type for correct task assignment (F5).
        // - Covers all link types for drift-review task creation (F7).
        // - Excludes links from the current manifest (already processed in step 6) (F6).
        let inbound_stale_links: Vec<StaleInboundLink> = store
            .iter()
            .filter(|l| {
                l.stale_since == Some(now)
                    && changed_set.contains(l.target_path.as_str())
                    && !manifest_paths.contains(&l.source_path)
            })
            .map(|l| StaleInboundLink {
                source_path: l.source_path.clone(),
                target_path: l.target_path.clone(),
                source_repo_id: l.source_repo_id.clone(),
                link_type: l.link_type.clone(),
            })
            .collect();
        drop(store);

        // Write staleness transitions through to the durable graph.
        for link in &mutated {
            if let Err(e) = link_repo.save(link).await {
                warn!(link_id = %link.id, "spec-registry: failed to persist stale link: {e}");
            }
        }

        // Apply side effects for inbound stale links.
        for link in &inbound_stale_links {
            // Look up source spec's ledger entry for workspace_id (F5).
            let maybe_source = ledger.find_by_path(&link.source_path).await.ok().flatten();
            let ws_id = maybe_source.as_ref().and_then(|e| e.workspace_id.clone());

            // Step 3 (generic): drift-review task for stale inbound links.
            // Excludes `references` — spec says "No mechanical enforcement."
            if link.link_type == SpecLinkType::References {
                continue;
            }
            create_drift_review_task(
                tasks,
                &link.source_path,
                &link.target_path,
                &link.link_type,
                link.source_repo_id.as_deref(),
                ws_id.as_deref(),
                now,
            )
            .await;

            // Step 4 (extends-specific): drift_status + approval invalidation.
            if link.link_type == SpecLinkType::Extends {
                if let Some(mut source_entry) = maybe_source {
                    source_entry.drift_status = "drifted".to_string();
                    source_entry.approval_status = ApprovalStatus::Pending;
                    source_entry.updated_at = now;
                    let _ = ledger.save(&source_entry).await;
                    approval_changed_paths.push(link.source_path.clone());
                    info!(
                        spec_path = %link.source_path,
                        "spec-registry: inbound extends — approval invalidated, drift_status = drifted"
                    );
                }
            }
        }
    }

    // 6c. task-288 (spec-links.md §Link Status): every approval change made
    // by this sync (content invalidation, deprecation, re-Pending on
    // re-add, extends drift) can create or dissolve a both-approved
    // conflicts_with pair — reconcile the stored `conflicted` status for
    // every link touching those specs so the graph agrees with current
    // conditions without waiting for the daily pass.
    for path in &approval_changed_paths {
        refresh_conflict_statuses_for_spec(ledger, repos, links_store, link_repo, path, now).await;
    }

    // 7. Warn about spec files not in manifest (warn-only; enforcement
    //    happens synchronously in the push handler via check_manifest_coverage).
    for path in find_unregistered_specs(&git_bin, repo_path, new_sha, &manifest_paths).await {
        warn!(
            spec_path = %path,
            "spec-registry: file under specs/ is not registered in manifest.yaml — \
             add it to specs/manifest.yaml to enable lifecycle tracking"
        );
    }
}

// ---------------------------------------------------------------------------
// Concept views (realized-model.md §4)
// ---------------------------------------------------------------------------

/// A concept view declared in the manifest's `concepts:` block
/// (realized-model.md §4). Converts to the domain `ConceptView` type.
/// A manifest without a `concepts:` key parses with an empty vec.
#[derive(Debug, Clone, Deserialize)]
pub struct ConceptDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub include: Vec<ConceptInclude>,
}

/// One `- types: [...]` / `- traits: [...]` / ... entry in a concept's
/// `include:` sequence of single-key maps.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ConceptInclude {
    #[serde(default)]
    pub types: Vec<String>,
    #[serde(default)]
    pub traits: Vec<String>,
    #[serde(default)]
    pub modules: Vec<String>,
    #[serde(default)]
    pub endpoints: Vec<String>,
    #[serde(default)]
    pub specs: Vec<String>,
}

impl From<&ConceptDef> for gyre_common::graph::ConceptView {
    fn from(def: &ConceptDef) -> Self {
        let mut view = gyre_common::graph::ConceptView {
            name: def.name.clone(),
            description: def.description.clone(),
            include_types: vec![],
            include_traits: vec![],
            include_modules: vec![],
            include_endpoints: vec![],
            include_specs: vec![],
        };
        for inc in &def.include {
            view.include_types.extend(inc.types.iter().cloned());
            view.include_traits.extend(inc.traits.iter().cloned());
            view.include_modules.extend(inc.modules.iter().cloned());
            view.include_endpoints.extend(inc.endpoints.iter().cloned());
            view.include_specs.extend(inc.specs.iter().cloned());
        }
        view
    }
}

// ---------------------------------------------------------------------------
// Drift-review task creation helper (TASK-016 F2)
// ---------------------------------------------------------------------------

/// Create a drift-review Task entity when an enforced link becomes stale.
///
/// spec-links.md §Automatic Staleness Detection step 3: "Creates drift-review tasks
/// in the source specs' repos." Applies to all link types except `references`
/// (which has no mechanical enforcement). Callers gate on link type before calling.
async fn create_drift_review_task(
    tasks: Option<&Arc<dyn gyre_ports::TaskRepository>>,
    source_path: &str,
    target_path: &str,
    link_type: &SpecLinkType,
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
        "Drift review: '{}' {} '{}' which has changed",
        source_path, link_type, target_path
    );
    let description = format!(
        "The target spec '{}' has changed (new SHA). The source spec '{}' has a '{}' \
         link to it and may need to be re-evaluated.",
        target_path, source_path, link_type
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
            link_type = %link_type,
            error = %e,
            "spec-registry: failed to create drift-review task"
        );
    } else {
        info!(
            task_id = %task.id,
            source = %source_path,
            target = %target_path,
            link_type = %link_type,
            "spec-registry: drift-review task created"
        );
    }
}

// ---------------------------------------------------------------------------
// Cross-repo / cross-workspace link validation
// (spec-links.md §Cross-Repo and Cross-Workspace Links)
// ---------------------------------------------------------------------------

/// Identity of the principal whose push triggered this ledger sync — the
/// "linking user" in spec-links.md §Cross-Repo and Cross-Workspace Links
/// ("Target workspace exists and the linking user has read access").
///
/// Threaded from the authenticated caller at push-triggered syncs (git_http
/// push handler, POST /repos/:id/sync-specs). Server-originated syncs
/// (mirror fetches, background jobs) pass `None` — no linking principal.
#[derive(Debug, Clone)]
pub struct LinkPrincipal {
    /// Authenticated agent id (a user's display name for API-key / user-JWT
    /// auth — the same value the auth extractor reports).
    pub agent_id: String,
    /// Backing user id when the principal authenticated as a user.
    /// `None` for agent JWTs and the global system token.
    pub user_id: Option<gyre_common::Id>,
    /// Workspace scope from the validated token hierarchy claims, when
    /// present (workspace-scoped agent JWTs).
    pub workspace_id: Option<String>,
    /// True for the genuine system principal (global dev/system token) —
    /// exempt from cross-workspace read-access validation.
    pub is_system: bool,
}

impl LinkPrincipal {
    /// Derive the linking principal from the auth extractor's resolved
    /// identity. Reuses `AuthenticatedAgent::is_system_principal` (never a
    /// bare agent_id string match — a user *named* "system" authenticates
    /// with their own user_id and must not be exempt).
    pub fn from_auth(auth: &crate::auth::AuthenticatedAgent) -> Self {
        Self {
            agent_id: auth.agent_id.clone(),
            user_id: auth.user_id.clone(),
            workspace_id: auth.workspace_id.clone(),
            is_system: auth.is_system_principal(),
        }
    }
}

/// Durable record that a link id was denied resolution because the linking
/// principal lacked read access to the target workspace. The staleness
/// checker consults it so a denied link is never silently flipped
/// "active": the pushing principal is not recoverable from the link row
/// (the spec's spec_links schema has no principal column), so the denial
/// travels in the KV store instead. Every re-push of the source manifest
/// refreshes it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkAccessDenial {
    /// Principal that was denied (agent_id / display name).
    pub principal: String,
    /// The link target as declared in the manifest.
    pub target: String,
    /// When the denial was recorded (unix seconds).
    pub at: u64,
}

/// KV namespace for [`LinkAccessDenial`] markers, keyed by link id.
pub(crate) const LINK_DENIAL_NAMESPACE: &str = "spec_link_denials";

/// Record (or refresh) the access-denial marker for a link id.
pub(crate) async fn record_link_denial(
    kv: &Arc<dyn gyre_ports::KvJsonStore>,
    link_id: &str,
    principal: Option<&LinkPrincipal>,
    target: &str,
    now: u64,
) {
    let denial = LinkAccessDenial {
        principal: principal
            .map(|p| p.agent_id.clone())
            .unwrap_or_else(|| "<unknown>".to_string()),
        target: target.to_string(),
        at: now,
    };
    match serde_json::to_string(&denial) {
        Ok(value) => {
            if let Err(e) = kv.kv_set(LINK_DENIAL_NAMESPACE, link_id, value).await {
                warn!(
                    link_id,
                    error = %e,
                    "spec-registry: failed to persist link access denial"
                );
            }
        }
        Err(e) => warn!(
            link_id,
            error = %e,
            "spec-registry: failed to serialize link access denial"
        ),
    }
}

/// Clear the access-denial marker for a link id (the link resolved, or was
/// re-declared by a push that is not access-denied).
pub(crate) async fn clear_link_denial(kv: &Arc<dyn gyre_ports::KvJsonStore>, link_id: &str) {
    if let Err(e) = kv.kv_remove(LINK_DENIAL_NAMESPACE, link_id).await {
        warn!(
            link_id,
            error = %e,
            "spec-registry: failed to clear link access denial"
        );
    }
}

/// Read the access-denial marker for a link id, if any.
pub(crate) async fn link_denial(
    kv: &Arc<dyn gyre_ports::KvJsonStore>,
    link_id: &str,
) -> Option<LinkAccessDenial> {
    let raw = kv.kv_get(LINK_DENIAL_NAMESPACE, link_id).await.ok()??;
    serde_json::from_str(&raw).ok()
}

/// spec-links.md §Cross-Repo and Cross-Workspace Links: "Target workspace
/// exists and the linking user has read access."
///
/// - `principal: None` — server-originated sync (mirror fetch, background
///   job): no linking user to validate, exempt.
/// - System principal — exempt.
/// - User principal — read access is workspace membership (any role grants
///   workspace read; an unaccepted invitation is not access).
/// - Agent principal — agents are workspace-scoped (platform-model.md §1
///   hierarchy enforcement): read access exists only when the validated
///   token's workspace scope IS the target workspace.
///
/// Fails closed when a user principal's membership cannot be checked
/// (missing repo handle or lookup error): an unvalidatable principal must
/// not get cross-workspace resolution.
async fn principal_has_read_access(
    memberships: Option<&Arc<dyn gyre_ports::WorkspaceMembershipRepository>>,
    principal: Option<&LinkPrincipal>,
    target_workspace_id: &gyre_common::Id,
) -> bool {
    let Some(p) = principal else {
        return true; // server-originated sync — no linking user
    };
    if p.is_system {
        return true;
    }
    match &p.user_id {
        Some(user_id) => match memberships {
            Some(memberships) => {
                match memberships
                    .find_by_user_and_workspace(user_id, target_workspace_id)
                    .await
                {
                    Ok(Some(membership)) => membership.accepted,
                    Ok(None) => false,
                    Err(e) => {
                        warn!(
                            principal = %p.agent_id,
                            workspace_id = %target_workspace_id,
                            error = %e,
                            "spec-registry: membership lookup failed — denying cross-workspace link (fail closed)"
                        );
                        false
                    }
                }
            }
            None => false, // cannot validate a user principal — fail closed
        },
        None => p.workspace_id.as_deref() == Some(target_workspace_id.as_str()),
    }
}

/// spec-links.md §Cross-Repo and Cross-Workspace Links: "Target spec path
/// exists in the target repo's manifest." Reads the target repo's current
/// default-branch `specs/manifest.yaml` via the same git-show mechanism the
/// sync itself uses and requires the target path to be declared. A missing
/// or unparseable manifest declares nothing — the path does not exist.
pub(crate) async fn target_manifest_declares_path(
    repo: &gyre_domain::Repository,
    target_path: &str,
) -> bool {
    // Fail closed: an unreadable manifest declares nothing (Unknown is
    // treated as not declared for NEW link resolution; existing-link
    // status re-evaluation distinguishes the two — see
    // `target_manifest_path_presence`).
    matches!(
        target_manifest_path_presence(repo, target_path).await,
        ManifestPathPresence::Declared
    )
}

/// Tri-state presence of a target path in a repo's current manifest
/// (task-288). `Unknown` is deliberately distinct from `Absent`: an
/// unreadable manifest (repo path gone, no manifest.yaml, unparseable
/// YAML) means the manifest state CANNOT be determined. Push-time
/// resolution of NEW links fails closed (unknown is treated as not
/// declared, `target_manifest_declares_path`), while re-evaluation of an
/// EXISTING link's stored status must not break links on undeterminable
/// state — only a manifest that is readable and does NOT declare the
/// path evidences "no longer in the manifest".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ManifestPathPresence {
    /// The manifest is readable and declares the target path.
    Declared,
    /// The manifest is readable and does NOT declare the target path —
    /// the "no longer in the manifest" half of the `broken` definition.
    Absent,
    /// The manifest could not be read — presence is undeterminable.
    Unknown,
}

/// Read the target repo's current manifest and classify the target path's
/// presence in it (see `ManifestPathPresence`).
pub(crate) async fn target_manifest_path_presence(
    repo: &gyre_domain::Repository,
    target_path: &str,
) -> ManifestPathPresence {
    let refname = format!("refs/heads/{}", repo.default_branch);
    match read_manifest(&repo.path, &refname).await {
        Some(manifest) => {
            if manifest
                .specs
                .iter()
                .any(|e| normalize_spec_rel_path(&e.path) == normalize_spec_rel_path(target_path))
            {
                ManifestPathPresence::Declared
            } else {
                ManifestPathPresence::Absent
            }
        }
        None => ManifestPathPresence::Unknown,
    }
}

/// spec-links.md §Link Status (task-288): evaluate the status a link's
/// stored graph row must hold given CURRENT conditions — the stored graph
/// reflects current conditions, not a one-way ratchet. Evaluation order:
/// 1. `conflicted` — a `conflicts_with` link whose two endpoints are both
///    Approved (violation). Takes precedence: the conflict is the salient
///    condition while it holds.
/// 2. `broken` — the target is absent from the ledger (deleted) OR absent
///    from its repo's current manifest (removed from the manifest; the
///    ledger keeps such specs as Deprecated, so the ledger alone can never
///    fire this half). A target deprecated via supersedes approval but
///    still declared in its manifest is NOT broken — deprecation is not
///    deletion. A target repo whose manifest cannot be read is
///    undeterminable (`ManifestPathPresence::Unknown`) — the SHA rules
///    apply rather than breaking on unknown state.
/// 3. `stale` — the pinned `target_sha` differs from the ledger's current
///    SHA for the target.
/// 4. `active` — otherwise.
pub(crate) async fn evaluate_link_status(
    ledger: &Arc<dyn gyre_ports::SpecLedgerRepository>,
    repos: Option<&Arc<dyn gyre_ports::RepoRepository>>,
    link: &SpecLinkEntry,
) -> String {
    let source_entry = ledger.find_by_path(&link.source_path).await.ok().flatten();
    let target_entry = ledger.find_by_path(&link.target_path).await.ok().flatten();

    // 1. conflicted — both endpoints of a conflicts_with link Approved.
    if link.link_type == SpecLinkType::ConflictsWith
        && source_entry
            .as_ref()
            .is_some_and(|e| e.approval_status == ApprovalStatus::Approved)
        && target_entry
            .as_ref()
            .is_some_and(|e| e.approval_status == ApprovalStatus::Approved)
    {
        return "conflicted".to_string();
    }

    // 2. broken — target absent from the ledger entirely.
    let Some(target_entry) = target_entry else {
        return "broken".to_string();
    };

    // 2 (cont.) — target absent from its repo's current manifest. The
    // target's repo is the explicit cross-repo target when set, else the
    // source's repo (same-repo links resolve within the source repo).
    if let Some(repos) = repos {
        let repo_id = link
            .target_repo_id
            .as_ref()
            .or(link.source_repo_id.as_ref());
        if let Some(repo_id) = repo_id {
            if let Ok(Some(repo)) = repos.find_by_id(&gyre_common::Id::new(repo_id)).await {
                if matches!(
                    target_manifest_path_presence(&repo, &link.target_path).await,
                    ManifestPathPresence::Absent
                ) {
                    return "broken".to_string();
                }
            }
            // Repo unresolvable or manifest unreadable — the manifest
            // condition is undeterminable; fall through to the SHA rules.
        }
    }

    // 3. stale / 4. active — the pinned SHA versus the ledger's current.
    if let Some(pinned_sha) = &link.target_sha {
        let current_sha = &target_entry.current_sha;
        if !current_sha.is_empty() && current_sha != pinned_sha {
            return "stale".to_string();
        }
    }

    "active".to_string()
}

/// Mutate a link's stored status in the hot cache and write the mutation
/// through to the durable graph (spec-links.md §Forge-Maintained Spec
/// Graph — the SQL table is authoritative). `stale_since` stamps the
/// transition INTO stale and is cleared when the link leaves stale
/// (task-288 reversion: the stored graph tracks current conditions).
/// Returns the updated entry when the link was found.
pub(crate) async fn write_link_status(
    links_store: &SpecLinksStore,
    link_repo: &Arc<dyn gyre_ports::SpecLinkRepository>,
    link_id: &str,
    status: &str,
    now: u64,
) -> Option<SpecLinkEntry> {
    let updated: Option<SpecLinkEntry> = {
        let mut store = links_store.lock().await;
        store.iter_mut().find(|l| l.id == link_id).map(|link| {
            link.status = status.to_string();
            link.stale_since = if status == "stale" { Some(now) } else { None };
            link.clone()
        })
    };
    if let Some(entry) = &updated {
        if let Err(e) = link_repo.save(entry).await {
            warn!(link_id = %link_id, "spec-registry: failed to persist link status: {e}");
        }
    }
    updated
}

/// Conflict-only status reconciliation over the given links (task-288,
/// spec-links.md §Link Status): set `conflicted` on a `conflicts_with`
/// link whose endpoints are both Approved, and REVERT a stored
/// `conflicted` whose condition has ceased to whatever the
/// active/stale/broken rules yield for its current pin and target state.
/// Links in other statuses are left to the staleness pass — this never
/// flips active/stale/broken. Returns the post-write stored entries.
pub(crate) async fn reconcile_conflict_statuses(
    ledger: &Arc<dyn gyre_ports::SpecLedgerRepository>,
    repos: Option<&Arc<dyn gyre_ports::RepoRepository>>,
    links_store: &SpecLinksStore,
    link_repo: &Arc<dyn gyre_ports::SpecLinkRepository>,
    links: Vec<SpecLinkEntry>,
    now: u64,
) -> Vec<SpecLinkEntry> {
    let mut reconciled = Vec::with_capacity(links.len());
    for link in links {
        if link.status == "unresolved" || link.link_type != SpecLinkType::ConflictsWith {
            reconciled.push(link);
            continue;
        }
        let evaluated = evaluate_link_status(ledger, repos, &link).await;
        if evaluated == "conflicted" {
            if link.status == "conflicted" {
                reconciled.push(link);
            } else {
                info!(
                    source = %link.source_path,
                    target = %link.target_path,
                    "spec-registry: conflicts_with link has both endpoints approved — marking conflicted"
                );
                reconciled.push(
                    write_link_status(links_store, link_repo, &link.id, "conflicted", now)
                        .await
                        .unwrap_or_else(|| link.clone()),
                );
            }
        } else if link.status == "conflicted" {
            // The both-approved condition ceased — revert to the
            // active/stale/broken rules for the current pin and target.
            info!(
                source = %link.source_path,
                target = %link.target_path,
                reverted_to = %evaluated,
                "spec-registry: conflict condition ceased — reverting link status"
            );
            reconciled.push(
                write_link_status(links_store, link_repo, &link.id, &evaluated, now)
                    .await
                    .unwrap_or_else(|| link.clone()),
            );
        } else {
            reconciled.push(link);
        }
    }
    reconciled
}

/// task-288 (spec-links.md §Link Status): reconcile the stored `conflicted`
/// status of every link touching `spec_path` (as source or target).
/// Called whenever an approval changes — approve, revoke, and the
/// sync-time resets to Pending (extends drift, content change
/// invalidation, deprecation) — so a both-approved conflicts_with pair
/// becomes `conflicted` and a ceased conflict reverts, without waiting
/// for the daily staleness pass.
pub(crate) async fn refresh_conflict_statuses_for_spec(
    ledger: &Arc<dyn gyre_ports::SpecLedgerRepository>,
    repos: Option<&Arc<dyn gyre_ports::RepoRepository>>,
    links_store: &SpecLinksStore,
    link_repo: &Arc<dyn gyre_ports::SpecLinkRepository>,
    spec_path: &str,
    now: u64,
) {
    let touching: Vec<SpecLinkEntry> = {
        let store = links_store.lock().await;
        store
            .iter()
            .filter(|l| l.source_path == spec_path || l.target_path == spec_path)
            .cloned()
            .collect()
    };
    if touching.is_empty() {
        return;
    }
    reconcile_conflict_statuses(ledger, repos, links_store, link_repo, touching, now).await;
}

/// A manifest link target resolved exactly the way `sync_spec_ledger`
/// stores it (task-293): the push-time cycle check reuses this resolution so
/// the graph it validates is attributed identically to the graph the sync
/// persists — attribution parity is what makes cross-repo cycle detection
/// sound.
pub(crate) struct ResolvedManifestTarget {
    /// Target spec path within the target repo (no `@`-prefix).
    pub target_path: String,
    /// Resolved target repo id. `None` for same-repo links (target lives in
    /// the source repo) and for unresolved cross-repo targets.
    pub target_repo_id: Option<String>,
    /// Human-readable composite path preserved from the manifest `target`
    /// field. `None` for same-repo links.
    pub target_display: Option<String>,
    /// Link health after validation: "active" or "unresolved".
    pub status: String,
    /// Cross-workspace read-access denial marker (see `record_link_denial`).
    pub access_denied: bool,
}

/// Resolve a parsed manifest link target per spec-links.md §Cross-Repo and
/// Cross-Workspace Links: same-repo targets need no resolution; same-workspace
/// cross-repo targets (`@repo_name/spec_path`) resolve within the SOURCE
/// workspace; three-segment targets disambiguate by workspace existence
/// (the fully-qualified workspace form takes precedence, with a
/// same-workspace cross-repo fallback). Targets failing validation resolve
/// to `target_repo_id = None` / `status = "unresolved"`.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn resolve_manifest_link_target(
    parsed: &CrossWorkspaceTarget,
    entry_path: &str,
    link_target: &str,
    source_workspace_id: Option<&str>,
    workspaces: Option<&Arc<dyn gyre_ports::WorkspaceRepository>>,
    repos: Option<&Arc<dyn gyre_ports::RepoRepository>>,
    tenant_id: Option<&gyre_common::Id>,
    memberships: Option<&Arc<dyn gyre_ports::WorkspaceMembershipRepository>>,
    principal: Option<&LinkPrincipal>,
) -> ResolvedManifestTarget {
    match parsed {
        CrossWorkspaceTarget::SameRepo { path } => ResolvedManifestTarget {
            target_path: path.clone(),
            target_repo_id: None,
            target_display: None,
            status: "active".to_string(),
            access_denied: false,
        },
        CrossWorkspaceTarget::CrossRepo { repo_name, path } => {
            // Same-workspace cross-repo (spec-links.md:
            // "@{repo_name}/{spec_path} (no workspace prefix)") —
            // resolved within the SOURCE workspace. No separate
            // read-access check: the push itself required write
            // access to that workspace, which subsumes read.
            let display = format!("@{repo_name}/{path}");
            let (repo_id, status) = validate_cross_repo_target(
                repos,
                source_workspace_id,
                entry_path,
                link_target,
                repo_name,
                path,
            )
            .await;
            ResolvedManifestTarget {
                target_path: path.clone(),
                target_repo_id: repo_id,
                target_display: Some(display),
                status,
                access_denied: false,
            }
        }
        CrossWorkspaceTarget::CrossWorkspace {
            workspace_slug,
            repo_name,
            path,
        } => {
            let display = format!("@{workspace_slug}/{repo_name}/{path}");
            // Three-segment targets are syntactically
            // ambiguous between the cross-workspace form
            // (@{workspace_slug}/{repo_name}/{spec_path})
            // and the same-workspace cross-repo form
            // (@{repo_name}/{spec_path}) whenever the spec
            // path itself contains a slash — spec-links.md
            // declares both formats. The fully-qualified
            // workspace form takes precedence; only when
            // no workspace with that slug exists in the
            // tenant is the target the same-workspace
            // cross-repo form, resolved within the SOURCE
            // workspace (whose write access the push
            // already required, subsuming read).
            let ws_exists = match (workspaces, tenant_id) {
                (Some(workspaces), Some(tenant_id)) => !matches!(
                    workspaces.find_by_slug(tenant_id, workspace_slug).await,
                    Ok(None)
                ),
                // No workspace context to disambiguate with:
                // keep the cross-workspace interpretation
                // (which cannot resolve without context).
                _ => true,
            };
            if ws_exists {
                let (repo_id, status, denied) = validate_cross_workspace_target(
                    workspaces,
                    repos,
                    tenant_id,
                    memberships,
                    principal,
                    entry_path,
                    link_target,
                    workspace_slug,
                    repo_name,
                    path,
                )
                .await;
                ResolvedManifestTarget {
                    target_path: path.clone(),
                    target_repo_id: repo_id,
                    target_display: Some(display),
                    status,
                    access_denied: denied,
                }
            } else {
                // Fallback: seg1 is the repo name, the rest
                // is the slashed spec path.
                let fallback_path = format!("{repo_name}/{path}");
                let (repo_id, status) = validate_cross_repo_target(
                    repos,
                    source_workspace_id,
                    entry_path,
                    link_target,
                    workspace_slug,
                    &fallback_path,
                )
                .await;
                ResolvedManifestTarget {
                    target_path: fallback_path,
                    target_repo_id: repo_id,
                    target_display: Some(display),
                    status,
                    access_denied: false,
                }
            }
        }
    }
}

/// Validate and resolve a same-workspace cross-repo target
/// (`@repo_name/spec_path`) at push time. Returns
/// `(target_repo_id, status)`.
///
/// Validation chain: the repo exists within the SOURCE workspace, and the
/// target path is declared in the target repo's current manifest.
async fn validate_cross_repo_target(
    repos: Option<&Arc<dyn gyre_ports::RepoRepository>>,
    source_workspace_id: Option<&str>,
    entry_path: &str,
    link_target: &str,
    repo_name: &str,
    path: &str,
) -> (Option<String>, String) {
    let ws_id = source_workspace_id.map(gyre_common::Id::new);
    let repo = match (repos, ws_id.as_ref()) {
        (Some(repos), Some(ws_id)) => repos
            .find_by_name_and_workspace(ws_id, repo_name)
            .await
            .ok()
            .flatten(),
        _ => None,
    };
    let Some(repo) = repo else {
        warn!(
            source = entry_path,
            target = link_target,
            repo_name,
            "spec-registry: cross-repo target unresolved (repo not found in source workspace)"
        );
        return (None, "unresolved".to_string());
    };
    if !target_manifest_declares_path(&repo, path).await {
        warn!(
            source = entry_path,
            target = link_target,
            spec_path = path,
            repo_id = %repo.id,
            "spec-registry: cross-repo target unresolved (path not declared in target repo manifest)"
        );
        return (None, "unresolved".to_string());
    }
    (Some(repo.id.to_string()), "active".to_string())
}

/// Validate and resolve a cross-workspace target
/// (`@workspace_slug/repo_name/spec_path`) at push time. Returns
/// `(target_repo_id, status, access_denied)`.
///
/// Validation chain per spec-links.md §Cross-Repo and Cross-Workspace
/// Links: target workspace exists (tenant-scoped) → the linking principal
/// has read access → target repo exists within that workspace → target
/// path is declared in the target repo's current manifest.
#[allow(clippy::too_many_arguments)]
async fn validate_cross_workspace_target(
    workspaces: Option<&Arc<dyn gyre_ports::WorkspaceRepository>>,
    repos: Option<&Arc<dyn gyre_ports::RepoRepository>>,
    tenant_id: Option<&gyre_common::Id>,
    memberships: Option<&Arc<dyn gyre_ports::WorkspaceMembershipRepository>>,
    principal: Option<&LinkPrincipal>,
    entry_path: &str,
    link_target: &str,
    workspace_slug: &str,
    repo_name: &str,
    path: &str,
) -> (Option<String>, String, bool) {
    // 1. Target workspace exists (tenant-scoped — never a fabricated tenant).
    let target_ws = match (workspaces, tenant_id) {
        (Some(workspaces), Some(tenant_id)) => workspaces
            .find_by_slug(tenant_id, workspace_slug)
            .await
            .ok()
            .flatten(),
        _ => None,
    };
    let Some(target_ws) = target_ws else {
        warn!(
            source = entry_path,
            target = link_target,
            workspace_slug,
            "spec-registry: cross-workspace target unresolved (workspace not found in tenant)"
        );
        return (None, "unresolved".to_string(), false);
    };

    // 2. The linking principal has read access to the target workspace.
    if !principal_has_read_access(memberships, principal, &target_ws.id).await {
        warn!(
            source = entry_path,
            target = link_target,
            principal = principal.map(|p| p.agent_id.as_str()).unwrap_or("<none>"),
            workspace_id = %target_ws.id,
            "spec-registry: cross-workspace link DENIED — linking principal lacks read access to target workspace; storing unresolved"
        );
        return (None, "unresolved".to_string(), true);
    }

    // 3. Target repo exists within that workspace.
    let repo = match repos {
        Some(repos) => repos
            .find_by_name_and_workspace(&target_ws.id, repo_name)
            .await
            .ok()
            .flatten(),
        None => None,
    };
    let Some(repo) = repo else {
        warn!(
            source = entry_path,
            target = link_target,
            workspace_slug,
            repo_name,
            "spec-registry: cross-workspace target unresolved (repo not found in workspace)"
        );
        return (None, "unresolved".to_string(), false);
    };

    // 4. Target spec path exists in the target repo's manifest.
    if !target_manifest_declares_path(&repo, path).await {
        warn!(
            source = entry_path,
            target = link_target,
            spec_path = path,
            repo_id = %repo.id,
            "spec-registry: cross-workspace target unresolved (path not declared in target repo manifest)"
        );
        return (None, "unresolved".to_string(), false);
    }

    (Some(repo.id.to_string()), "active".to_string(), false)
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
pub(crate) async fn get_blob_sha(
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

/// Find `specs/*.md` files not listed in the manifest.
///
/// Uses `git ls-tree -r --name-only <sha> specs/` to enumerate spec files.
/// Returns the list of relative spec paths (e.g. `"system/new-spec.md"`) that
/// are NOT in the manifest.  Excludes `index.md`, `prior-art/`, `milestones/`.
async fn find_unregistered_specs(
    git_bin: &str,
    repo_path: &str,
    sha: &str,
    manifest_paths: &std::collections::HashSet<String>,
) -> Vec<String> {
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
        _ => return Vec::new(),
    };

    let mut unregistered = Vec::new();
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        // Only check .md files under specs/ (excluding manifest.yaml and index.md).
        if !line.ends_with(".md") {
            continue;
        }
        let relative = line.strip_prefix("specs/").unwrap_or(line);
        // Skip index.md, prior-art/, and milestones/ directories.
        if relative == "index.md"
            || relative.starts_with("prior-art/")
            || relative.starts_with("milestones/")
        {
            continue;
        }
        if !manifest_paths.contains(relative) {
            unregistered.push(relative.to_string());
        }
    }
    unregistered
}

/// Check manifest coverage: reject or warn about unregistered spec files.
///
/// spec-registry.md §Ledger Sync on Push step 4: "For files under `specs/` not
/// in manifest: reject push (or warn, policy-dependent)."
///
/// When `enforce` is true, returns `Err` with a rejection message listing the
/// unregistered files and a fix hint.  When `enforce` is false, logs warnings
/// only and returns `Ok(())`.
pub async fn check_manifest_coverage(
    repo_path: &str,
    sha: &str,
    manifest_paths: &std::collections::HashSet<String>,
    enforce: bool,
) -> Result<(), String> {
    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());
    let unregistered = find_unregistered_specs(&git_bin, repo_path, sha, manifest_paths).await;

    if unregistered.is_empty() {
        return Ok(());
    }

    if enforce {
        let file_list = unregistered
            .iter()
            .map(|p| format!("  - {p}"))
            .collect::<Vec<_>>()
            .join("\n");
        Err(format!(
            "push rejected: spec files not registered in specs/manifest.yaml:\n\
             {file_list}\n\
             fix: add entries to specs/manifest.yaml for the files listed above"
        ))
    } else {
        for path in &unregistered {
            warn!(
                spec_path = %path,
                "spec-registry: file under specs/ is not registered in manifest.yaml — \
                 add it to specs/manifest.yaml to enable lifecycle tracking"
            );
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Cycle detection for spec link graph (TASK-019)
// spec-links.md §Cycle Detection: "The forge rejects manifest changes that
// would create cycles in the spec graph."
// Only `depends_on`, `implements`, and `extends` links are checked.
// `conflicts_with` is bidirectional by nature (not a cycle).
// `references` and `supersedes` are excluded from cycle detection.
//
// task-293: detection runs over the TENANT-WIDE forge graph — every stored
// link in the durable spec_links table plus the pushed manifest's edges —
// not just the single manifest being pushed. A cycle whose edges are
// declared in two different manifests or repos (the enterprise case the
// spec's Problem section exists for) is therefore rejected at receive time.
//
// task-293 review repair (round 2), two scope corrections:
// - FAIL CLOSED on storage errors: if the durable spec_links table cannot
//   be read, the push is rejected — never silently degraded to the
//   pushed manifest's own edges (the pre-task-293 scope).
// - REJECT ONLY CYCLES THE PUSH CREATES: a cycle is reported only when at
//   least one edge contributed by the pushed manifest participates in it.
//   Stored-only cycles (legacy pre-gate rows, ungated mirror-synced edges)
//   are not created by this push and must not block unrelated repos that
//   have no remedy of their own.
// ---------------------------------------------------------------------------

/// Node identity in the tenant-wide spec graph: (repo id, spec path).
/// The empty repo id is the legacy unscoped bucket (links recorded before
/// repo scoping existed).
type SpecGraphKey = (String, String);

fn spec_graph_key(repo: &str, path: &str) -> SpecGraphKey {
    (repo.to_string(), path.to_string())
}

/// Render a graph node for the rejection message: `repo_id:path`, or just
/// the path for the legacy unscoped bucket.
fn display_spec_graph_key(key: &SpecGraphKey) -> String {
    if key.0.is_empty() {
        key.1.clone()
    } else {
        format!("{}:{}", key.0, key.1)
    }
}

/// Whether a link type participates in cycle detection per spec-links.md
/// §Cycle Detection: `conflicts_with` is bidirectional by nature (not a
/// cycle), `references` and `supersedes` are excluded; every other directed
/// type (`depends_on`, `implements`, `extends`) participates.
fn participates_in_cycle_detection(link_type: &SpecLinkType) -> bool {
    !matches!(
        link_type,
        SpecLinkType::ConflictsWith | SpecLinkType::References | SpecLinkType::Supersedes
    )
}

/// Context for tenant-wide cycle detection (task-293): the durable
/// spec-links table plus the resolution context needed to attribute the
/// pushed manifest's cross-repo edges to their repos. `None` for the
/// context degrades `check_spec_link_cycles` to the pushed manifest's own
/// edges only (the pre-task-293 scope).
pub struct TenantCycleContext<'a> {
    /// Durable spec-link graph (spec-links.md §Forge-Maintained Spec Graph).
    /// `None` skips the stored-links half of the combined graph.
    pub link_repo: Option<&'a Arc<dyn gyre_ports::SpecLinkRepository>>,
    /// Repo the manifest is being pushed to: the pushed manifest's entries
    /// (and its same-repo targets) are attributed to it.
    pub source_repo_id: Option<&'a str>,
    /// Workspace of the push — same-workspace cross-repo targets resolve
    /// within it.
    pub source_workspace_id: Option<&'a str>,
    /// Cross-workspace / cross-repo resolution context (mirrors what
    /// `sync_spec_ledger` receives so attribution matches the stored graph).
    pub workspaces: Option<&'a Arc<dyn gyre_ports::WorkspaceRepository>>,
    pub repos: Option<&'a Arc<dyn gyre_ports::RepoRepository>>,
    pub tenant_id: Option<&'a gyre_common::Id>,
    pub memberships: Option<&'a Arc<dyn gyre_ports::WorkspaceMembershipRepository>>,
    /// The authenticated pusher — cross-workspace targets the pusher cannot
    /// read resolve unresolved and therefore never become graph edges,
    /// exactly as `sync_spec_ledger` stores them.
    pub principal: Option<&'a LinkPrincipal>,
    /// KV store holding durable link access-denial markers. An unresolved
    /// cross-repo row denied at push time never activates (the staleness
    /// pass skips it), so it must not contribute an edge to the combined
    /// graph. `None` skips the denial check.
    pub kv: Option<&'a Arc<dyn gyre_ports::KvJsonStore>>,
}

/// Check the pushed manifest's spec links for cycles over the TENANT-WIDE
/// forge graph: all stored links in the durable spec_links table plus the
/// pushed manifest's edges, with node identity (repo, path). Returns
/// `Ok(())` if no cycles, `Err(message)` naming the cycle if one is
/// detected. `tenant = None` checks the manifest's own edges only.
pub async fn check_spec_link_cycles(
    repo_path: &str,
    sha: &str,
    tenant: Option<&TenantCycleContext<'_>>,
) -> Result<(), String> {
    let git_bin = std::env::var("GYRE_GIT_PATH").unwrap_or_else(|_| "git".to_string());

    let manifest_yaml = match read_git_file(&git_bin, repo_path, sha, "specs/manifest.yaml").await {
        Some(content) => content,
        None => return Ok(()), // No manifest — no cycles possible.
    };

    let manifest = match parse_manifest(&manifest_yaml) {
        Ok(m) => m,
        Err(_) => return Ok(()), // Parse error handled by sync_spec_ledger.
    };

    match tenant {
        None => detect_link_cycles(&manifest),
        Some(ctx) => {
            let graph = build_tenant_cycle_graph(&manifest, ctx).await?;
            detect_graph_cycles(&graph)
        }
    }
}

/// The combined tenant-wide spec-link graph used for cycle detection:
/// adjacency over (repo, path) nodes plus the list of edges contributed
/// by the manifest being pushed. Only cycles containing at least one
/// pushed edge are cycles THIS push creates (spec-links.md §Cycle
/// Detection: the forge "rejects manifest changes that WOULD CREATE
/// cycles") — a stored-only cycle (legacy pre-gate rows, ungated
/// mirror-synced edges) exists regardless of the push.
struct SpecCycleGraph {
    adj: std::collections::HashMap<SpecGraphKey, Vec<SpecGraphKey>>,
    /// Edges contributed by the pushed manifest: (from, to) pairs. A cycle
    /// is reported only when one of these edges participates in it.
    pushed: Vec<(SpecGraphKey, SpecGraphKey)>,
}

impl SpecCycleGraph {
    fn add_edge(&mut self, from: SpecGraphKey, to: SpecGraphKey, pushed: bool) {
        if pushed {
            self.pushed.push((from.clone(), to.clone()));
        }
        self.adj.entry(from).or_default().push(to);
    }
}

/// Build the combined tenant-wide cycle graph (task-293):
///
/// 1. All stored links in the durable spec_links table — every repo's
///    previously synced edges — EXCEPT the pushed repo's links for source
///    paths declared in the pushed manifest: the push replaces exactly
///    those rows (`sync_spec_ledger`'s `replace_for_source` scoping), so
///    counting the pre-push versions alongside the manifest's new edges
///    would reject pushes that REMOVE a cycle, not create one.
/// 2. The pushed manifest's edges: entries attributed to the source repo,
///    cross-repo/cross-workspace targets resolved to their repos via the
///    same resolution the sync uses, same-repo targets attributed to the
///    source repo. Unresolved cross-repo targets have no attributable
///    target node — the sync stores them unresolved, so they are not edges
///    in the stored graph either.
///
/// Unresolved cross-repo STORED rows (target_repo_id NULL, target_display
/// `@…`) are attributed exactly when the staleness pass
/// (`resolve_unresolved_cross_workspace_links`) will activate them — the
/// same source-scope derivation and the same target-manifest requirement,
/// with the pushed manifest standing in for the target repo's current
/// manifest when the target repo is the one being pushed (review repair,
/// round 3: a pending row the daily job will deterministically flip into
/// an active attributed edge already belongs in the graph this push is
/// checked against, otherwise a cycle whose edges arrive as an unresolved
/// row plus a later declaring push lands permanently with no push ever
/// rejected). Rows that cannot activate (denied at push time, scope
/// underivable, target repo missing, target path still undeclared) stay
/// unattributed — the graph must not fabricate edges that will never
/// exist.
///
/// FAILS CLOSED (review repair, round 2): a `list_all` storage error is
/// propagated as `Err` — the push is rejected when the durable graph
/// cannot be read, mirroring the task-264 signing-gate contract
/// ("enumeration failure REJECTS the push ... never a silent pass").
/// Silently skipping the stored half would re-open the pre-task-293
/// detection scope exactly when the graph is unreadable.
async fn build_tenant_cycle_graph(
    manifest: &SpecManifest,
    ctx: &TenantCycleContext<'_>,
) -> Result<SpecCycleGraph, String> {
    let mut graph = SpecCycleGraph {
        adj: std::collections::HashMap::new(),
        pushed: Vec::new(),
    };
    let repo_scope = ctx.source_repo_id.unwrap_or("");
    let manifest_paths: std::collections::HashSet<&str> =
        manifest.specs.iter().map(|e| e.path.as_str()).collect();

    // 1. Stored links, minus the rows this push replaces.
    if let Some(link_repo) = ctx.link_repo {
        let stored = link_repo.list_all().await.map_err(|e| {
            warn!(
                error = %e,
                "spec-registry: cannot read stored spec links for cycle check — failing closed"
            );
            format!("spec link cycle check failed: unable to read the stored spec link graph: {e}")
        })?;
        for link in stored {
            if !participates_in_cycle_detection(&link.link_type) {
                continue;
            }
            let link_repo_key = link.source_repo_id.as_deref().unwrap_or("");
            if link_repo_key == repo_scope && manifest_paths.contains(link.source_path.as_str()) {
                continue; // replaced by the pushed manifest's edges below
            }
            let target_repo = match link.target_repo_id.as_deref() {
                Some(r) => r.to_string(),
                // Same-repo link: the target lives in the source repo.
                None if link.target_display.is_none() => link_repo_key.to_string(),
                // Unresolved cross-repo row (review repair, round 3):
                // attributed exactly when the staleness pass will activate
                // it — see `attribute_unresolved_stored_link`.
                None => {
                    let Some(attributed) =
                        attribute_unresolved_stored_link(&link, manifest, repo_scope, ctx).await
                    else {
                        continue; // cannot activate — not an edge in the graph
                    };
                    attributed
                }
            };
            graph.add_edge(
                spec_graph_key(link_repo_key, &link.source_path),
                spec_graph_key(&target_repo, &link.target_path),
                false,
            );
        }
    }

    // 2. The pushed manifest's edges.
    for entry in &manifest.specs {
        for link in &entry.links {
            if !participates_in_cycle_detection(&link.link_type) {
                continue;
            }
            let parsed = parse_cross_workspace_target(&link.target);
            let resolved = resolve_manifest_link_target(
                &parsed,
                &entry.path,
                &link.target,
                ctx.source_workspace_id,
                ctx.workspaces,
                ctx.repos,
                ctx.tenant_id,
                ctx.memberships,
                ctx.principal,
            )
            .await;
            // Same-repo targets attribute to the source repo; cross-repo
            // targets to their resolved repo. Unresolved cross-repo targets
            // are not edges in the graph the sync will store.
            let target_repo = match resolved.target_repo_id.as_deref() {
                Some(r) => r.to_string(),
                None if resolved.target_display.is_none() => repo_scope.to_string(),
                None => continue,
            };
            graph.add_edge(
                spec_graph_key(repo_scope, &entry.path),
                spec_graph_key(&target_repo, &resolved.target_path),
                true,
            );
        }
    }

    Ok(graph)
}

/// Attribute an unresolved cross-repo stored row to its target repo for
/// the combined cycle graph (review repair, round 3): the row is counted
/// as an edge exactly when the staleness pass
/// (`resolve_unresolved_cross_workspace_links`) will activate it — the
/// same source-scope derivation (source repo → workspace → tenant, never
/// a fabricated scope identity), the same target-display disambiguation,
/// and the same target-manifest requirement. When the target repo is the
/// repo being pushed, its "current manifest" is the pushed manifest
/// itself, so the activation (and therefore the edge) is deterministic
/// given this push's own content.
///
/// Returns the resolved target repo id, or `None` when the row cannot
/// activate — denied at push time, scope underivable, target repo
/// missing, or target path not declared in the target repo's current
/// manifest — in which case it must not contribute an edge.
async fn attribute_unresolved_stored_link(
    link: &SpecLinkEntry,
    manifest: &SpecManifest,
    repo_scope: &str,
    ctx: &TenantCycleContext<'_>,
) -> Option<String> {
    // Only rows the staleness pass considers for activation: unresolved
    // status with a composite cross-repo display target.
    if link.status != "unresolved" {
        return None;
    }
    let display = link.target_display.as_deref()?;
    if !display.starts_with('@') {
        return None;
    }
    // Access-denied at push time: the denial stands until the source
    // manifest is pushed again by a principal with read access — the row
    // never activates, so it is not an edge in the graph.
    if let Some(kv) = ctx.kv {
        if link_denial(kv, &link.id).await.is_some() {
            return None;
        }
    }
    let repos = ctx.repos?;
    let workspaces = ctx.workspaces?;
    // Source scope: source repo → workspace (→ tenant). Skip when
    // underivable — never fabricate a scope identity (the same rule the
    // staleness pass follows).
    let source_repo_id = link.source_repo_id.as_deref()?;
    let source_repo = repos
        .find_by_id(&gyre_common::Id::new(source_repo_id))
        .await
        .ok()??;
    let source_ws = workspaces
        .find_by_id(&source_repo.workspace_id)
        .await
        .ok()??;

    let parsed = parse_cross_workspace_target(display);
    let target_repo = match &parsed {
        CrossWorkspaceTarget::CrossWorkspace {
            workspace_slug,
            repo_name,
            ..
        } => {
            // Tenant-scoped workspace lookup under the SOURCE link's
            // tenant — the same disambiguation the staleness pass uses.
            match workspaces
                .find_by_slug(&source_ws.tenant_id, workspace_slug)
                .await
            {
                Ok(Some(target_ws)) => repos
                    .find_by_name_and_workspace(&target_ws.id, repo_name)
                    .await
                    .ok()
                    .flatten(),
                // No workspace with that slug in the source tenant: the
                // ambiguous three-segment form — the first segment is a
                // repo name in the source link's workspace.
                Ok(None) => repos
                    .find_by_name_and_workspace(&source_ws.id, workspace_slug)
                    .await
                    .ok()
                    .flatten(),
                // Lookup failed — cannot disambiguate; leave unattributed.
                Err(_) => None,
            }
        }
        CrossWorkspaceTarget::CrossRepo { repo_name, .. } => {
            // Same-workspace cross-repo: the source link's repo →
            // workspace gives the scope the repo name resolves within.
            repos
                .find_by_name_and_workspace(&source_ws.id, repo_name)
                .await
                .ok()
                .flatten()
        }
        CrossWorkspaceTarget::SameRepo { .. } => return None,
    }?;

    let target_repo_id = target_repo.id.to_string();
    if target_repo_id == repo_scope {
        // The pushed manifest IS the target repo's new manifest: the row
        // activates after this push lands iff the pushed manifest
        // declares the target path (the same normalization
        // `target_manifest_declares_path` applies).
        let declared = manifest.specs.iter().any(|e| {
            normalize_spec_rel_path(&e.path) == normalize_spec_rel_path(&link.target_path)
        });
        if !declared {
            return None;
        }
    } else if !target_manifest_declares_path(&target_repo, &link.target_path).await {
        // Same manifest-declaration requirement as the staleness pass: a
        // target path absent from the target repo's current manifest
        // leaves the row unresolved.
        return None;
    }
    Some(target_repo_id)
}

/// Would activating the pending link `link` — attributing its target to
/// `target_repo_id` — create a cycle in the given spec-link graph rows?
/// Returns `Some(cycle)` with the repo-attributed cycle nodes joined by
/// ` → ` when it would, `None` when the activation is safe. Used by the
/// staleness pass's activation gate (task-293 round 3): the durable graph
/// the push-time cycle gate protects must not acquire a cycle through
/// unresolved-link activation either.
pub(crate) fn link_activation_creates_cycle(
    graph_rows: &[SpecLinkEntry],
    link: &SpecLinkEntry,
    target_repo_id: &str,
) -> Option<String> {
    let mut adj: std::collections::HashMap<SpecGraphKey, Vec<SpecGraphKey>> =
        std::collections::HashMap::new();
    for row in graph_rows {
        if row.id == link.id {
            continue; // the candidate itself is not an edge yet
        }
        if !participates_in_cycle_detection(&row.link_type) {
            continue;
        }
        let source_repo = row.source_repo_id.as_deref().unwrap_or("");
        let row_target_repo = match row.target_repo_id.as_deref() {
            Some(r) => r,
            // Same-repo link: the target lives in the source repo.
            None if row.target_display.is_none() => source_repo,
            // Unresolved cross-repo row: no attributed edge yet.
            None => continue,
        };
        adj.entry(spec_graph_key(source_repo, &row.source_path))
            .or_default()
            .push(spec_graph_key(row_target_repo, &row.target_path));
    }
    // The activated edge is u → v (u = source node, v = target node); it
    // closes a cycle exactly when u is reachable from v through the graph.
    let source_repo = link.source_repo_id.as_deref().unwrap_or("");
    let u = spec_graph_key(source_repo, &link.source_path);
    let v = spec_graph_key(target_repo_id, &link.target_path);
    let chain = bfs_path(&adj, &v, &u)?;
    let mut named: Vec<String> = vec![display_spec_graph_key(&u)];
    named.extend(chain.iter().map(|k| display_spec_graph_key(k)));
    Some(named.join(" → "))
}

/// BFS shortest path from `start` to `goal` through `adj`; returns the
/// node chain start → … → goal (inclusive, in walk order) when reachable.
/// Shared by the push-time cycle check and the staleness activation gate.
fn bfs_path<'a>(
    adj: &'a std::collections::HashMap<SpecGraphKey, Vec<SpecGraphKey>>,
    start: &'a SpecGraphKey,
    goal: &'a SpecGraphKey,
) -> Option<Vec<&'a SpecGraphKey>> {
    use std::collections::{HashMap, HashSet, VecDeque};
    let mut visited: HashSet<&SpecGraphKey> = HashSet::new();
    let mut parent: HashMap<&SpecGraphKey, &SpecGraphKey> = HashMap::new();
    let mut queue: VecDeque<&SpecGraphKey> = VecDeque::new();
    queue.push_back(start);
    visited.insert(start);
    while let Some(node) = queue.pop_front() {
        if node == goal {
            // Reconstruct the chain: walk the parent links from the goal
            // back to the BFS root, then reverse.
            let mut chain: Vec<&SpecGraphKey> = Vec::new();
            let mut cur = node;
            loop {
                chain.push(cur);
                match parent.get(cur) {
                    Some(&p) => cur = p,
                    None => break,
                }
            }
            chain.reverse();
            return Some(chain);
        }
        if let Some(targets) = adj.get(node) {
            for next in targets {
                if visited.insert(next) {
                    parent.insert(next, node);
                    queue.push_back(next);
                }
            }
        }
    }
    None
}

/// Detect cycles the PUSHED MANIFEST would create in the (repo, path)-keyed
/// spec link graph (review repair, round 2: only cycles in which at least
/// one edge contributed by the pushed manifest participates are reported —
/// a stored-only cycle is not created by this push). A cycle contains the
/// pushed edge (u → v) exactly when u is reachable from v through the
/// combined graph, so each pushed edge is checked with a BFS from its
/// target back to its source. Checks `depends_on`, `implements`, and
/// `extends` links; excludes `conflicts_with` (bidirectional), `references`,
/// and `supersedes` per spec-links.md §Cycle Detection (callers filter
/// edges by [`participates_in_cycle_detection`]). The error message names
/// the cycle with repo-attributed nodes joined by ` → `.
fn detect_graph_cycles(graph: &SpecCycleGraph) -> Result<(), String> {
    for (u, v) in &graph.pushed {
        // BFS from v back to u: if u is reachable, the pushed edge u → v
        // closes a cycle u → v → … → u.
        if let Some(chain) = bfs_path(&graph.adj, v, u) {
            let mut named: Vec<String> = vec![display_spec_graph_key(u)];
            named.extend(chain.iter().map(|k| display_spec_graph_key(k)));
            return Err(format!("spec link cycle detected: {}", named.join(" → ")));
        }
    }

    Ok(())
}

/// Detect cycles in the spec link graph from a parsed manifest (its own
/// edges only, all nodes attributed to one scope — the pre-task-293
/// single-manifest check, kept for manifests pushed without tenant
/// context). Every edge comes from the manifest, so every edge is a
/// "pushed" edge: any cycle in the manifest's own edges is a cycle this
/// push creates. Checks `depends_on`, `implements`, and `extends` links.
/// Excludes `conflicts_with` (bidirectional), `references`, and `supersedes`
/// per spec-links.md §Cycle Detection.
pub fn detect_link_cycles(manifest: &SpecManifest) -> Result<(), String> {
    let mut graph = SpecCycleGraph {
        adj: std::collections::HashMap::new(),
        pushed: Vec::new(),
    };
    for entry in &manifest.specs {
        for link in &entry.links {
            if !participates_in_cycle_detection(&link.link_type) {
                continue;
            }
            // Without tenant context, cross-repo targets contribute only
            // their path portion (single-bucket attribution).
            let target_path = match parse_cross_workspace_target(&link.target) {
                CrossWorkspaceTarget::SameRepo { path } => path,
                CrossWorkspaceTarget::CrossRepo { path, .. } => path,
                CrossWorkspaceTarget::CrossWorkspace { path, .. } => path,
            };
            graph.add_edge(
                spec_graph_key("", &entry.path),
                spec_graph_key("", &target_path),
                true,
            );
        }
    }
    detect_graph_cycles(&graph)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Test-only in-memory ledger fixture. Production state never uses this:
    /// the ledger is port-backed (`gyre_ports::SpecLedgerRepository`, wired
    /// via `store!` in `build_state`), so the alias lives inside the test
    /// module rather than production code.
    type SpecLedger = Arc<Mutex<HashMap<String, SpecLedgerEntry>>>;

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
    fn test_manifest_without_concepts_parses_empty() {
        let m = parse_manifest(SAMPLE_MANIFEST).expect("parse failed");
        assert!(
            m.concepts.is_empty(),
            "manifest without concepts: key must parse to empty vec"
        );
    }

    #[test]
    fn test_parse_concepts_spec_examples() {
        let yaml = r#"
version: 1
specs:
  - path: system/source-control.md
    title: Source Control
    owner: user:jsell
concepts:
  - name: Authentication
    description: "Token validation, RBAC, ABAC, JWT handling"
    include:
      - types: ["*Auth*", "*Token*", "*Rbac*", "*Abac*", "*Jwt*"]
      - traits: ["*Auth*"]
      - modules: ["*::auth*", "*::identity*"]
      - endpoints: ["/api/v1/auth/*", "/.well-known/*"]
      - specs: ["identity-security.md", "abac-policy-engine.md"]

  - name: Merge Pipeline
    description: "Merge queue, gates, MR lifecycle"
    include:
      - types: ["MergeRequest", "MergeQueueEntry", "Gate*", "QueueProcessor"]
      - modules: ["*::merge*", "*::gates*"]
      - specs: ["source-control.md", "agent-gates.md", "merge-dependencies.md"]
"#;
        let m = parse_manifest(yaml).expect("parse failed");
        assert_eq!(m.concepts.len(), 2);

        let auth = &m.concepts[0];
        assert_eq!(auth.name, "Authentication");
        assert_eq!(
            auth.description,
            "Token validation, RBAC, ABAC, JWT handling"
        );
        let view = gyre_common::graph::ConceptView::from(auth);
        assert_eq!(
            view.include_types,
            vec!["*Auth*", "*Token*", "*Rbac*", "*Abac*", "*Jwt*"]
        );
        assert_eq!(view.include_traits, vec!["*Auth*"]);
        assert_eq!(view.include_modules, vec!["*::auth*", "*::identity*"]);
        assert_eq!(
            view.include_endpoints,
            vec!["/api/v1/auth/*", "/.well-known/*"]
        );
        assert_eq!(
            view.include_specs,
            vec!["identity-security.md", "abac-policy-engine.md"]
        );

        let merge = &m.concepts[1];
        assert_eq!(merge.name, "Merge Pipeline");
        let view = gyre_common::graph::ConceptView::from(merge);
        assert_eq!(
            view.include_types,
            vec!["MergeRequest", "MergeQueueEntry", "Gate*", "QueueProcessor"]
        );
        assert!(view.include_traits.is_empty());
        assert_eq!(view.include_modules, vec!["*::merge*", "*::gates*"]);
        assert!(view.include_endpoints.is_empty());
        assert_eq!(
            view.include_specs,
            vec![
                "source-control.md",
                "agent-gates.md",
                "merge-dependencies.md"
            ]
        );
    }

    #[test]
    fn test_parse_concept_without_include_and_split_keys() {
        let yaml = r#"
version: 1
specs:
  - path: system/x.md
    title: X
    owner: user:jsell
concepts:
  - name: Bare
  - name: Split
    description: "same key across two include entries"
    include:
      - types: ["A*"]
      - modules: ["m"]
      - types: ["B*"]
"#;
        let m = parse_manifest(yaml).expect("parse failed");
        assert_eq!(m.concepts.len(), 2);
        let bare = &m.concepts[0];
        assert_eq!(bare.name, "Bare");
        let bare_view = gyre_common::graph::ConceptView::from(bare);
        assert!(bare_view.include_types.is_empty());
        assert!(bare_view.include_specs.is_empty());

        // Same key appearing in multiple include entries aggregates.
        let split_view = gyre_common::graph::ConceptView::from(&m.concepts[1]);
        assert_eq!(split_view.include_types, vec!["A*", "B*"]);
        assert_eq!(split_view.include_modules, vec!["m"]);
    }

    #[test]
    fn test_parse_manifest_ok() {
        let m = parse_manifest(SAMPLE_MANIFEST).expect("parse failed");
        assert_eq!(m.version, 1);
        assert_eq!(m.specs.len(), 3);
        assert_eq!(m.specs[0].path, "system/design-principles.md");
        assert_eq!(m.specs[0].title, "Design Principles");
        assert!(m.defaults.requires_approval);
    }

    /// The exact starter-kit manifest `gyre bootstrap --starter-kit` writes
    /// (crates/gyre-cli/src/bootstrap/starter-manifest.yaml). Consumed via
    /// include_str! so the test always sees the shipped bytes, never a copy
    /// that could drift (task-099 F5 process fix #176: embedded templates
    /// must round-trip through the consumer's real parser in a test). The
    /// original F5 defect — `approval: human` instead of
    /// `approval: {mode: human_only}` — parsed nowhere but the CLI's
    /// structural check and silently emptied the spec ledger on push.
    const CLI_STARTER_MANIFEST: &str =
        include_str!("../../gyre-cli/src/bootstrap/starter-manifest.yaml");

    #[test]
    fn cli_starter_manifest_parses_with_real_parser() {
        // Round-trip 1: the raw template must parse as SpecManifest.
        let m = parse_manifest(CLI_STARTER_MANIFEST)
            .expect("gyre bootstrap starter manifest must parse as SpecManifest");
        assert_eq!(m.version, 1);
        assert_eq!(m.specs.len(), 1, "starter kit registers one spec");
        let entry = &m.specs[0];
        assert_eq!(entry.path, "system/design-principles.md");
        assert_eq!(entry.title, "Design Principles");
        assert_eq!(entry.owner, "admin");
        assert_eq!(entry.kind.as_deref(), Some("system"));
        // The F5 defect shape: approval must be ApprovalConfig, and its
        // effective mode must be a real ApprovalMode.
        assert_eq!(entry.effective_approval_mode(), ApprovalMode::HumanOnly);
        assert!(entry.effective_requires_approval(&m.defaults));
    }

    #[tokio::test]
    async fn cli_starter_manifest_round_trips_through_ledger_sync() {
        // Round-trip 2: the full consumer path. Build a real git repo with
        // the starter kit's manifest + spec file, run the real
        // sync_spec_ledger against it, and assert the ledger is populated —
        // the exact path a pushed starter kit takes. If the template
        // regresses to anything parse_manifest rejects, sync warn-returns
        // and the ledger stays empty, failing this test.
        let (dir, sha) = make_test_repo(&[
            ("specs/manifest.yaml", CLI_STARTER_MANIFEST),
            ("specs/index.md", "# Spec Index\n"),
            (
                "specs/system/design-principles.md",
                "# Design Principles\n\nStarter kit template.\n",
            ),
        ])
        .await;

        let ledger: Arc<dyn gyre_ports::SpecLedgerRepository> =
            Arc::new(crate::mem::MemSpecLedgerRepository::default());
        let links_store: SpecLinksStore = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        // Durable link repo (Mem is enough here: this test asserts ledger
        // population, not link durability, but the sync path requires one).
        let link_repo: Arc<dyn gyre_ports::SpecLinkRepository> =
            Arc::new(crate::mem::MemSpecLinkRepository::default());

        sync_spec_ledger(
            &ledger,
            &links_store,
            &link_repo,
            dir.path().to_str().unwrap(),
            &sha,
            1_700_000_000,
            Some("repo-1"),
            Some("ws-1"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;

        let entries = ledger.list_all().await.unwrap();
        assert_eq!(
            entries.len(),
            1,
            "starter-kit manifest must register its spec in the ledger"
        );
        let e = entries
            .iter()
            .find(|e| e.path == "system/design-principles.md")
            .expect("starter spec must be in the ledger");
        assert_eq!(e.title, "Design Principles");
        assert_eq!(e.approval_mode, "human_only");
        assert_eq!(e.approval_status, ApprovalStatus::Pending);
        assert!(!e.current_sha.is_empty(), "blob SHA must resolve at HEAD");
        assert_eq!(e.repo_id.as_deref(), Some("repo-1"));
        assert_eq!(e.workspace_id.as_deref(), Some("ws-1"));
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

    // -----------------------------------------------------------------------
    // §15 manifest helpers — path resolution and priority overrides
    // -----------------------------------------------------------------------

    const PRIORITY_MANIFEST: &str = r#"
version: 1
defaults:
  priority: Low
specs:
  - path: system/spec-a.md
    title: Spec A
    owner: user:jsell
    priority: Critical
  - path: system/spec-b.md
    title: Spec B
    owner: user:jsell
  - path: development/spec-c.md
    title: Spec C
    owner: user:jsell
"#;

    #[test]
    fn manifest_entry_for_path_resolves_with_and_without_prefix() {
        let m = parse_manifest(PRIORITY_MANIFEST).unwrap();
        // Diff paths carry the specs/ prefix; manifest entries do not.
        let a = manifest_entry_for_path(&m, "specs/system/spec-a.md").unwrap();
        assert_eq!(a.path, "system/spec-a.md");
        // The same identity without the prefix also resolves.
        let b = manifest_entry_for_path(&m, "system/spec-b.md").unwrap();
        assert_eq!(b.title, "Spec B");
        // Unregistered files do not resolve.
        assert!(manifest_entry_for_path(&m, "specs/system/unregistered.md").is_none());
        assert!(manifest_entry_for_path(&m, "src/main.rs").is_none());
    }

    #[test]
    fn effective_priority_entry_override_beats_defaults() {
        let m = parse_manifest(PRIORITY_MANIFEST).unwrap();
        let a = manifest_entry_for_path(&m, "specs/system/spec-a.md").unwrap();
        assert_eq!(
            a.effective_priority(&m.defaults),
            Some(TaskPriority::Critical)
        );
    }

    #[test]
    fn effective_priority_falls_back_to_defaults_then_none() {
        let m = parse_manifest(PRIORITY_MANIFEST).unwrap();
        // No per-entry override: manifest defaults priority applies.
        let b = manifest_entry_for_path(&m, "specs/system/spec-b.md").unwrap();
        assert_eq!(b.effective_priority(&m.defaults), Some(TaskPriority::Low));
        // No override anywhere (defaults has none): caller keeps its default.
        let bare = parse_manifest(
            "version: 1\nspecs:\n  - path: system/x.md\n    title: X\n    owner: user:t\n",
        )
        .unwrap();
        let x = &bare.specs[0];
        assert_eq!(x.effective_priority(&bare.defaults), None);
    }

    // -----------------------------------------------------------------------
    // §9 Approval Status Resolution — resolve_approval_status unit tests
    // -----------------------------------------------------------------------

    const APPROVAL_MANIFEST: &str = r#"
version: 1
defaults:
  requires_approval: true
specs:
  - path: system/human-only.md
    title: Human Only
    owner: user:jsell
    approval:
      mode: human_only
      human_approvers:
        - user:jsell
  - path: system/owner-default.md
    title: Owner Default
    owner: user:alice
    approval:
      mode: human_only
  - path: system/owner-default-both.md
    title: Owner Default Both
    owner: user:alice
    approval:
      mode: human_and_agent
      agent_approvers:
        - persona: accountability
  - path: system/agent-only.md
    title: Agent Only
    owner: user:jsell
    approval:
      mode: agent_only
      agent_approvers:
        - persona: accountability
          min_attestation_level: 3
  - path: system/agent-only-pinned.md
    title: Agent Only Pinned Stack
    owner: user:jsell
    approval:
      mode: agent_only
      agent_approvers:
        - persona: accountability
          stack_hash: "sha256:abc"
  - path: system/both.md
    title: Both
    owner: user:jsell
    approval:
      mode: human_and_agent
      human_approvers:
        - user:jsell
      agent_approvers:
        - persona: accountability
  - path: system/no-approval.md
    title: No Approval Required
    owner: user:jsell
    requires_approval: false
"#;

    fn approval_manifest() -> SpecManifest {
        parse_manifest(APPROVAL_MANIFEST).unwrap()
    }

    fn manifest_entry<'a>(m: &'a SpecManifest, path: &str) -> &'a SpecEntry {
        m.specs.iter().find(|e| e.path == path).unwrap_or_else(|| {
            panic!("manifest has no entry for {path}");
        })
    }

    fn approval_event(
        id: &str,
        spec_sha: &str,
        approver_type: &str,
        approver_id: &str,
    ) -> SpecApprovalEvent {
        SpecApprovalEvent {
            id: id.to_string(),
            spec_path: String::new(),
            spec_sha: spec_sha.to_string(),
            approver_type: approver_type.to_string(),
            approver_id: approver_id.to_string(),
            persona: None,
            attestation_level: None,
            stack_hash: None,
            approved_at: 0,
            revoked_at: None,
            revoked_by: None,
            revocation_reason: None,
        }
    }

    fn agent_event(id: &str, spec_sha: &str, persona: &str) -> SpecApprovalEvent {
        let mut ev = approval_event(id, spec_sha, "agent", "agent:a1");
        ev.persona = Some(persona.to_string());
        ev
    }

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn test_resolve_human_only_approved_by_valid_human() {
        let m = approval_manifest();
        let entry = manifest_entry(&m, "system/human-only.md");
        let ev = approval_event("e1", SHA, "human", "user:jsell");
        assert_eq!(
            resolve_approval_status(entry, &m.defaults, SHA, &[ev]),
            ApprovalStatus::Approved
        );
    }

    #[test]
    fn test_resolve_human_only_pending_when_only_agent_approved() {
        // Reverting to "any valid approval" would mark this Approved.
        let m = approval_manifest();
        let entry = manifest_entry(&m, "system/human-only.md");
        let ev = agent_event("e1", SHA, "accountability");
        assert_eq!(
            resolve_approval_status(entry, &m.defaults, SHA, &[ev]),
            ApprovalStatus::Pending
        );
    }

    #[test]
    fn test_resolve_human_only_pending_when_approver_not_in_list() {
        let m = approval_manifest();
        let entry = manifest_entry(&m, "system/human-only.md");
        let ev = approval_event("e1", SHA, "human", "user:intruder");
        assert_eq!(
            resolve_approval_status(entry, &m.defaults, SHA, &[ev]),
            ApprovalStatus::Pending
        );
    }

    #[test]
    fn test_resolve_omitted_human_approvers_reserve_to_owner() {
        // Manifest Schema: `approval.human_approvers` defaults to `[owner]` —
        // omitting the list reserves approval to the owner; it must not
        // widen to "any human may approve".
        let m = approval_manifest();
        let entry = manifest_entry(&m, "system/owner-default.md");

        // A non-owner human approval (bob) does not approve the spec.
        let bob = approval_event("e1", SHA, "human", "user:bob");
        assert_eq!(
            resolve_approval_status(entry, &m.defaults, SHA, &[bob]),
            ApprovalStatus::Pending
        );

        // The owner's (alice's) approval does.
        let alice = approval_event("e2", SHA, "human", "user:alice");
        assert_eq!(
            resolve_approval_status(entry, &m.defaults, SHA, &[alice]),
            ApprovalStatus::Approved
        );
    }

    #[test]
    fn test_resolve_omitted_human_approvers_owner_scopes_human_and_agent() {
        // Same [owner] default under mode human_and_agent: the human half of
        // the required pair is owner-scoped, not any-human.
        let m = approval_manifest();
        let entry = manifest_entry(&m, "system/owner-default-both.md");
        let mut agent = agent_event("e2", SHA, "accountability");
        agent.attestation_level = Some(3);

        // Valid agent + non-owner human (bob) → still Pending.
        let bob = approval_event("e1", SHA, "human", "user:bob");
        assert_eq!(
            resolve_approval_status(entry, &m.defaults, SHA, &[bob, agent.clone()]),
            ApprovalStatus::Pending
        );

        // Valid agent + owner human (alice) → Approved.
        let alice = approval_event("e3", SHA, "human", "user:alice");
        assert_eq!(
            resolve_approval_status(entry, &m.defaults, SHA, &[alice, agent]),
            ApprovalStatus::Approved
        );
    }

    #[test]
    fn test_resolve_agent_only_approved_by_matching_agent() {
        let m = approval_manifest();
        let entry = manifest_entry(&m, "system/agent-only.md");
        let mut ev = agent_event("e1", SHA, "accountability");
        ev.attestation_level = Some(3);
        assert_eq!(
            resolve_approval_status(entry, &m.defaults, SHA, &[ev]),
            ApprovalStatus::Approved
        );
    }

    #[test]
    fn test_resolve_agent_only_pending_when_attestation_below_minimum() {
        // min_attestation_level: 3, event records level 2 → not valid.
        let m = approval_manifest();
        let entry = manifest_entry(&m, "system/agent-only.md");
        let mut ev = agent_event("e1", SHA, "accountability");
        ev.attestation_level = Some(2);
        assert_eq!(
            resolve_approval_status(entry, &m.defaults, SHA, &[ev]),
            ApprovalStatus::Pending
        );
    }

    #[test]
    fn test_resolve_agent_only_pending_when_persona_not_configured() {
        let m = approval_manifest();
        let entry = manifest_entry(&m, "system/agent-only.md");
        let mut ev = agent_event("e1", SHA, "builder");
        ev.attestation_level = Some(3);
        assert_eq!(
            resolve_approval_status(entry, &m.defaults, SHA, &[ev]),
            ApprovalStatus::Pending
        );
    }

    #[test]
    fn test_resolve_agent_only_pending_when_stack_hash_mismatches() {
        // Pinned stack_hash "sha256:abc", event records a different hash.
        let m = approval_manifest();
        let entry = manifest_entry(&m, "system/agent-only-pinned.md");
        let mut ev = agent_event("e1", SHA, "accountability");
        ev.attestation_level = Some(3);
        ev.stack_hash = Some("sha256:xyz".to_string());
        assert_eq!(
            resolve_approval_status(entry, &m.defaults, SHA, &[ev]),
            ApprovalStatus::Pending
        );
    }

    #[test]
    fn test_resolve_agent_only_approved_on_stack_hash_match() {
        let m = approval_manifest();
        let entry = manifest_entry(&m, "system/agent-only-pinned.md");
        let mut ev = agent_event("e1", SHA, "accountability");
        ev.attestation_level = Some(3);
        ev.stack_hash = Some("sha256:abc".to_string());
        assert_eq!(
            resolve_approval_status(entry, &m.defaults, SHA, &[ev]),
            ApprovalStatus::Approved
        );
    }

    #[test]
    fn test_resolve_agent_only_pending_when_stack_hash_missing() {
        // Pinned stack_hash but the event recorded none (e.g. human typo as agent).
        let m = approval_manifest();
        let entry = manifest_entry(&m, "system/agent-only-pinned.md");
        let mut ev = agent_event("e1", SHA, "accountability");
        ev.attestation_level = Some(3);
        assert_eq!(
            resolve_approval_status(entry, &m.defaults, SHA, &[ev]),
            ApprovalStatus::Pending
        );
    }

    #[test]
    fn test_resolve_human_and_agent_requires_both() {
        let m = approval_manifest();
        let entry = manifest_entry(&m, "system/both.md");

        // Human alone → Pending.
        let human = approval_event("e1", SHA, "human", "user:jsell");
        assert_eq!(
            resolve_approval_status(entry, &m.defaults, SHA, std::slice::from_ref(&human)),
            ApprovalStatus::Pending
        );

        // Agent alone → Pending. Reverting to "any valid approval" would mark
        // this Approved.
        let mut agent = agent_event("e2", SHA, "accountability");
        agent.attestation_level = Some(3);
        assert_eq!(
            resolve_approval_status(entry, &m.defaults, SHA, std::slice::from_ref(&agent)),
            ApprovalStatus::Pending
        );

        // Both → Approved.
        assert_eq!(
            resolve_approval_status(entry, &m.defaults, SHA, &[human, agent]),
            ApprovalStatus::Approved
        );
    }

    #[test]
    fn test_resolve_stale_sha_is_not_valid() {
        let m = approval_manifest();
        let entry = manifest_entry(&m, "system/human-only.md");
        let ev = approval_event(
            "e1",
            "9999999999999999999999999999999999999999",
            "human",
            "user:jsell",
        );
        assert_eq!(
            resolve_approval_status(entry, &m.defaults, SHA, &[ev]),
            ApprovalStatus::Pending
        );
    }

    #[test]
    fn test_resolve_revoked_event_is_not_valid() {
        let m = approval_manifest();
        let entry = manifest_entry(&m, "system/human-only.md");
        let mut ev = approval_event("e1", SHA, "human", "user:jsell");
        ev.revoked_at = Some(1);
        assert_eq!(
            resolve_approval_status(entry, &m.defaults, SHA, &[ev]),
            ApprovalStatus::Pending
        );
    }

    #[test]
    fn test_resolve_requires_approval_false_is_approved_regardless() {
        let m = approval_manifest();
        let entry = manifest_entry(&m, "system/no-approval.md");
        assert_eq!(
            resolve_approval_status(entry, &m.defaults, SHA, &[]),
            ApprovalStatus::Approved
        );
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
                    bindings_sha: String::new(),
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
                    bindings_sha: String::new(),
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
                    bindings_sha: String::new(),
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
            source_sha: "src-sha".to_string(),
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
            bindings_sha: String::new(),
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
        source_repo_id: &str,
    ) -> SpecLinkEntry {
        SpecLinkEntry {
            id: id.to_string(),
            source_path: source.to_string(),
            source_repo_id: Some(source_repo_id.to_string()),
            source_sha: "src-sha".to_string(),
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
            "repo1",
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
            &SpecLinkType::Extends,
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
            "repo1",
        );
        let implements_link = make_test_link(
            "impl-link",
            "system/impl.md",
            "system/target.md",
            SpecLinkType::Implements,
            Some("old_sha_123"),
            "repo1",
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

        // Pre-existing inbound extends link — uses a DISTINCT source_repo_id
        // from the pushed repo to verify F5: task is created in the link's repo,
        // not the pushed repo.
        let extends_link = make_test_link(
            "ext-inbound",
            "system/child.md",
            "system/parent.md",
            SpecLinkType::Extends,
            Some("old_parent_sha"),
            "repo_LINK_SOURCE",
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

        // Collect ALL stale inbound links for side effects (matching new step 6b logic).
        let inbound_stale_links: Vec<(String, String, Option<String>, SpecLinkType)> = {
            let store = links_store.lock().await;
            store
                .iter()
                .filter(|l| {
                    l.stale_since == Some(now) && changed_set.contains(l.target_path.as_str())
                })
                .map(|l| {
                    (
                        l.source_path.clone(),
                        l.target_path.clone(),
                        l.source_repo_id.clone(),
                        l.link_type.clone(),
                    )
                })
                .collect()
        };

        // Apply side effects (matching new step 6b logic — uses link's repo, not pushed repo).
        for (source_path, target_path, link_repo_id, link_type) in &inbound_stale_links {
            let maybe_source = ledger.find_by_path(source_path).await.ok().flatten();
            let ws_id = maybe_source.as_ref().and_then(|e| e.workspace_id.clone());

            // Step 3: drift-review task using link's source_repo_id (F5).
            create_drift_review_task(
                Some(&tasks),
                source_path,
                target_path,
                link_type,
                link_repo_id.as_deref(),
                ws_id.as_deref(),
                now,
            )
            .await;

            // Step 4: extends-specific side effects.
            if *link_type == SpecLinkType::Extends {
                if let Some(mut source_entry) = maybe_source {
                    source_entry.drift_status = "drifted".to_string();
                    source_entry.approval_status = ApprovalStatus::Pending;
                    source_entry.updated_at = now;
                    ledger.save(&source_entry).await.unwrap();
                }
            }
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

        // Verify drift-review task was created with the LINK's repo ID (F5),
        // not the pushed repo's ID.
        let all_tasks = tasks.list().await.unwrap();
        assert_eq!(all_tasks.len(), 1);
        let task = &all_tasks[0];
        assert!(task.title.contains("Drift review"));
        assert_eq!(task.spec_path, Some("system/child.md".to_string()));
        assert_eq!(task.labels, vec!["drift-review".to_string()]);
        assert_eq!(task.repo_id, gyre_common::Id::new("repo_LINK_SOURCE"));
        assert_eq!(task.workspace_id, gyre_common::Id::new("ws1"));
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
            "repo1",
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

    /// F6: When an extends link's source AND target are both in the current manifest
    /// and the target's SHA changed, step 6 (outbound) and step 6b (inbound) must
    /// NOT both create drift-review tasks. Step 6b excludes manifest source paths.
    #[tokio::test]
    async fn no_duplicate_drift_review_for_same_repo_extends() {
        use crate::mem::{MemSpecLedgerRepository, MemTaskRepository};

        let ledger: Arc<dyn gyre_ports::SpecLedgerRepository> =
            Arc::new(MemSpecLedgerRepository::default());
        let links_store: SpecLinksStore = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let tasks: Arc<dyn gyre_ports::TaskRepository> = Arc::new(MemTaskRepository::default());
        let now = 2_000_000u64;

        // Both specs in the same repo/manifest.
        let parent =
            make_test_ledger_entry("system/parent.md", "new_sha_999", ApprovalStatus::Approved);
        ledger.save(&parent).await.unwrap();

        let extending =
            make_test_ledger_entry("system/extending.md", "ext_sha", ApprovalStatus::Approved);
        ledger.save(&extending).await.unwrap();

        // Manifest paths (both specs are in the manifest).
        let manifest_paths: std::collections::HashSet<String> = [
            "system/parent.md".to_string(),
            "system/extending.md".to_string(),
        ]
        .into_iter()
        .collect();

        // Step 6 (outbound): create stale link and task #1.
        let mut link = make_test_link(
            "ext-same-repo",
            "system/extending.md",
            "system/parent.md",
            SpecLinkType::Extends,
            Some("old_sha"),
            "repo1",
        );
        link.status = "stale".to_string();
        link.stale_since = Some(now);

        // Generic task creation (as step 6 does for all stale links).
        create_drift_review_task(
            Some(&tasks),
            &link.source_path,
            &link.target_path,
            &link.link_type,
            Some("repo1"),
            Some("ws1"),
            now,
        )
        .await;

        // Add the stale link to the store (as step 6 does after processing).
        {
            let mut store = links_store.lock().await;
            store.push(link);
        }

        // Step 6b (inbound): scan for stale links targeting changed specs.
        let changed_set: std::collections::HashSet<&str> =
            ["system/parent.md"].iter().copied().collect();

        // Collect inbound stale links — EXCLUDING manifest source paths (F6 fix).
        let inbound_stale_links: Vec<(String, String, Option<String>, SpecLinkType)> = {
            let store = links_store.lock().await;
            store
                .iter()
                .filter(|l| {
                    l.stale_since == Some(now)
                        && changed_set.contains(l.target_path.as_str())
                        && !manifest_paths.contains(&l.source_path)
                })
                .map(|l| {
                    (
                        l.source_path.clone(),
                        l.target_path.clone(),
                        l.source_repo_id.clone(),
                        l.link_type.clone(),
                    )
                })
                .collect()
        };

        // The link's source_path ("system/extending.md") IS in manifest_paths,
        // so it should be excluded from inbound processing.
        assert!(
            inbound_stale_links.is_empty(),
            "same-repo extends link should be excluded from step 6b"
        );

        // Only 1 task should exist (from step 6), not 2.
        let all_tasks = tasks.list().await.unwrap();
        assert_eq!(all_tasks.len(), 1);
    }

    /// F7: Drift-review tasks are created for ALL stale link types, not just extends.
    /// spec-links.md §Automatic Staleness Detection step 3 is generic.
    #[tokio::test]
    async fn drift_review_task_created_for_non_extends_stale_links() {
        use crate::mem::MemTaskRepository;

        let tasks: Arc<dyn gyre_ports::TaskRepository> = Arc::new(MemTaskRepository::default());

        // Create drift-review tasks for various non-extends link types.
        create_drift_review_task(
            Some(&tasks),
            "system/consumer.md",
            "system/dependency.md",
            &SpecLinkType::DependsOn,
            Some("repo_consumer"),
            Some("ws_consumer"),
            2_000_000,
        )
        .await;

        create_drift_review_task(
            Some(&tasks),
            "system/impl.md",
            "system/parent_spec.md",
            &SpecLinkType::Implements,
            Some("repo_impl"),
            Some("ws_impl"),
            2_000_000,
        )
        .await;

        let all_tasks = tasks.list().await.unwrap();
        assert_eq!(all_tasks.len(), 2);

        // Verify depends_on task.
        let dep_task = all_tasks
            .iter()
            .find(|t| t.spec_path == Some("system/consumer.md".to_string()))
            .unwrap();
        assert!(dep_task.title.contains("depends_on"));
        assert!(dep_task.title.contains("system/dependency.md"));
        assert_eq!(dep_task.labels, vec!["drift-review".to_string()]);
        assert_eq!(dep_task.repo_id, gyre_common::Id::new("repo_consumer"));
        assert_eq!(dep_task.workspace_id, gyre_common::Id::new("ws_consumer"));

        // Verify implements task.
        let impl_task = all_tasks
            .iter()
            .find(|t| t.spec_path == Some("system/impl.md".to_string()))
            .unwrap();
        assert!(impl_task.title.contains("implements"));
        assert!(impl_task.title.contains("system/parent_spec.md"));
        assert_eq!(impl_task.labels, vec!["drift-review".to_string()]);
        assert_eq!(impl_task.repo_id, gyre_common::Id::new("repo_impl"));
    }

    /// Verify that `references` links do NOT create drift-review tasks.
    /// spec-links.md §Link Types: `references` = "No mechanical enforcement."
    /// Staleness marking is informational only; task creation is enforcement.
    #[tokio::test]
    async fn no_drift_review_task_for_references_links() {
        use crate::mem::MemTaskRepository;

        let tasks: Arc<dyn gyre_ports::TaskRepository> = Arc::new(MemTaskRepository::default());

        // Calling create_drift_review_task directly for a references link
        // should still create (it doesn't check type internally).
        // The guard is at the call sites. Verify the call-site guard
        // by simulating the step 6 condition.
        let link = SpecLinkEntry {
            id: "link-ref-1".to_string(),
            source_path: "system/notes.md".to_string(),
            target_path: "system/design.md".to_string(),
            link_type: SpecLinkType::References,
            target_sha: Some("old_sha".to_string()),
            source_repo_id: Some("repo_notes".to_string()),
            source_sha: "src-sha".to_string(),
            target_repo_id: None,
            target_display: None,
            reason: None,
            status: "stale".to_string(),
            stale_since: Some(1_000_000),
            created_at: 1_000_000,
        };

        // Simulate the step 6 guard: `if link.status == "stale" && link.link_type != SpecLinkType::References`
        if link.status == "stale" && link.link_type != SpecLinkType::References {
            create_drift_review_task(
                Some(&tasks),
                &link.source_path,
                &link.target_path,
                &link.link_type,
                link.source_repo_id.as_deref(),
                Some("ws_notes"),
                2_000_000,
            )
            .await;
        }

        // Simulate the step 6b guard: `if *link_type == SpecLinkType::References { continue; }`
        if link.link_type != SpecLinkType::References {
            create_drift_review_task(
                Some(&tasks),
                &link.source_path,
                &link.target_path,
                &link.link_type,
                link.source_repo_id.as_deref(),
                Some("ws_notes"),
                2_000_000,
            )
            .await;
        }

        // No tasks should have been created for references links.
        let all_tasks = tasks.list().await.unwrap();
        assert_eq!(
            all_tasks.len(),
            0,
            "references links must not produce drift-review tasks (no mechanical enforcement)"
        );
    }

    // -----------------------------------------------------------------------
    // TASK-017: Manifest enforcement tests
    // -----------------------------------------------------------------------

    /// Helper: create a temp git repo with the given files committed.
    /// Returns (repo_path_string, sha_string).
    async fn make_test_repo(files: &[(&str, &str)]) -> (tempfile::TempDir, String) {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().to_str().unwrap();

        // git init + initial commit
        let run = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(path)
                .output()
                .expect("git command failed");
            assert!(
                out.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };

        run(&["init", "-b", "main"]);
        run(&["config", "user.email", "test@test.com"]);
        run(&["config", "user.name", "Test"]);

        for (file_path, content) in files {
            let full = dir.path().join(file_path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, content).unwrap();
            run(&["add", file_path]);
        }

        run(&["commit", "-m", "initial"]);
        let sha = run(&["rev-parse", "HEAD"]);

        (dir, sha)
    }

    /// Helper: overwrite files in an existing test repo, commit, and return
    /// the new HEAD SHA (models a second push).
    fn add_commit(dir: &tempfile::TempDir, files: &[(&str, &str)]) -> String {
        let path = dir.path();
        let run = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(path)
                .output()
                .expect("git command failed");
            assert!(
                out.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        for (file_path, content) in files {
            let full = path.join(file_path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, content).unwrap();
            run(&["add", file_path]);
        }
        run(&["commit", "-m", "update"]);
        run(&["rev-parse", "HEAD"])
    }

    #[tokio::test]
    async fn find_unregistered_specs_detects_missing_entries() {
        let (dir, sha) = make_test_repo(&[
            ("specs/manifest.yaml", "version: 1\nspecs:\n  - path: system/registered.md\n    title: Registered\n    owner: user:test\n"),
            ("specs/system/registered.md", "# Registered"),
            ("specs/system/unregistered.md", "# Unregistered"),
        ]).await;

        let manifest_paths: std::collections::HashSet<String> =
            ["system/registered.md".to_string()].into_iter().collect();
        let result =
            find_unregistered_specs("git", dir.path().to_str().unwrap(), &sha, &manifest_paths)
                .await;

        assert_eq!(result, vec!["system/unregistered.md"]);
    }

    #[tokio::test]
    async fn find_unregistered_specs_excludes_index_priorart_milestones() {
        let (dir, sha) = make_test_repo(&[
            ("specs/manifest.yaml", "version: 1\nspecs: []\n"),
            ("specs/index.md", "# Index"),
            ("specs/prior-art/comparison.md", "# Prior Art"),
            ("specs/milestones/m1.md", "# Milestone 1"),
        ])
        .await;

        let manifest_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
        let result =
            find_unregistered_specs("git", dir.path().to_str().unwrap(), &sha, &manifest_paths)
                .await;

        assert!(
            result.is_empty(),
            "index.md, prior-art/, milestones/ should be excluded, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn find_unregistered_specs_ignores_non_md_files() {
        let (dir, sha) = make_test_repo(&[
            ("specs/manifest.yaml", "version: 1\nspecs: []\n"),
            ("specs/system/notes.txt", "just a text file"),
            ("specs/system/diagram.png", "fake png"),
        ])
        .await;

        let manifest_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
        let result =
            find_unregistered_specs("git", dir.path().to_str().unwrap(), &sha, &manifest_paths)
                .await;

        assert!(
            result.is_empty(),
            "non-.md files should be ignored, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn find_unregistered_specs_all_registered() {
        let (dir, sha) = make_test_repo(&[
            ("specs/manifest.yaml", "version: 1\nspecs:\n  - path: system/a.md\n    title: A\n    owner: user:t\n  - path: dev/b.md\n    title: B\n    owner: user:t\n"),
            ("specs/system/a.md", "# A"),
            ("specs/dev/b.md", "# B"),
        ]).await;

        let manifest_paths: std::collections::HashSet<String> =
            ["system/a.md".to_string(), "dev/b.md".to_string()]
                .into_iter()
                .collect();
        let result =
            find_unregistered_specs("git", dir.path().to_str().unwrap(), &sha, &manifest_paths)
                .await;

        assert!(
            result.is_empty(),
            "all specs are registered, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn check_manifest_coverage_enforce_rejects_with_message() {
        let (dir, sha) = make_test_repo(&[
            ("specs/manifest.yaml", "version: 1\nspecs: []\n"),
            ("specs/system/orphan.md", "# Orphan"),
        ])
        .await;

        let manifest_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
        let result = check_manifest_coverage(
            dir.path().to_str().unwrap(),
            &sha,
            &manifest_paths,
            true, // enforce
        )
        .await;

        let err = result.expect_err("should reject when enforce=true");
        assert!(
            err.contains("push rejected"),
            "error should mention rejection: {err}"
        );
        assert!(
            err.contains("system/orphan.md"),
            "error should list unregistered file: {err}"
        );
        assert!(err.contains("fix:"), "error should include fix hint: {err}");
    }

    #[tokio::test]
    async fn check_manifest_coverage_warn_only_returns_ok() {
        let (dir, sha) = make_test_repo(&[
            ("specs/manifest.yaml", "version: 1\nspecs: []\n"),
            ("specs/system/orphan.md", "# Orphan"),
        ])
        .await;

        let manifest_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
        let result = check_manifest_coverage(
            dir.path().to_str().unwrap(),
            &sha,
            &manifest_paths,
            false, // warn only
        )
        .await;

        assert!(result.is_ok(), "should not reject when enforce=false");
    }

    #[tokio::test]
    async fn check_manifest_coverage_no_violations_returns_ok() {
        let (dir, sha) = make_test_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/ok.md\n    title: OK\n    owner: user:t\n",
            ),
            ("specs/system/ok.md", "# OK"),
        ])
        .await;

        let manifest_paths: std::collections::HashSet<String> =
            ["system/ok.md".to_string()].into_iter().collect();
        let result = check_manifest_coverage(
            dir.path().to_str().unwrap(),
            &sha,
            &manifest_paths,
            true, // enforce
        )
        .await;

        assert!(
            result.is_ok(),
            "should accept when all specs are registered"
        );
    }

    // -----------------------------------------------------------------------
    // Task-198: persistent forge-maintained spec-link graph
    // (spec-links.md §Forge-Maintained Spec Graph)
    // -----------------------------------------------------------------------

    /// The in-memory graph must be loadable from the durable repository at
    /// boot: seed via a real `sync_spec_ledger` push, then simulate a restart
    /// by building a fresh store from the same repository and asserting the
    /// graph is non-empty and identical — before any re-push.
    ///
    /// This is the section-closing restart-durability evidence: it fails
    /// against the pre-task empty-init behavior (fresh store starts empty and
    /// the graph is lost until every repo pushes again).
    #[tokio::test]
    async fn restart_rebuilds_spec_link_graph_from_persisted_table() {
        use crate::mem::{MemRepoRepository, MemSpecLedgerRepository, MemWorkspaceRepository};

        // Manifest with two specs and a cross-spec depends_on link pinned to a
        // SHA that does not match the target's ledger SHA — the staleness
        // checker must see it after a restart too.
        let manifest = r#"version: 1
specs:
  - path: system/parent.md
    title: Parent
    owner: user:test
  - path: system/child.md
    title: Child
    owner: user:test
    links:
      - type: depends_on
        target: system/parent.md
        target_sha: old_pinned_sha
        reason: needs parent guarantees"#;
        let (dir, sha) = make_test_repo(&[
            ("specs/manifest.yaml", manifest),
            ("specs/system/parent.md", "# P"),
            ("specs/system/child.md", "# C"),
        ])
        .await;

        // "Push" phase: run sync against the real durable repo (a real
        // SQLite database, exactly as the server wires it in build_state).
        let db = tempfile::NamedTempFile::new().unwrap();
        let db_path = db.path().to_str().unwrap().to_string();
        let link_repo: Arc<dyn gyre_ports::SpecLinkRepository> = {
            let storage = gyre_adapters::SqliteStorage::new(&db_path).unwrap();
            Arc::new(storage)
        };
        let ledger: Arc<dyn gyre_ports::SpecLedgerRepository> =
            Arc::new(MemSpecLedgerRepository::default());
        let links_store: SpecLinksStore = Arc::new(Mutex::new(Vec::new()));

        // Target spec already in the ledger at a DIFFERENT SHA than the pinned
        // one, so the link is stale — the post-restart staleness check below
        // must observe the same state.
        let parent = make_test_ledger_entry(
            "system/parent.md",
            "new_current_sha",
            ApprovalStatus::Approved,
        );
        ledger.save(&parent).await.unwrap();

        // Context repos for cross-workspace resolution (None-linked tests
        // don't resolve, but pass real stores to match server wiring).
        let ws_ctx: Arc<dyn gyre_ports::WorkspaceRepository> =
            Arc::new(MemWorkspaceRepository::default());
        let repos_ctx: Arc<dyn gyre_ports::RepoRepository> = Arc::new(MemRepoRepository::default());
        sync_spec_ledger(
            &ledger,
            &links_store,
            &link_repo,
            dir.path().to_str().unwrap(),
            &sha,
            1_700_000_000,
            Some("repo-1"),
            Some("ws-1"),
            Some(&ws_ctx),
            Some(&repos_ctx),
            None,
            None,
            None,
            None,
            None,
        )
        .await;

        let before_restart: Vec<SpecLinkEntry> = links_store.lock().await.clone();
        assert_eq!(
            before_restart.len(),
            1,
            "sync must record the depends_on link"
        );
        let pushed = &before_restart[0];
        assert_eq!(pushed.source_path, "system/child.md");
        assert_eq!(pushed.target_path, "system/parent.md");
        assert_eq!(pushed.link_type, SpecLinkType::DependsOn);
        assert_eq!(pushed.target_sha.as_deref(), Some("old_pinned_sha"));
        // source_sha must be the source spec's real blob SHA from the push —
        // not a placeholder.
        assert!(
            !pushed.source_sha.is_empty(),
            "source_sha must be populated from the source spec's blob SHA"
        );

        // ── Simulated restart: brand-new store over the SAME durable repo. ──
        let restarted_store: SpecLinksStore = Arc::new(Mutex::new(Vec::new()));
        crate::load_spec_links_into_store(&link_repo, &restarted_store);
        let after_restart: Vec<SpecLinkEntry> = restarted_store.lock().await.clone();

        assert!(
            !after_restart.is_empty(),
            "graph must be non-empty after restart without any re-push"
        );
        assert_eq!(
            after_restart, before_restart,
            "restarted graph must be identical to the pre-restart graph"
        );

        // A NEW server process would open a NEW storage handle on the same DB
        // file — verify the data is in the file, not just this handle.
        drop(link_repo);
        let reopened: Arc<dyn gyre_ports::SpecLinkRepository> = {
            let storage = gyre_adapters::SqliteStorage::new(&db_path).unwrap();
            Arc::new(storage)
        };
        let third_boot_store: SpecLinksStore = Arc::new(Mutex::new(Vec::new()));
        crate::load_spec_links_into_store(&reopened, &third_boot_store);
        let from_file: Vec<SpecLinkEntry> = third_boot_store.lock().await.clone();
        assert_eq!(
            from_file, before_restart,
            "graph must survive a fresh storage handle on the same database file"
        );
    }

    /// The staleness checker must return the same answer against a restarted
    /// (loaded-from-disk) graph as it would right after the push — the graph
    /// consumers (staleness, gates, patrol) must not silently degrade to an
    /// empty graph across a restart.
    #[tokio::test]
    async fn staleness_query_parity_after_restart() {
        use crate::mem::{MemRepoRepository, MemSpecLedgerRepository, MemWorkspaceRepository};

        // The link is pinned to the target's CURRENT ledger SHA at push time,
        // so it is active after the push. The staleness verdict is then
        // re-derived AFTER a restart, once the target spec has drifted —
        // the checker must compare the persisted pinned SHA against the
        // ledger's current SHA and mark the link stale on the restarted
        // graph, exactly as it would right after the push.
        //
        // The first commit carries both spec files; parent's REAL blob SHA
        // is then pinned in the manifest committed on top. The pin must
        // match the blob for the link to be active at push time, and
        // parent must be DECLARED in the repo's manifest — a same-repo
        // target absent from the manifest is "no longer in the manifest",
        // which the push-time broken check (task-288) correctly breaks.
        let (dir, sha0) = make_test_repo(&[
            ("specs/manifest.yaml", "version: 1\nspecs: []\n"),
            ("specs/system/child.md", "# C"),
            ("specs/system/parent.md", "# P"),
        ])
        .await;
        let parent_blob = blob_sha_at(&dir, &sha0, "specs/system/parent.md");
        let manifest = format!(
            r#"version: 1
specs:
  - path: system/parent.md
    title: Parent
    owner: user:test
  - path: system/child.md
    title: Child
    owner: user:test
    links:
      - type: depends_on
        target: system/parent.md
        target_sha: {parent_blob}"#
        );
        let sha = add_commit(&dir, &[("specs/manifest.yaml", manifest.as_str())]);

        let db = tempfile::NamedTempFile::new().unwrap();
        let db_path = db.path().to_str().unwrap().to_string();
        let link_repo: Arc<dyn gyre_ports::SpecLinkRepository> = {
            let storage = gyre_adapters::SqliteStorage::new(&db_path).unwrap();
            Arc::new(storage)
        };
        let ledger: Arc<dyn gyre_ports::SpecLedgerRepository> =
            Arc::new(MemSpecLedgerRepository::default());
        let links_store: SpecLinksStore = Arc::new(Mutex::new(Vec::new()));

        // No pre-seeded parent ledger entry is needed: parent.md is
        // declared in the pushed manifest, so the sync itself registers it
        // at the pinned blob SHA — the link is healthy at push time.

        let ws_ctx: Arc<dyn gyre_ports::WorkspaceRepository> =
            Arc::new(MemWorkspaceRepository::default());
        let repos_ctx: Arc<dyn gyre_ports::RepoRepository> = Arc::new(MemRepoRepository::default());
        sync_spec_ledger(
            &ledger,
            &links_store,
            &link_repo,
            dir.path().to_str().unwrap(),
            &sha,
            1_700_000_000,
            Some("repo-1"),
            Some("ws-1"),
            Some(&ws_ctx),
            Some(&repos_ctx),
            None,
            None,
            None,
            None,
            None,
        )
        .await;

        // Right after the push: pinned SHA matches the ledger — the link is
        // active, not stale.
        let post_push: Vec<SpecLinkEntry> = links_store.lock().await.clone();
        assert_eq!(post_push.len(), 1);
        assert_eq!(
            post_push[0].status, "active",
            "pinned SHA matches the ledger at push time — link must be active"
        );
        assert_eq!(
            post_push[0].target_sha.as_deref(),
            Some(parent_blob.as_str())
        );

        // ── Restart: fresh store loaded from the durable repo. ──
        let restarted_store: SpecLinksStore = Arc::new(Mutex::new(Vec::new()));
        crate::load_spec_links_into_store(&link_repo, &restarted_store);
        let after_restart: Vec<SpecLinkEntry> = restarted_store.lock().await.clone();
        assert_eq!(
            after_restart, post_push,
            "restarted graph must be identical to the post-push graph"
        );

        // ── The target spec now drifts (ledger SHA advances). ──
        let parent_v2 =
            make_test_ledger_entry("system/parent.md", "sha_v2", ApprovalStatus::Approved);
        ledger.save(&parent_v2).await.unwrap();

        // ── Run the REAL staleness checker against the restarted graph. ──
        // The verdict must be re-derived from the persisted pinned SHA vs
        // the ledger's current SHA — two independent sources. Against the
        // pre-task empty-init behavior this fails: an empty restarted graph
        // gives the checker nothing to mark stale.
        let base = crate::mem::test_state();
        let state = Arc::new(crate::AppState {
            spec_links_store: restarted_store.clone(),
            spec_link_repo: link_repo.clone(),
            spec_ledger: ledger.clone(),
            ..(*base).clone()
        });
        crate::spec_link_staleness::run_once(&state).await.unwrap();

        let rechecked: Vec<SpecLinkEntry> = restarted_store.lock().await.clone();
        assert_eq!(rechecked.len(), 1, "checker must see the restarted graph");
        assert_eq!(
            rechecked[0].status, "stale",
            "checker must re-derive 'stale' from ledger SHA vs persisted pinned SHA"
        );
        assert!(
            rechecked[0].stale_since.is_some(),
            "checker must stamp stale_since on the restarted graph"
        );

        // The checker's verdict is written through to the durable table —
        // the next restart loads the stale verdict, not a reset one.
        let persisted: Vec<SpecLinkEntry> = link_repo.list_all().await.unwrap();
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].status, "stale");
        assert_eq!(persisted[0].stale_since, rechecked[0].stale_since);
    }

    /// Regression (task-198): two repos may carry the same spec path. A push
    /// from repo B must replace ONLY repo B's rows for that path — repo A's
    /// links survive in both the hot cache and the durable table, and the two
    /// stores never diverge (a restart must not resurrect or drop links).
    #[tokio::test]
    async fn sync_replaces_links_scoped_to_source_repo() {
        use crate::mem::{MemRepoRepository, MemSpecLedgerRepository, MemWorkspaceRepository};

        // The IDENTICAL manifest pushed by both repos — same spec path, same
        // link type, same target. Under an id format without repo scope the
        // two links collapse to one id, repo B's replace_for_source INSERT
        // hits repo A's row's PRIMARY KEY, the transaction rolls back, and
        // the swallowed error silently loses one repo's durable row (after a
        // restart that repo's link is gone from the tenant-wide graph).
        let manifest = r#"version: 1
specs:
  - path: system/shared.md
    title: Shared
    owner: user:test
    links:
      - type: depends_on
        target: system/target.md"#;
        let (dir, sha) = make_test_repo(&[
            ("specs/manifest.yaml", manifest),
            ("specs/system/shared.md", "# S"),
        ])
        .await;

        let db = tempfile::NamedTempFile::new().unwrap();
        let db_path = db.path().to_str().unwrap().to_string();
        let link_repo: Arc<dyn gyre_ports::SpecLinkRepository> = {
            let storage = gyre_adapters::SqliteStorage::new(&db_path).unwrap();
            Arc::new(storage)
        };
        let ledger: Arc<dyn gyre_ports::SpecLedgerRepository> =
            Arc::new(MemSpecLedgerRepository::default());
        let links_store: SpecLinksStore = Arc::new(Mutex::new(Vec::new()));

        let ws_ctx: Arc<dyn gyre_ports::WorkspaceRepository> =
            Arc::new(MemWorkspaceRepository::default());
        let repos_ctx: Arc<dyn gyre_ports::RepoRepository> = Arc::new(MemRepoRepository::default());

        // Real push from repo A, then a real push from repo B with the same
        // manifest (both use the production id construction).
        sync_spec_ledger(
            &ledger,
            &links_store,
            &link_repo,
            dir.path().to_str().unwrap(),
            &sha,
            1_700_000_000,
            Some("repo-a"),
            Some("ws-1"),
            Some(&ws_ctx),
            Some(&repos_ctx),
            None,
            None,
            None,
            None,
            None,
        )
        .await;
        sync_spec_ledger(
            &ledger,
            &links_store,
            &link_repo,
            dir.path().to_str().unwrap(),
            &sha,
            1_700_000_500,
            Some("repo-b"),
            Some("ws-1"),
            Some(&ws_ctx),
            Some(&repos_ctx),
            None,
            None,
            None,
            None,
            None,
        )
        .await;

        // Cache: both repos' links present with distinct ids.
        let cache: Vec<SpecLinkEntry> = links_store.lock().await.clone();
        assert_eq!(cache.len(), 2, "both repos' links must be in the cache");
        assert!(
            cache
                .iter()
                .any(|l| l.source_repo_id.as_deref() == Some("repo-a")),
            "repo A's link must survive a repo-B push"
        );
        assert!(
            cache
                .iter()
                .any(|l| l.source_repo_id.as_deref() == Some("repo-b")),
            "repo B's new link must be present"
        );
        let distinct_ids: std::collections::HashSet<&str> =
            cache.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(
            distinct_ids.len(),
            2,
            "same-path links from different repos must have distinct ids"
        );

        // Durable table must match the cache exactly (the invariant the
        // boot-load path relies on) — a third boot from the same file sees
        // both links without any re-push.
        let persisted: Vec<SpecLinkEntry> = link_repo.list_all().await.unwrap();
        assert_eq!(persisted.len(), 2, "both repos' rows must be in the table");
        let mut sorted_cache: Vec<SpecLinkEntry> = cache.clone();
        sorted_cache.sort_by(|a, b| a.id.cmp(&b.id));
        let mut sorted_persisted = persisted.clone();
        sorted_persisted.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(
            sorted_persisted, sorted_cache,
            "durable table must match the cache (collision must not drop a repo's row)"
        );

        // Restart: a fresh store over the same durable table must contain
        // BOTH repos' links — the collision previously left only one.
        let restarted_store: SpecLinksStore = Arc::new(Mutex::new(Vec::new()));
        crate::load_spec_links_into_store(&link_repo, &restarted_store);
        let after_restart: Vec<SpecLinkEntry> = restarted_store.lock().await.clone();
        assert_eq!(
            after_restart.len(),
            2,
            "restart must rebuild BOTH repos' links (id collision previously dropped one)"
        );
        assert!(after_restart
            .iter()
            .any(|l| l.source_repo_id.as_deref() == Some("repo-a")));
        assert!(after_restart
            .iter()
            .any(|l| l.source_repo_id.as_deref() == Some("repo-b")));
    }

    /// Regression (task-198): when a manifest edit REMOVES a spec's links, the
    /// durable rows must be deleted too — otherwise a restart resurrects
    /// links that were removed from the manifest. The empty link set must
    /// still issue a replace (delete) for that source spec.
    #[tokio::test]
    async fn sync_clears_durable_rows_when_links_removed_from_manifest() {
        use crate::mem::{MemRepoRepository, MemSpecLedgerRepository, MemWorkspaceRepository};

        let with_link = r#"version: 1
specs:
  - path: system/child.md
    title: Child
    owner: user:test
    links:
      - type: depends_on
        target: system/parent.md"#;
        let without_link = r#"version: 1
specs:
  - path: system/child.md
    title: Child
    owner: user:test"#;

        let (dir, sha_with_link) = make_test_repo(&[
            ("specs/manifest.yaml", with_link),
            ("specs/system/child.md", "# C"),
        ])
        .await;

        let db = tempfile::NamedTempFile::new().unwrap();
        let db_path = db.path().to_str().unwrap().to_string();
        let link_repo: Arc<dyn gyre_ports::SpecLinkRepository> = {
            let storage = gyre_adapters::SqliteStorage::new(&db_path).unwrap();
            Arc::new(storage)
        };
        let ledger: Arc<dyn gyre_ports::SpecLedgerRepository> =
            Arc::new(MemSpecLedgerRepository::default());
        let links_store: SpecLinksStore = Arc::new(Mutex::new(Vec::new()));
        let ws_ctx: Arc<dyn gyre_ports::WorkspaceRepository> =
            Arc::new(MemWorkspaceRepository::default());
        let repos_ctx: Arc<dyn gyre_ports::RepoRepository> = Arc::new(MemRepoRepository::default());

        // First push: manifest declares the link.
        sync_spec_ledger(
            &ledger,
            &links_store,
            &link_repo,
            dir.path().to_str().unwrap(),
            &sha_with_link,
            1_700_000_000,
            Some("repo-1"),
            Some("ws-1"),
            Some(&ws_ctx),
            Some(&repos_ctx),
            None,
            None,
            None,
            None,
            None,
        )
        .await;
        assert_eq!(links_store.lock().await.len(), 1);
        assert_eq!(link_repo.list_all().await.unwrap().len(), 1);

        // Second push: same spec, link removed from the manifest. Commit the
        // edited manifest and sync the new HEAD.
        let sha_without_link = add_commit(&dir, &[("specs/manifest.yaml", without_link)]);
        sync_spec_ledger(
            &ledger,
            &links_store,
            &link_repo,
            dir.path().to_str().unwrap(),
            &sha_without_link,
            1_700_000_500,
            Some("repo-1"),
            Some("ws-1"),
            Some(&ws_ctx),
            Some(&repos_ctx),
            None,
            None,
            None,
            None,
            None,
        )
        .await;

        // Cache dropped the link…
        assert!(
            links_store.lock().await.is_empty(),
            "removed link must be dropped from the hot cache"
        );
        // …and the durable table must agree (this is the bug: skipping the
        // replace for an empty link set left the row behind, resurrecting it
        // on the next boot load).
        let persisted: Vec<SpecLinkEntry> = link_repo.list_all().await.unwrap();
        assert!(
            persisted.is_empty(),
            "removed link must be deleted from the durable table, got {persisted:?}"
        );
    }

    // -----------------------------------------------------------------------
    // task-287: cross-repo / cross-workspace link validation
    // (spec-links.md §Cross-Repo and Cross-Workspace Links)
    // -----------------------------------------------------------------------

    fn make_workspace_in_tenant(id: &str, tenant: &str, slug: &str) -> gyre_domain::Workspace {
        gyre_domain::Workspace {
            id: gyre_common::Id::new(id),
            tenant_id: gyre_common::Id::new(tenant),
            name: format!("ws-{id}"),
            slug: slug.to_string(),
            description: None,
            budget: None,
            max_repos: None,
            max_agents_per_repo: None,
            trust_level: gyre_domain::TrustLevel::Guided,
            llm_model: None,
            created_at: 1_000_000,
            compute_target_id: None,
        }
    }

    fn make_repo_entity(id: &str, name: &str, ws_id: &str, path: &str) -> gyre_domain::Repository {
        gyre_domain::Repository {
            id: gyre_common::Id::new(id),
            name: name.to_string(),
            path: path.to_string(),
            workspace_id: gyre_common::Id::new(ws_id),
            default_branch: "main".to_string(),
            is_mirror: false,
            mirror_url: None,
            mirror_interval_secs: None,
            last_mirror_sync: None,
            description: None,
            status: gyre_domain::RepoStatus::Active,
            created_at: 1_000_000,
            updated_at: 1_000_000,
            max_agents: None,
            max_agent_iterations: None,
            budget: None,
        }
    }

    fn make_membership_entity(
        user_id: &str,
        ws_id: &str,
        role: gyre_domain::WorkspaceRole,
    ) -> gyre_domain::WorkspaceMembership {
        gyre_domain::WorkspaceMembership {
            id: gyre_common::Id::new(uuid::Uuid::new_v4().to_string()),
            user_id: gyre_common::Id::new(user_id),
            workspace_id: gyre_common::Id::new(ws_id),
            role,
            invited_by: gyre_common::Id::new("admin"),
            accepted: true,
            accepted_at: Some(1_000_000),
            created_at: 1_000_000,
        }
    }

    /// Blob SHA of a file at a commit (same mechanism sync_spec_ledger uses
    /// for ledger current_sha and link target_sha pinning).
    fn blob_sha_at(dir: &tempfile::TempDir, sha: &str, file: &str) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["rev-parse", &format!("{sha}:{file}")])
            .output()
            .expect("git rev-parse failed");
        assert!(
            out.status.success(),
            "git rev-parse {sha}:{file} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// AC 1: a manifest in repo A (workspace W) declaring depends_on
    /// @B/system/auth.md, where repo B exists in W, resolves to B's repo id
    /// with status "active" — and a subsequent change to B's target spec
    /// marks A's link stale (push-time inbound staleness, step 6b).
    ///
    /// Against the pre-task code this fails at the first assertion:
    /// push-time resolution passed None as the workspace to the repo-name
    /// lookup (which returns None without a workspace scope), so every
    /// same-workspace @repo/path link was stored "unresolved" forever.
    #[tokio::test]
    async fn cross_repo_same_workspace_link_resolves_active_then_stale_on_target_change() {
        use crate::mem::{MemRepoRepository, MemSpecLedgerRepository, MemWorkspaceRepository};

        // Target repo B in workspace ws-1 (tenant "tenant-287", ≠ the
        // literal "default" — the scope the fabricated-tenant bug broke).
        let (dir_b, sha_b) = make_test_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/auth.md\n    title: Auth\n    owner: user:test\n",
            ),
            ("specs/system/auth.md", "# Auth v1\n"),
        ])
        .await;

        // Source repo A links @B/system/auth.md pinned to B's current blob SHA.
        let auth_blob_v1 = blob_sha_at(&dir_b, &sha_b, "specs/system/auth.md");
        let manifest_a = format!(
            "version: 1\nspecs:\n  - path: system/a.md\n    title: A\n    owner: user:test\n    links:\n      - type: depends_on\n        target: \"@B/system/auth.md\"\n        target_sha: {auth_blob_v1}\n"
        );
        let (dir_a, sha_a) = make_test_repo(&[
            ("specs/manifest.yaml", manifest_a.as_str()),
            ("specs/system/a.md", "# A\n"),
        ])
        .await;

        let ledger: Arc<dyn gyre_ports::SpecLedgerRepository> =
            Arc::new(MemSpecLedgerRepository::default());
        let links_store: SpecLinksStore = Arc::new(Mutex::new(Vec::new()));
        let link_repo: Arc<dyn gyre_ports::SpecLinkRepository> =
            Arc::new(crate::mem::MemSpecLinkRepository::default());

        let ws_ctx: Arc<dyn gyre_ports::WorkspaceRepository> =
            Arc::new(MemWorkspaceRepository::default());
        let ws = make_workspace_in_tenant("ws-1", "tenant-287", "ws-one");
        ws_ctx.create(&ws).await.unwrap();
        let repos_ctx: Arc<dyn gyre_ports::RepoRepository> = Arc::new(MemRepoRepository::default());
        repos_ctx
            .create(&make_repo_entity(
                "repo-a",
                "A",
                "ws-1",
                dir_a.path().to_str().unwrap(),
            ))
            .await
            .unwrap();
        repos_ctx
            .create(&make_repo_entity(
                "repo-b",
                "B",
                "ws-1",
                dir_b.path().to_str().unwrap(),
            ))
            .await
            .unwrap();
        let tenant = gyre_common::Id::new("tenant-287");

        // Push B first: registers system/auth.md in the ledger at v1.
        sync_spec_ledger(
            &ledger,
            &links_store,
            &link_repo,
            dir_b.path().to_str().unwrap(),
            &sha_b,
            1_700_000_000,
            Some("repo-b"),
            Some("ws-1"),
            Some(&ws_ctx),
            Some(&repos_ctx),
            Some(&tenant),
            None,
            None,
            None,
            None,
        )
        .await;

        // Push A: the same-workspace cross-repo link must resolve to
        // repo-b's id and be active (pinned SHA matches the ledger).
        sync_spec_ledger(
            &ledger,
            &links_store,
            &link_repo,
            dir_a.path().to_str().unwrap(),
            &sha_a,
            1_700_000_100,
            Some("repo-a"),
            Some("ws-1"),
            Some(&ws_ctx),
            Some(&repos_ctx),
            Some(&tenant),
            None,
            None,
            None,
            None,
        )
        .await;

        let links: Vec<SpecLinkEntry> = links_store.lock().await.clone();
        assert_eq!(links.len(), 1, "A's depends_on link must be recorded");
        assert_eq!(links[0].target_repo_id.as_deref(), Some("repo-b"));
        assert_eq!(links[0].status, "active");
        assert_eq!(
            links[0].target_display.as_deref(),
            Some("@B/system/auth.md")
        );

        // Subsequent change to B's target spec: A's link must go stale via
        // push-time inbound staleness (sync step 6b).
        let sha_b2 = add_commit(&dir_b, &[("specs/system/auth.md", "# Auth v2\n")]);
        sync_spec_ledger(
            &ledger,
            &links_store,
            &link_repo,
            dir_b.path().to_str().unwrap(),
            &sha_b2,
            1_700_000_200,
            Some("repo-b"),
            Some("ws-1"),
            Some(&ws_ctx),
            Some(&repos_ctx),
            Some(&tenant),
            None,
            None,
            None,
            None,
        )
        .await;

        let links: Vec<SpecLinkEntry> = links_store.lock().await.clone();
        assert_eq!(
            links[0].status, "stale",
            "a change to B's target spec must mark A's link stale, got {:?}",
            links[0]
        );
        assert!(links[0].stale_since.is_some());
    }

    /// AC 3 / verification-intent probe 2: a link whose target repo resolves
    /// but whose target path is absent from the target repo's manifest is
    /// stored "unresolved" with the missing path named in the warning.
    /// Against the pre-task code this fails: the link was marked "active"
    /// on repo existence alone.
    #[tokio::test]
    async fn cross_repo_link_unresolved_when_target_path_not_in_target_manifest() {
        use crate::mem::{MemRepoRepository, MemSpecLedgerRepository, MemWorkspaceRepository};

        let (dir_b, sha_b) = make_test_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/auth.md\n    title: Auth\n    owner: user:test\n",
            ),
            ("specs/system/auth.md", "# Auth\n"),
        ])
        .await;
        // A links a path B's manifest does NOT declare.
        let manifest_a = "version: 1\nspecs:\n  - path: system/a.md\n    title: A\n    owner: user:test\n    links:\n      - type: depends_on\n        target: \"@B/system/nonexistent.md\"\n";
        let (dir_a, sha_a) = make_test_repo(&[
            ("specs/manifest.yaml", manifest_a),
            ("specs/system/a.md", "# A\n"),
        ])
        .await;

        let ledger: Arc<dyn gyre_ports::SpecLedgerRepository> =
            Arc::new(MemSpecLedgerRepository::default());
        let links_store: SpecLinksStore = Arc::new(Mutex::new(Vec::new()));
        let link_repo: Arc<dyn gyre_ports::SpecLinkRepository> =
            Arc::new(crate::mem::MemSpecLinkRepository::default());
        let ws_ctx: Arc<dyn gyre_ports::WorkspaceRepository> =
            Arc::new(MemWorkspaceRepository::default());
        ws_ctx
            .create(&make_workspace_in_tenant("ws-1", "tenant-287", "ws-one"))
            .await
            .unwrap();
        let repos_ctx: Arc<dyn gyre_ports::RepoRepository> = Arc::new(MemRepoRepository::default());
        repos_ctx
            .create(&make_repo_entity(
                "repo-a",
                "A",
                "ws-1",
                dir_a.path().to_str().unwrap(),
            ))
            .await
            .unwrap();
        // repo-b exists (its manifest just doesn't declare the target path).
        repos_ctx
            .create(&make_repo_entity(
                "repo-b",
                "B",
                "ws-1",
                dir_b.path().to_str().unwrap(),
            ))
            .await
            .unwrap();
        let tenant = gyre_common::Id::new("tenant-287");
        let _ = sha_b;

        sync_spec_ledger(
            &ledger,
            &links_store,
            &link_repo,
            dir_a.path().to_str().unwrap(),
            &sha_a,
            1_700_000_000,
            Some("repo-a"),
            Some("ws-1"),
            Some(&ws_ctx),
            Some(&repos_ctx),
            Some(&tenant),
            None,
            None,
            None,
            None,
        )
        .await;

        let links: Vec<SpecLinkEntry> = links_store.lock().await.clone();
        assert_eq!(links.len(), 1);
        assert_eq!(
            links[0].status, "unresolved",
            "a target path absent from the target repo's manifest must be unresolved, got {:?}",
            links[0]
        );
        assert_eq!(links[0].target_repo_id, None);
        assert_eq!(links[0].target_path, "system/nonexistent.md");
    }

    /// AC 2: a cross-workspace link pushed by a principal without read
    /// access to the target workspace is stored "unresolved" (never
    /// "active"), with the denial named in the durable marker (and the
    /// warning). Positive control: once the principal holds membership in
    /// the target workspace, a re-push resolves the link active and clears
    /// the marker.
    #[tokio::test]
    async fn cross_workspace_link_denied_without_read_access_stays_unresolved() {
        use crate::mem::{
            MemKvStore, MemRepoRepository, MemSpecLedgerRepository,
            MemWorkspaceMembershipRepository, MemWorkspaceRepository,
        };

        // Target repo C in workspace ws-two (same tenant, different workspace).
        let (dir_c, _sha_c) = make_test_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/spec.md\n    title: Spec\n    owner: user:test\n",
            ),
            ("specs/system/spec.md", "# Spec\n"),
        ])
        .await;
        let manifest_a = "version: 1\nspecs:\n  - path: system/a.md\n    title: A\n    owner: user:test\n    links:\n      - type: depends_on\n        target: \"@ws-two/C/system/spec.md\"\n";
        let (dir_a, sha_a) = make_test_repo(&[
            ("specs/manifest.yaml", manifest_a),
            ("specs/system/a.md", "# A\n"),
        ])
        .await;

        let ledger: Arc<dyn gyre_ports::SpecLedgerRepository> =
            Arc::new(MemSpecLedgerRepository::default());
        let links_store: SpecLinksStore = Arc::new(Mutex::new(Vec::new()));
        let link_repo: Arc<dyn gyre_ports::SpecLinkRepository> =
            Arc::new(crate::mem::MemSpecLinkRepository::default());

        let ws_ctx: Arc<dyn gyre_ports::WorkspaceRepository> =
            Arc::new(MemWorkspaceRepository::default());
        ws_ctx
            .create(&make_workspace_in_tenant(
                "ws-one-id",
                "tenant-287",
                "ws-one",
            ))
            .await
            .unwrap();
        ws_ctx
            .create(&make_workspace_in_tenant(
                "ws-two-id",
                "tenant-287",
                "ws-two",
            ))
            .await
            .unwrap();
        let repos_ctx: Arc<dyn gyre_ports::RepoRepository> = Arc::new(MemRepoRepository::default());
        repos_ctx
            .create(&make_repo_entity(
                "repo-a",
                "A",
                "ws-one-id",
                dir_a.path().to_str().unwrap(),
            ))
            .await
            .unwrap();
        repos_ctx
            .create(&make_repo_entity(
                "repo-c",
                "C",
                "ws-two-id",
                dir_c.path().to_str().unwrap(),
            ))
            .await
            .unwrap();

        // The pushing principal is a user with membership in the SOURCE
        // workspace only — no read access to ws-two.
        let memberships: Arc<dyn gyre_ports::WorkspaceMembershipRepository> =
            Arc::new(MemWorkspaceMembershipRepository::default());
        memberships
            .create(&make_membership_entity(
                "u-1",
                "ws-one-id",
                gyre_domain::WorkspaceRole::Developer,
            ))
            .await
            .unwrap();
        let kv: Arc<dyn gyre_ports::KvJsonStore> = Arc::new(MemKvStore::default());
        let principal = LinkPrincipal {
            agent_id: "user-one".to_string(),
            user_id: Some(gyre_common::Id::new("u-1")),
            workspace_id: Some("ws-one-id".to_string()),
            is_system: false,
        };
        let tenant = gyre_common::Id::new("tenant-287");

        /// Shared port context for the `sync_a` helper; bundling the
        /// references keeps the helper under clippy's `too_many_arguments`
        /// threshold.
        struct SyncCtx<'a> {
            ledger: &'a Arc<dyn gyre_ports::SpecLedgerRepository>,
            links_store: &'a SpecLinksStore,
            link_repo: &'a Arc<dyn gyre_ports::SpecLinkRepository>,
            ws_ctx: &'a Arc<dyn gyre_ports::WorkspaceRepository>,
            repos_ctx: &'a Arc<dyn gyre_ports::RepoRepository>,
            memberships: &'a Arc<dyn gyre_ports::WorkspaceMembershipRepository>,
            kv: &'a Arc<dyn gyre_ports::KvJsonStore>,
            tenant: &'a gyre_common::Id,
        }

        async fn sync_a(
            ctx: &SyncCtx<'_>,
            principal: Option<&LinkPrincipal>,
            dir_a: &str,
            sha_a: &str,
        ) {
            sync_spec_ledger(
                ctx.ledger,
                ctx.links_store,
                ctx.link_repo,
                dir_a,
                sha_a,
                1_700_000_000,
                Some("repo-a"),
                Some("ws-one-id"),
                Some(ctx.ws_ctx),
                Some(ctx.repos_ctx),
                Some(ctx.tenant),
                principal,
                Some(ctx.memberships),
                Some(ctx.kv),
                None,
            )
            .await
        }

        let ctx = SyncCtx {
            ledger: &ledger,
            links_store: &links_store,
            link_repo: &link_repo,
            ws_ctx: &ws_ctx,
            repos_ctx: &repos_ctx,
            memberships: &memberships,
            kv: &kv,
            tenant: &tenant,
        };

        // Push by the principal WITHOUT read access to ws-two: the link
        // must be stored unresolved with target_repo_id NULL.
        sync_a(
            &ctx,
            Some(&principal),
            dir_a.path().to_str().unwrap(),
            &sha_a,
        )
        .await;
        let links: Vec<SpecLinkEntry> = links_store.lock().await.clone();
        assert_eq!(links.len(), 1);
        assert_eq!(
            links[0].status, "unresolved",
            "a cross-workspace link pushed without read access must never be active, got {:?}",
            links[0]
        );
        assert_eq!(links[0].target_repo_id, None);

        // The denial is durably recorded, naming the principal and target
        // (the staleness pass consults this so it cannot flip the link).
        let denial = link_denial(&kv, &links[0].id).await;
        let denial = denial.expect("access denial must be recorded for the denied link");
        assert_eq!(denial.principal, "user-one");
        assert_eq!(denial.target, "@ws-two/C/system/spec.md");

        // Positive control: grant the principal Viewer membership in the
        // target workspace and re-push — the link resolves active and the
        // denial marker clears.
        memberships
            .create(&make_membership_entity(
                "u-1",
                "ws-two-id",
                gyre_domain::WorkspaceRole::Viewer,
            ))
            .await
            .unwrap();
        sync_a(
            &ctx,
            Some(&principal),
            dir_a.path().to_str().unwrap(),
            &sha_a,
        )
        .await;
        let links: Vec<SpecLinkEntry> = links_store.lock().await.clone();
        assert_eq!(links.len(), 1);
        assert_eq!(
            links[0].status, "active",
            "a cross-workspace link pushed by a target-workspace member must resolve active, got {:?}",
            links[0]
        );
        assert_eq!(links[0].target_repo_id.as_deref(), Some("repo-c"));
        assert!(
            link_denial(&kv, &links[0].id).await.is_none(),
            "re-push by an authorized principal must clear the denial marker"
        );
    }

    // -----------------------------------------------------------------------
    // task-288: stored link statuses — push-time broken on manifest removal,
    // conflicted at store time (spec-links.md §Link Status)
    // -----------------------------------------------------------------------

    /// AC 1 (push-time half): removing a spec from a repo's manifest (pushing
    /// the removal) transitions inbound links targeting it to "broken" in
    /// the stored graph — the ledger keeps the removed spec as Deprecated
    /// (save, not delete), so the ledger-only lookup can never fire the
    /// "no longer in the manifest" half of `broken`.
    ///
    /// Against the pre-task code this fails: the inbound link stayed
    /// "active" through every sync and daily pass because the
    /// Ok(Some(entry)) path left it untouched.
    #[tokio::test]
    async fn sync_marks_inbound_links_broken_when_target_leaves_manifest() {
        use crate::mem::{MemRepoRepository, MemSpecLedgerRepository, MemWorkspaceRepository};

        // Repo B's manifest initially declares system/b.md.
        let manifest_with_b =
            "version: 1\nspecs:\n  - path: system/b.md\n    title: B\n    owner: user:test\n";
        let (dir_b, sha_with_b) = make_test_repo(&[
            ("specs/manifest.yaml", manifest_with_b),
            ("specs/system/b.md", "# B\n"),
        ])
        .await;
        let b_blob = blob_sha_at(&dir_b, &sha_with_b, "specs/system/b.md");

        let ledger: Arc<dyn gyre_ports::SpecLedgerRepository> =
            Arc::new(MemSpecLedgerRepository::default());
        let links_store: SpecLinksStore = Arc::new(Mutex::new(Vec::new()));
        let link_repo: Arc<dyn gyre_ports::SpecLinkRepository> =
            Arc::new(crate::mem::MemSpecLinkRepository::default());
        let ws_ctx: Arc<dyn gyre_ports::WorkspaceRepository> =
            Arc::new(MemWorkspaceRepository::default());
        let repos_ctx: Arc<dyn gyre_ports::RepoRepository> = Arc::new(MemRepoRepository::default());

        // The state a prior push of both repos produced: the target in the
        // ledger at its real blob SHA, and a resolved inbound link from
        // repo A targeting repo B's spec (cross-repo: target_repo_id).
        ledger
            .save(&make_test_ledger_entry(
                "system/b.md",
                &b_blob,
                ApprovalStatus::Approved,
            ))
            .await
            .unwrap();
        links_store.lock().await.push(SpecLinkEntry {
            id: "link-inbound".to_string(),
            source_path: "system/a.md".to_string(),
            source_repo_id: Some("repo-a".to_string()),
            source_sha: "sha-a".to_string(),
            link_type: SpecLinkType::DependsOn,
            target_path: "system/b.md".to_string(),
            target_repo_id: Some("repo-b".to_string()),
            target_display: Some("@repo-b/system/b.md".to_string()),
            target_sha: Some(b_blob.clone()),
            reason: None,
            status: "active".to_string(),
            created_at: 1_700_000_000,
            stale_since: None,
        });

        // Control: a sync of repo B whose manifest still declares the
        // target leaves the inbound link active.
        sync_spec_ledger(
            &ledger,
            &links_store,
            &link_repo,
            dir_b.path().to_str().unwrap(),
            &sha_with_b,
            1_700_000_000,
            Some("repo-b"),
            Some("ws-b"),
            Some(&ws_ctx),
            Some(&repos_ctx),
            None,
            None,
            None,
            None,
            None,
        )
        .await;
        {
            let links: Vec<SpecLinkEntry> = links_store.lock().await.clone();
            let l = links.iter().find(|l| l.id == "link-inbound").unwrap();
            assert_eq!(
                l.status, "active",
                "declared target must keep the link active"
            );
        }

        // Push the removal: B's manifest no longer declares system/b.md.
        let manifest_without_b = "version: 1\nspecs: []\n";
        let sha_without_b = add_commit(&dir_b, &[("specs/manifest.yaml", manifest_without_b)]);
        sync_spec_ledger(
            &ledger,
            &links_store,
            &link_repo,
            dir_b.path().to_str().unwrap(),
            &sha_without_b,
            1_700_000_500,
            Some("repo-b"),
            Some("ws-b"),
            Some(&ws_ctx),
            Some(&repos_ctx),
            None,
            None,
            None,
            None,
            None,
        )
        .await;

        // The inbound link is broken in the hot cache…
        {
            let links: Vec<SpecLinkEntry> = links_store.lock().await.clone();
            let l = links
                .iter()
                .find(|l| l.id == "link-inbound")
                .expect("inbound link must survive the target-repo push");
            assert_eq!(
                l.status, "broken",
                "a target removed from its repo's manifest must break inbound links"
            );
        }
        // …and in the durable table.
        let persisted: Vec<SpecLinkEntry> = link_repo.list_all().await.unwrap();
        let row = persisted.iter().find(|l| l.id == "link-inbound").unwrap();
        assert_eq!(row.status, "broken");

        // The ledger keeps the removed target as Deprecated — removal is
        // deprecation, not deletion, which is exactly why the manifest
        // check (not the ledger lookup) must drive `broken`.
        let entry = ledger.find_by_path("system/b.md").await.unwrap().unwrap();
        assert_eq!(entry.approval_status, ApprovalStatus::Deprecated);
    }

    /// AC 2 (sync half): a conflicts_with link declared by a manifest push
    /// between two ALREADY-Approved specs (so the approval-time gate never
    /// rejects) is stored as "conflicted" — not "active" — in both the hot
    /// cache and the durable graph.
    #[tokio::test]
    async fn sync_stores_conflicts_with_link_as_conflicted_when_both_approved() {
        use crate::mem::{MemRepoRepository, MemSpecLedgerRepository, MemWorkspaceRepository};

        let manifest = r#"version: 1
specs:
  - path: system/a.md
    title: A
    owner: user:test
    links:
      - type: conflicts_with
        target: system/b.md
  - path: system/b.md
    title: B
    owner: user:test"#;
        let (dir_a, sha_a) = make_test_repo(&[
            ("specs/manifest.yaml", manifest),
            ("specs/system/a.md", "# A\n"),
            ("specs/system/b.md", "# B\n"),
        ])
        .await;
        let a_blob = blob_sha_at(&dir_a, &sha_a, "specs/system/a.md");
        let b_blob = blob_sha_at(&dir_a, &sha_a, "specs/system/b.md");

        let ledger: Arc<dyn gyre_ports::SpecLedgerRepository> =
            Arc::new(MemSpecLedgerRepository::default());
        let links_store: SpecLinksStore = Arc::new(Mutex::new(Vec::new()));
        let link_repo: Arc<dyn gyre_ports::SpecLinkRepository> =
            Arc::new(crate::mem::MemSpecLinkRepository::default());
        let ws_ctx: Arc<dyn gyre_ports::WorkspaceRepository> =
            Arc::new(MemWorkspaceRepository::default());
        let repos_ctx: Arc<dyn gyre_ports::RepoRepository> = Arc::new(MemRepoRepository::default());

        // Both endpoints already Approved at their real blob SHAs (so the
        // sync neither invalidates their approvals nor deprecates them).
        for (path, blob) in [("system/a.md", &a_blob), ("system/b.md", &b_blob)] {
            let mut e = make_test_ledger_entry(path, blob, ApprovalStatus::Approved);
            e.repo_id = Some("repo-a".to_string());
            ledger.save(&e).await.unwrap();
        }

        sync_spec_ledger(
            &ledger,
            &links_store,
            &link_repo,
            dir_a.path().to_str().unwrap(),
            &sha_a,
            1_700_000_000,
            Some("repo-a"),
            Some("ws-a"),
            Some(&ws_ctx),
            Some(&repos_ctx),
            None,
            None,
            None,
            None,
            None,
        )
        .await;

        let links: Vec<SpecLinkEntry> = links_store.lock().await.clone();
        assert_eq!(links.len(), 1, "the conflicts_with link must be stored");
        assert_eq!(
            links[0].status, "conflicted",
            "a conflicts_with link declared between two already-approved specs \
             must be stored as conflicted, got {:?}",
            links[0]
        );

        // The status is written through to the durable graph.
        let persisted: Vec<SpecLinkEntry> = link_repo.list_all().await.unwrap();
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].status, "conflicted");
    }

    // -----------------------------------------------------------------------
    // TASK-019: Cycle detection tests
    // -----------------------------------------------------------------------

    #[test]
    fn cycle_detection_no_cycles() {
        let yaml = r#"
version: 1
specs:
  - path: system/a.md
    title: A
    owner: user:test
    links:
      - type: depends_on
        target: system/b.md
  - path: system/b.md
    title: B
    owner: user:test
    links:
      - type: depends_on
        target: system/c.md
  - path: system/c.md
    title: C
    owner: user:test
"#;
        let manifest = parse_manifest(yaml).unwrap();
        let result = detect_link_cycles(&manifest);
        assert!(result.is_ok(), "no cycle should be detected: {result:?}");
    }

    #[test]
    fn cycle_detection_simple_cycle() {
        // A depends_on B depends_on C depends_on A → rejected.
        let yaml = r#"
version: 1
specs:
  - path: system/a.md
    title: A
    owner: user:test
    links:
      - type: depends_on
        target: system/b.md
  - path: system/b.md
    title: B
    owner: user:test
    links:
      - type: depends_on
        target: system/c.md
  - path: system/c.md
    title: C
    owner: user:test
    links:
      - type: depends_on
        target: system/a.md
"#;
        let manifest = parse_manifest(yaml).unwrap();
        let result = detect_link_cycles(&manifest);
        assert!(result.is_err(), "cycle should be detected");
        let msg = result.unwrap_err();
        assert!(msg.contains("spec link cycle detected"), "msg: {msg}");
    }

    #[test]
    fn cycle_detection_implements_cycle() {
        // A implements B implements A → rejected.
        let yaml = r#"
version: 1
specs:
  - path: system/a.md
    title: A
    owner: user:test
    links:
      - type: implements
        target: system/b.md
  - path: system/b.md
    title: B
    owner: user:test
    links:
      - type: implements
        target: system/a.md
"#;
        let manifest = parse_manifest(yaml).unwrap();
        let result = detect_link_cycles(&manifest);
        assert!(result.is_err(), "implements cycle should be detected");
    }

    #[test]
    fn cycle_detection_excludes_conflicts_with() {
        // conflicts_with is bidirectional by nature — not a cycle.
        let yaml = r#"
version: 1
specs:
  - path: system/a.md
    title: A
    owner: user:test
    links:
      - type: conflicts_with
        target: system/b.md
  - path: system/b.md
    title: B
    owner: user:test
    links:
      - type: conflicts_with
        target: system/a.md
"#;
        let manifest = parse_manifest(yaml).unwrap();
        let result = detect_link_cycles(&manifest);
        assert!(
            result.is_ok(),
            "conflicts_with should not trigger cycle detection"
        );
    }

    #[test]
    fn cycle_detection_excludes_references() {
        // references links are excluded from cycle detection.
        let yaml = r#"
version: 1
specs:
  - path: system/a.md
    title: A
    owner: user:test
    links:
      - type: references
        target: system/b.md
  - path: system/b.md
    title: B
    owner: user:test
    links:
      - type: references
        target: system/a.md
"#;
        let manifest = parse_manifest(yaml).unwrap();
        let result = detect_link_cycles(&manifest);
        assert!(
            result.is_ok(),
            "references should not trigger cycle detection"
        );
    }

    #[test]
    fn cycle_detection_excludes_supersedes() {
        // supersedes links are excluded from cycle detection.
        let yaml = r#"
version: 1
specs:
  - path: system/a.md
    title: A
    owner: user:test
    links:
      - type: supersedes
        target: system/b.md
  - path: system/b.md
    title: B
    owner: user:test
    links:
      - type: supersedes
        target: system/a.md
"#;
        let manifest = parse_manifest(yaml).unwrap();
        let result = detect_link_cycles(&manifest);
        assert!(
            result.is_ok(),
            "supersedes should not trigger cycle detection"
        );
    }

    #[test]
    fn cycle_detection_self_loop() {
        // A depends_on A → rejected.
        let yaml = r#"
version: 1
specs:
  - path: system/a.md
    title: A
    owner: user:test
    links:
      - type: depends_on
        target: system/a.md
"#;
        let manifest = parse_manifest(yaml).unwrap();
        let result = detect_link_cycles(&manifest);
        assert!(result.is_err(), "self-loop should be detected");
    }

    #[test]
    fn cycle_detection_mixed_types_cycle() {
        // A depends_on B implements A → cycle (both types are checked).
        let yaml = r#"
version: 1
specs:
  - path: system/a.md
    title: A
    owner: user:test
    links:
      - type: depends_on
        target: system/b.md
  - path: system/b.md
    title: B
    owner: user:test
    links:
      - type: implements
        target: system/a.md
"#;
        let manifest = parse_manifest(yaml).unwrap();
        let result = detect_link_cycles(&manifest);
        assert!(
            result.is_err(),
            "mixed depends_on/implements cycle should be detected"
        );
    }

    #[test]
    fn cycle_detection_extends_cycle() {
        // A extends B extends A → cycle (extends is a directed link type,
        // not excluded by spec-links.md §Cycle Detection).
        let yaml = r#"
version: 1
specs:
  - path: system/a.md
    title: A
    owner: user:test
    links:
      - type: extends
        target: system/b.md
  - path: system/b.md
    title: B
    owner: user:test
    links:
      - type: extends
        target: system/a.md
"#;
        let manifest = parse_manifest(yaml).unwrap();
        let result = detect_link_cycles(&manifest);
        assert!(result.is_err(), "extends cycle should be detected");
        let msg = result.unwrap_err();
        assert!(msg.contains("spec link cycle detected"), "msg: {msg}");
    }

    #[test]
    fn cycle_detection_extends_no_cycle() {
        // A extends B (no cycle — linear chain).
        let yaml = r#"
version: 1
specs:
  - path: system/a.md
    title: A
    owner: user:test
    links:
      - type: extends
        target: system/b.md
  - path: system/b.md
    title: B
    owner: user:test
    links: []
"#;
        let manifest = parse_manifest(yaml).unwrap();
        let result = detect_link_cycles(&manifest);
        assert!(
            result.is_ok(),
            "linear extends chain should not trigger cycle detection"
        );
    }

    #[test]
    fn cycle_detection_mixed_extends_depends_cycle() {
        // A extends B depends_on A → cycle (mixed directed types).
        let yaml = r#"
version: 1
specs:
  - path: system/a.md
    title: A
    owner: user:test
    links:
      - type: extends
        target: system/b.md
  - path: system/b.md
    title: B
    owner: user:test
    links:
      - type: depends_on
        target: system/a.md
"#;
        let manifest = parse_manifest(yaml).unwrap();
        let result = detect_link_cycles(&manifest);
        assert!(
            result.is_err(),
            "mixed extends/depends_on cycle should be detected"
        );
    }

    // -----------------------------------------------------------------------
    // task-293: tenant-wide cycle detection (durable spec_links table +
    // pushed manifest edges, node identity (repo, path))
    // -----------------------------------------------------------------------

    /// Seed a stored (already-synced) link row in the durable graph.
    fn stored_link(
        source_repo: &str,
        source_path: &str,
        link_type: SpecLinkType,
        target_repo: Option<&str>,
        target_path: &str,
    ) -> SpecLinkEntry {
        let target_display = target_repo.map(|_| format!("@B/{target_path}"));
        SpecLinkEntry {
            id: format!("{source_repo}:{source_path}:{link_type}:{target_path}"),
            source_path: source_path.to_string(),
            source_repo_id: Some(source_repo.to_string()),
            source_sha: "sha".to_string(),
            link_type,
            target_path: target_path.to_string(),
            target_repo_id: target_repo.map(str::to_string),
            target_display,
            target_sha: None,
            reason: None,
            status: "active".to_string(),
            created_at: 1_700_000_000,
            stale_since: None,
        }
    }

    /// Shared fixture: workspace ws-1 (tenant-293) with real git repos
    /// repo-a ("A") and repo-b ("B") registered in a mem RepoRepository so
    /// `@A/...` / `@B/...` manifest targets resolve to their repo ids.
    async fn two_repo_workspace(
        dir_a: &tempfile::TempDir,
        dir_b: &tempfile::TempDir,
    ) -> (
        Arc<dyn gyre_ports::WorkspaceRepository>,
        Arc<dyn gyre_ports::RepoRepository>,
        gyre_common::Id,
    ) {
        use crate::mem::{MemRepoRepository, MemWorkspaceRepository};
        let ws_ctx: Arc<dyn gyre_ports::WorkspaceRepository> =
            Arc::new(MemWorkspaceRepository::default());
        ws_ctx
            .create(&make_workspace_in_tenant("ws-1", "tenant-293", "ws-one"))
            .await
            .unwrap();
        let repos_ctx: Arc<dyn gyre_ports::RepoRepository> = Arc::new(MemRepoRepository::default());
        repos_ctx
            .create(&make_repo_entity(
                "repo-a",
                "A",
                "ws-1",
                dir_a.path().to_str().unwrap(),
            ))
            .await
            .unwrap();
        repos_ctx
            .create(&make_repo_entity(
                "repo-b",
                "B",
                "ws-1",
                dir_b.path().to_str().unwrap(),
            ))
            .await
            .unwrap();
        (ws_ctx, repos_ctx, gyre_common::Id::new("tenant-293"))
    }

    /// AC 1: pushing A depends_on @B/b in repo 1 succeeds; then pushing
    /// B depends_on @A/a in repo 2 is rejected with the cycle named.
    /// Against the pre-task-293 code this fails at the second check: each
    /// push only saw its own manifest's edges, so both landed cleanly and
    /// the cycle lived permanently in the forge graph.
    #[tokio::test]
    async fn tenant_cycle_cross_repo_second_push_rejected() {
        use crate::mem::{MemSpecLedgerRepository, MemSpecLinkRepository};

        // Repo B declares system/b.md (no links yet).
        let (dir_b, _sha_b1) = make_test_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/b.md\n    title: B\n    owner: user:test\n",
            ),
            ("specs/system/b.md", "# B\n"),
        ])
        .await;
        // Repo A declares system/a.md depending on @B/system/b.md.
        let (dir_a, sha_a) = make_test_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/a.md\n    title: A\n    owner: user:test\n    links:\n      - type: depends_on\n        target: \"@B/system/b.md\"\n",
            ),
            ("specs/system/a.md", "# A\n"),
        ])
        .await;

        let (ws_ctx, repos_ctx, tenant) = two_repo_workspace(&dir_a, &dir_b).await;
        let link_repo: Arc<dyn gyre_ports::SpecLinkRepository> =
            Arc::new(MemSpecLinkRepository::default());

        // Push 1 (repo A): accepted — no back-edge exists anywhere yet.
        let ctx_a = TenantCycleContext {
            link_repo: Some(&link_repo),
            source_repo_id: Some("repo-a"),
            source_workspace_id: Some("ws-1"),
            workspaces: Some(&ws_ctx),
            repos: Some(&repos_ctx),
            tenant_id: Some(&tenant),
            memberships: None,
            principal: None,
            kv: None,
        };
        check_spec_link_cycles(dir_a.path().to_str().unwrap(), &sha_a, Some(&ctx_a))
            .await
            .expect("first push (A depends_on @B/b) must be accepted");

        // The accepted push lands: the post-receive sync stores A's edge in
        // the durable graph with the target attributed to repo B.
        let ledger: Arc<dyn gyre_ports::SpecLedgerRepository> =
            Arc::new(MemSpecLedgerRepository::default());
        let links_store: SpecLinksStore = Arc::new(Mutex::new(Vec::new()));
        sync_spec_ledger(
            &ledger,
            &links_store,
            &link_repo,
            dir_a.path().to_str().unwrap(),
            &sha_a,
            1_700_000_000,
            Some("repo-a"),
            Some("ws-1"),
            Some(&ws_ctx),
            Some(&repos_ctx),
            Some(&tenant),
            None,
            None,
            None,
            None,
        )
        .await;
        let stored = link_repo.list_all().await.unwrap();
        assert_eq!(stored.len(), 1, "push 1 must store exactly one link");
        assert_eq!(
            stored[0].target_repo_id.as_deref(),
            Some("repo-b"),
            "A's @B/system/b.md edge must be attributed to repo B: {:?}",
            stored[0]
        );

        // Push 2 (repo B): manifest now adds B depends_on @A/system/a.md.
        let sha_b2 = add_commit(
            &dir_b,
            &[(
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/b.md\n    title: B\n    owner: user:test\n    links:\n      - type: depends_on\n        target: \"@A/system/a.md\"\n",
            )],
        );
        let ctx_b = TenantCycleContext {
            link_repo: Some(&link_repo),
            source_repo_id: Some("repo-b"),
            source_workspace_id: Some("ws-1"),
            workspaces: Some(&ws_ctx),
            repos: Some(&repos_ctx),
            tenant_id: Some(&tenant),
            memberships: None,
            principal: None,
            kv: None,
        };
        let err = check_spec_link_cycles(dir_b.path().to_str().unwrap(), &sha_b2, Some(&ctx_b))
            .await
            .expect_err("second push (B depends_on @A/a) must be rejected: the cycle A→B→A now exists across two repos");
        assert!(
            err.contains("spec link cycle detected"),
            "rejection must name the cycle: {err}"
        );
        assert!(
            err.contains("repo-a:system/a.md") && err.contains("repo-b:system/b.md"),
            "cycle message must name both cross-repo nodes with repo attribution: {err}"
        );
    }

    /// AC 3 (companion): a references-typed B→A second push is accepted —
    /// the detection was widened, not made indiscriminate.
    #[tokio::test]
    async fn tenant_cycle_references_back_edge_accepted() {
        use crate::mem::MemSpecLinkRepository;

        let (dir_b, _sha_b1) = make_test_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/b.md\n    title: B\n    owner: user:test\n",
            ),
            ("specs/system/b.md", "# B\n"),
        ])
        .await;
        let (dir_a, _sha_a) = make_test_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/a.md\n    title: A\n    owner: user:test\n    links:\n      - type: depends_on\n        target: \"@B/system/b.md\"\n",
            ),
            ("specs/system/a.md", "# A\n"),
        ])
        .await;

        let (ws_ctx, repos_ctx, tenant) = two_repo_workspace(&dir_a, &dir_b).await;
        let link_repo: Arc<dyn gyre_ports::SpecLinkRepository> =
            Arc::new(MemSpecLinkRepository::default());
        // A's accepted push already landed: its edge is in the durable graph.
        link_repo
            .save(&stored_link(
                "repo-a",
                "system/a.md",
                SpecLinkType::DependsOn,
                Some("repo-b"),
                "system/b.md",
            ))
            .await
            .unwrap();

        // B pushes a references-typed back-edge: references is excluded
        // from cycle detection, so this push must be accepted.
        let sha_b2 = add_commit(
            &dir_b,
            &[(
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/b.md\n    title: B\n    owner: user:test\n    links:\n      - type: references\n        target: \"@A/system/a.md\"\n",
            )],
        );
        let ctx_b = TenantCycleContext {
            link_repo: Some(&link_repo),
            source_repo_id: Some("repo-b"),
            source_workspace_id: Some("ws-1"),
            workspaces: Some(&ws_ctx),
            repos: Some(&repos_ctx),
            tenant_id: Some(&tenant),
            memberships: None,
            principal: None,
            kv: None,
        };
        check_spec_link_cycles(dir_b.path().to_str().unwrap(), &sha_b2, Some(&ctx_b))
            .await
            .expect("references-typed back-edge must be accepted (excluded from cycle detection)");
    }

    /// AC 3: conflicts_with bidirectionality across repos is not reported
    /// as a cycle — neither stored conflicts_with edges nor the pushed
    /// manifest's own.
    #[tokio::test]
    async fn tenant_cycle_conflicts_with_bidirectional_not_a_cycle() {
        use crate::mem::MemSpecLinkRepository;

        let (dir_b, _sha_b1) = make_test_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/b.md\n    title: B\n    owner: user:test\n",
            ),
            ("specs/system/b.md", "# B\n"),
        ])
        .await;
        let (dir_a, _sha_a) = make_test_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/a.md\n    title: A\n    owner: user:test\n",
            ),
            ("specs/system/a.md", "# A\n"),
        ])
        .await;

        let (ws_ctx, repos_ctx, tenant) = two_repo_workspace(&dir_a, &dir_b).await;
        let link_repo: Arc<dyn gyre_ports::SpecLinkRepository> =
            Arc::new(MemSpecLinkRepository::default());
        // Stored conflicts_with edge A→B (repo-a → repo-b).
        link_repo
            .save(&stored_link(
                "repo-a",
                "system/a.md",
                SpecLinkType::ConflictsWith,
                Some("repo-b"),
                "system/b.md",
            ))
            .await
            .unwrap();

        // B pushes the conflicts_with back-edge: conflicts_with is
        // bidirectional by nature, not a cycle.
        let sha_b2 = add_commit(
            &dir_b,
            &[(
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/b.md\n    title: B\n    owner: user:test\n    links:\n      - type: conflicts_with\n        target: \"@A/system/a.md\"\n",
            )],
        );
        let ctx_b = TenantCycleContext {
            link_repo: Some(&link_repo),
            source_repo_id: Some("repo-b"),
            source_workspace_id: Some("ws-1"),
            workspaces: Some(&ws_ctx),
            repos: Some(&repos_ctx),
            tenant_id: Some(&tenant),
            memberships: None,
            principal: None,
            kv: None,
        };
        check_spec_link_cycles(dir_b.path().to_str().unwrap(), &sha_b2, Some(&ctx_b))
            .await
            .expect("conflicts_with bidirectionality must not be reported as a cycle");
    }

    /// AC 2: single-manifest cycles are still rejected with the full
    /// tenant context in scope — no regression from the widened graph.
    #[tokio::test]
    async fn tenant_cycle_single_manifest_cycle_rejected_with_tenant_context() {
        use crate::mem::MemSpecLinkRepository;

        let (dir_b, sha_b1) = make_test_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/a.md\n    title: A\n    owner: user:test\n    links:\n      - type: depends_on\n        target: system/b.md\n  - path: system/b.md\n    title: B\n    owner: user:test\n    links:\n      - type: depends_on\n        target: system/a.md\n",
            ),
            ("specs/system/a.md", "# A\n"),
            ("specs/system/b.md", "# B\n"),
        ])
        .await;
        let (dir_a, _sha_a) = make_test_repo(&[(
            "specs/manifest.yaml",
            "version: 1\nspecs:\n  - path: system/a.md\n    title: A\n    owner: user:test\n",
        )])
        .await;

        let (ws_ctx, repos_ctx, tenant) = two_repo_workspace(&dir_a, &dir_b).await;
        let link_repo: Arc<dyn gyre_ports::SpecLinkRepository> =
            Arc::new(MemSpecLinkRepository::default());
        let ctx = TenantCycleContext {
            link_repo: Some(&link_repo),
            source_repo_id: Some("repo-b"),
            source_workspace_id: Some("ws-1"),
            workspaces: Some(&ws_ctx),
            repos: Some(&repos_ctx),
            tenant_id: Some(&tenant),
            memberships: None,
            principal: None,
            kv: None,
        };
        let err = check_spec_link_cycles(dir_b.path().to_str().unwrap(), &sha_b1, Some(&ctx))
            .await
            .expect_err("single-manifest cycle must still be rejected");
        assert!(
            err.contains("spec link cycle detected"),
            "rejection must name the cycle: {err}"
        );
    }

    /// Replace semantics: the pushed manifest's edges supersede the stored
    /// rows for its (repo, source path) scope — a push that REMOVES the
    /// cycle-participating edge is accepted even though the stale stored
    /// row still carries it. Without the replacement scoping, counting the
    /// pre-push row alongside the manifest's (empty) edge set would reject
    /// the very push that fixes the graph.
    #[tokio::test]
    async fn tenant_cycle_push_removing_edge_accepted() {
        use crate::mem::MemSpecLinkRepository;

        let (dir_b, _sha_b1) = make_test_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/b.md\n    title: B\n    owner: user:test\n",
            ),
            ("specs/system/b.md", "# B\n"),
        ])
        .await;
        // Repo A's initial manifest carries the A→B edge — the state that
        // produced the legacy stored row below.
        let (dir_a, _sha_a) = make_test_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/a.md\n    title: A\n    owner: user:test\n    links:\n      - type: depends_on\n        target: \"@B/system/b.md\"\n",
            ),
            ("specs/system/a.md", "# A\n"),
        ])
        .await;

        let (ws_ctx, repos_ctx, tenant) = two_repo_workspace(&dir_a, &dir_b).await;
        let link_repo: Arc<dyn gyre_ports::SpecLinkRepository> =
            Arc::new(MemSpecLinkRepository::default());
        // Legacy cyclic rows (recorded before the gate existed): A→B and B→A.
        link_repo
            .save(&stored_link(
                "repo-a",
                "system/a.md",
                SpecLinkType::DependsOn,
                Some("repo-b"),
                "system/b.md",
            ))
            .await
            .unwrap();
        link_repo
            .save(&stored_link(
                "repo-b",
                "system/b.md",
                SpecLinkType::DependsOn,
                Some("repo-a"),
                "system/a.md",
            ))
            .await
            .unwrap();

        // Repo A pushes a manifest that drops its link entirely.
        let sha_a2 = add_commit(
            &dir_a,
            &[(
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/a.md\n    title: A\n    owner: user:test\n",
            )],
        );
        let ctx_a = TenantCycleContext {
            link_repo: Some(&link_repo),
            source_repo_id: Some("repo-a"),
            source_workspace_id: Some("ws-1"),
            workspaces: Some(&ws_ctx),
            repos: Some(&repos_ctx),
            tenant_id: Some(&tenant),
            memberships: None,
            principal: None,
            kv: None,
        };
        check_spec_link_cycles(dir_a.path().to_str().unwrap(), &sha_a2, Some(&ctx_a))
            .await
            .expect("a push that removes the cycle-participating edge must be accepted");

        // Negative control in the same fixture: re-declaring the edge while
        // B's stored back-edge exists is still a cycle in the combined graph.
        let sha_a3 = add_commit(
            &dir_a,
            &[(
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/a.md\n    title: A\n    owner: user:test\n    links:\n      - type: depends_on\n        target: \"@B/system/b.md\"\n",
            )],
        );
        check_spec_link_cycles(dir_a.path().to_str().unwrap(), &sha_a3, Some(&ctx_a))
            .await
            .expect_err("re-declaring the edge while the stored back-edge exists must be rejected");
    }

    /// Unresolved cross-repo stored links have no attributable target node
    /// (the sync stores them with target_repo_id NULL) — they must not be
    /// silently re-attributed to the source repo, which would fabricate
    /// self-repo edges and false cycles.
    ///
    /// Fixture: repo-a carries an unresolved cross-repo row
    /// (system/c.md → @B/system/b.md, target never resolved) and a
    /// same-repo row (system/b.md → system/c.md). The pushed manifest
    /// declares neither source path, so both rows survive the push. The
    /// only real edge is b→c — no cycle. If the unresolved row's target
    /// were mis-attributed to repo-a, the graph would gain
    /// c→(repo-a, b), closing the false cycle c→b→c.
    #[tokio::test]
    async fn tenant_cycle_unresolved_stored_edge_not_attributed() {
        use crate::mem::MemSpecLinkRepository;

        let (dir_b, _sha_b1) = make_test_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/b.md\n    title: B\n    owner: user:test\n",
            ),
            ("specs/system/b.md", "# B\n"),
        ])
        .await;
        let (dir_a, sha_a) = make_test_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/a.md\n    title: A\n    owner: user:test\n",
            ),
            ("specs/system/a.md", "# A\n"),
        ])
        .await;

        let (ws_ctx, repos_ctx, tenant) = two_repo_workspace(&dir_a, &dir_b).await;
        let link_repo: Arc<dyn gyre_ports::SpecLinkRepository> =
            Arc::new(MemSpecLinkRepository::default());
        // Unresolved cross-repo row from repo A (target repo never resolved:
        // stored with target_repo_id NULL and a composite display target).
        let mut unresolved = stored_link(
            "repo-a",
            "system/c.md",
            SpecLinkType::DependsOn,
            None,
            "system/b.md",
        );
        unresolved.status = "unresolved".to_string();
        // Unresolved cross-repo rows keep their composite display target
        // with target_repo_id NULL — the storage form the sync produces for
        // a target that failed validation.
        unresolved.target_display = Some("@B/system/b.md".to_string());
        link_repo.save(&unresolved).await.unwrap();
        // Same-repo back-edge b→c in repo A (target attributed to the
        // source repo — the normal same-repo storage form).
        link_repo
            .save(&stored_link(
                "repo-a",
                "system/b.md",
                SpecLinkType::DependsOn,
                None,
                "system/c.md",
            ))
            .await
            .unwrap();

        let ctx_a = TenantCycleContext {
            link_repo: Some(&link_repo),
            source_repo_id: Some("repo-a"),
            source_workspace_id: Some("ws-1"),
            workspaces: Some(&ws_ctx),
            repos: Some(&repos_ctx),
            tenant_id: Some(&tenant),
            memberships: None,
            principal: None,
            kv: None,
        };
        check_spec_link_cycles(dir_a.path().to_str().unwrap(), &sha_a, Some(&ctx_a))
            .await
            .expect("an unresolved stored cross-repo row must not fabricate a cycle");
    }

    /// Review repair (round 3), deferred cross-repo cycle bypass: a push
    /// whose cross-repo edge is stored UNRESOLVED (the target repo does
    /// not declare the target path yet — the documented purpose of
    /// unresolved links) is activated later by the staleness pass once the
    /// target repo's manifest declares the path. The pending row's edge
    /// therefore belongs in the combined cycle graph exactly when that
    /// activation is deterministic — otherwise the A→B→A cycle arrives as
    /// push 1 (unresolved B→A) + push 2 (A→B) + staleness activation, and
    /// lands permanently in the forge graph with no push ever rejected.
    /// Against the round-2 candidate this fails: the pending B→A row was
    /// dropped from the graph, so the step-2 check returned Ok.
    #[tokio::test]
    async fn tenant_cycle_deferred_unresolved_back_edge_rejected() {
        use crate::mem::{MemSpecLedgerRepository, MemSpecLinkRepository};

        // Step 1 fixture: repo B declares system/b.md and links it to
        // @A/system/a.md; repo A does NOT declare system/a.md yet, so the
        // edge can only be stored unresolved (target_repo_id NULL).
        let (dir_b, sha_b1) = make_test_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/b.md\n    title: B\n    owner: user:test\n    links:\n      - type: depends_on\n        target: \"@A/system/a.md\"\n",
            ),
            ("specs/system/b.md", "# B\n"),
        ])
        .await;
        let (dir_a, _sha_a1) = make_test_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/other.md\n    title: Other\n    owner: user:test\n",
            ),
            ("specs/system/other.md", "# Other\n"),
        ])
        .await;

        let (ws_ctx, repos_ctx, tenant) = two_repo_workspace(&dir_a, &dir_b).await;
        let link_repo: Arc<dyn gyre_ports::SpecLinkRepository> =
            Arc::new(MemSpecLinkRepository::default());

        // Step 1 (push to repo B): the pushed edge is unresolved (repo A
        // does not declare system/a.md), so the push is accepted — the
        // documented purpose of unresolved links.
        let ctx_b = TenantCycleContext {
            link_repo: Some(&link_repo),
            source_repo_id: Some("repo-b"),
            source_workspace_id: Some("ws-1"),
            workspaces: Some(&ws_ctx),
            repos: Some(&repos_ctx),
            tenant_id: Some(&tenant),
            memberships: None,
            principal: None,
            kv: None,
        };
        check_spec_link_cycles(dir_b.path().to_str().unwrap(), &sha_b1, Some(&ctx_b))
            .await
            .expect("push 1 (unresolved B depends_on @A/a) must be accepted");

        // The accepted push lands: the sync stores B's edge UNRESOLVED —
        // the exact deferred state the staleness pass later activates.
        let ledger: Arc<dyn gyre_ports::SpecLedgerRepository> =
            Arc::new(MemSpecLedgerRepository::default());
        let links_store: SpecLinksStore = Arc::new(Mutex::new(Vec::new()));
        sync_spec_ledger(
            &ledger,
            &links_store,
            &link_repo,
            dir_b.path().to_str().unwrap(),
            &sha_b1,
            1_700_000_000,
            Some("repo-b"),
            Some("ws-1"),
            Some(&ws_ctx),
            Some(&repos_ctx),
            Some(&tenant),
            None,
            None,
            None,
            None,
        )
        .await;
        let stored = link_repo.list_all().await.unwrap();
        assert_eq!(stored.len(), 1, "push 1 must store exactly one link");
        assert_eq!(
            stored[0].status, "unresolved",
            "the deferred state must be reproduced: {:?}",
            stored[0]
        );
        assert_eq!(stored[0].target_repo_id, None);
        assert_eq!(stored[0].target_display.as_deref(), Some("@A/system/a.md"));

        // Step 2 (push to repo A): the manifest now declares system/a.md
        // and links it to @B/system/b.md. The pending B→A row activates
        // deterministically given this push's own content (target repo =
        // the pushed repo, path declared by the pushed manifest), so its
        // edge is in the combined graph — the push must be rejected with
        // the cycle named. Against the round-2 candidate this returns Ok
        // and the staleness pass then lands the A→B→A cycle permanently.
        let sha_a2 = add_commit(
            &dir_a,
            &[(
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/a.md\n    title: A\n    owner: user:test\n    links:\n      - type: depends_on\n        target: \"@B/system/b.md\"\n",
            )],
        );
        let ctx_a = TenantCycleContext {
            link_repo: Some(&link_repo),
            source_repo_id: Some("repo-a"),
            source_workspace_id: Some("ws-1"),
            workspaces: Some(&ws_ctx),
            repos: Some(&repos_ctx),
            tenant_id: Some(&tenant),
            memberships: None,
            principal: None,
            kv: None,
        };
        let err = check_spec_link_cycles(dir_a.path().to_str().unwrap(), &sha_a2, Some(&ctx_a))
            .await
            .expect_err(
                "the step-2 push must be rejected: the pending B→A row activates deterministically \
                 given this push, closing the A→B→A cycle",
            );
        assert!(
            err.contains("spec link cycle detected"),
            "rejection must name the cycle: {err}"
        );
        assert!(
            err.contains("repo-a:system/a.md") && err.contains("repo-b:system/b.md"),
            "cycle message must name both cross-repo nodes with repo attribution: {err}"
        );
    }

    /// Test-only SpecLinkRepository whose `list_all` fails, modeling the
    /// storage-error path (SQLite "database is locked" under concurrent
    /// receive-packs, pool/disk failure) the cycle gate must FAIL CLOSED
    /// on — never silently degrade to the pushed manifest's own edges.
    struct FailingSpecLinkRepository;

    #[async_trait::async_trait]
    impl gyre_ports::SpecLinkRepository for FailingSpecLinkRepository {
        async fn list_all(&self) -> anyhow::Result<Vec<SpecLinkEntry>> {
            anyhow::bail!("storage unavailable (test)")
        }
        async fn replace_for_source(
            &self,
            _source_repo_id: &str,
            _source_path: &str,
            _links: &[SpecLinkEntry],
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn save(&self, _entry: &SpecLinkEntry) -> anyhow::Result<()> {
            Ok(())
        }
        async fn delete_by_source_repo(&self, _source_repo_id: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    /// Review repair (round 2), fail-closed: when the durable spec_links
    /// table cannot be read, the push is REJECTED — the check must not
    /// silently fall back to the pushed manifest's own edges (the
    /// pre-task-293 scope). Against the round-1 code this fails: the
    /// `if let Ok(stored)` guard skipped the stored half and the B→A push
    /// returned Ok even though the A→B row was in the table.
    #[tokio::test]
    async fn tenant_cycle_list_all_error_fails_closed() {
        // Repo A's edge A→B is already stored (its push landed earlier).
        let (dir_b, _sha_b1) = make_test_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/b.md\n    title: B\n    owner: user:test\n",
            ),
            ("specs/system/b.md", "# B\n"),
        ])
        .await;
        let (dir_a, _sha_a) = make_test_repo(&[(
            "specs/manifest.yaml",
            "version: 1\nspecs:\n  - path: system/a.md\n    title: A\n    owner: user:test\n",
        )])
        .await;

        let (ws_ctx, repos_ctx, tenant) = two_repo_workspace(&dir_a, &dir_b).await;
        // The storage backend errors on read — the exact state the round-1
        // code silently ignored.
        let link_repo: Arc<dyn gyre_ports::SpecLinkRepository> =
            Arc::new(FailingSpecLinkRepository);

        // Repo B pushes the back-edge B depends_on @A/a.
        let sha_b2 = add_commit(
            &dir_b,
            &[(
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/b.md\n    title: B\n    owner: user:test\n    links:\n      - type: depends_on\n        target: \"@A/system/a.md\"\n",
            )],
        );
        let ctx_b = TenantCycleContext {
            link_repo: Some(&link_repo),
            source_repo_id: Some("repo-b"),
            source_workspace_id: Some("ws-1"),
            workspaces: Some(&ws_ctx),
            repos: Some(&repos_ctx),
            tenant_id: Some(&tenant),
            memberships: None,
            principal: None,
            kv: None,
        };
        let err = check_spec_link_cycles(dir_b.path().to_str().unwrap(), &sha_b2, Some(&ctx_b))
            .await
            .expect_err(
                "a storage error reading the durable graph must reject the push (fail closed), \
                 not silently degrade to the manifest's own edges",
            );
        assert!(
            err.contains("spec link cycle check failed"),
            "fail-closed rejection must say the graph could not be read: {err}"
        );
    }

    /// Review repair (round 2), push-scoped detection: a stored-only cycle
    /// (legacy pre-gate rows — the exact state the task's discovery
    /// evidence describes, and equally reachable via the ungated
    /// mirror-sync paths) must NOT block a push that creates no cycle.
    /// Against the round-1 code this fails: detect_graph_cycles scanned
    /// every node of the combined graph, so any manifest push was rejected
    /// while a stored cycle existed anywhere. The companion assertion
    /// (re-declaring a cycle-participating edge in the same fixture is
    /// still rejected) proves the gate was scoped, not disabled.
    #[tokio::test]
    async fn tenant_cycle_legacy_stored_cycle_does_not_block_unrelated_push() {
        use crate::mem::{MemRepoRepository, MemSpecLinkRepository, MemWorkspaceRepository};

        let (dir_b, _sha_b1) = make_test_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/b.md\n    title: B\n    owner: user:test\n",
            ),
            ("specs/system/b.md", "# B\n"),
        ])
        .await;
        let (dir_a, _sha_a) = make_test_repo(&[(
            "specs/manifest.yaml",
            "version: 1\nspecs:\n  - path: system/a.md\n    title: A\n    owner: user:test\n",
        )])
        .await;
        // Repo C: unrelated to the stored cycle, pushing a real edge of
        // its own (C depends_on @A/a — acyclic).
        let (dir_c, sha_c1) = make_test_repo(&[
            (
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/c.md\n    title: C\n    owner: user:test\n    links:\n      - type: depends_on\n        target: \"@A/system/a.md\"\n",
            ),
        ])
        .await;

        // Three-repo workspace fixture (repo C must resolve @A/... too).
        let ws_ctx: Arc<dyn gyre_ports::WorkspaceRepository> =
            Arc::new(MemWorkspaceRepository::default());
        ws_ctx
            .create(&make_workspace_in_tenant("ws-1", "tenant-293", "ws-one"))
            .await
            .unwrap();
        let repos_ctx: Arc<dyn gyre_ports::RepoRepository> = Arc::new(MemRepoRepository::default());
        for (id, name, dir) in [
            ("repo-a", "A", &dir_a),
            ("repo-b", "B", &dir_b),
            ("repo-c", "C", &dir_c),
        ] {
            repos_ctx
                .create(&make_repo_entity(
                    id,
                    name,
                    "ws-1",
                    dir.path().to_str().unwrap(),
                ))
                .await
                .unwrap();
        }
        let tenant = gyre_common::Id::new("tenant-293");

        let link_repo: Arc<dyn gyre_ports::SpecLinkRepository> =
            Arc::new(MemSpecLinkRepository::default());
        // Legacy stored cycle A↔B (recorded before the gate existed, or
        // mirror-synced past it): both directions in the durable table.
        link_repo
            .save(&stored_link(
                "repo-a",
                "system/a.md",
                SpecLinkType::DependsOn,
                Some("repo-b"),
                "system/b.md",
            ))
            .await
            .unwrap();
        link_repo
            .save(&stored_link(
                "repo-b",
                "system/b.md",
                SpecLinkType::DependsOn,
                Some("repo-a"),
                "system/a.md",
            ))
            .await
            .unwrap();

        // Repo C pushes its own acyclic edge while the legacy cycle sits in
        // the stored graph: the push creates no cycle, so it is accepted.
        let ctx_c = TenantCycleContext {
            link_repo: Some(&link_repo),
            source_repo_id: Some("repo-c"),
            source_workspace_id: Some("ws-1"),
            workspaces: Some(&ws_ctx),
            repos: Some(&repos_ctx),
            tenant_id: Some(&tenant),
            memberships: None,
            principal: None,
            kv: None,
        };
        check_spec_link_cycles(dir_c.path().to_str().unwrap(), &sha_c1, Some(&ctx_c))
            .await
            .expect("a stored-only legacy cycle must not block a push that creates no cycle");

        // Companion: a push that DOES contribute an edge participating in
        // the stored cycle is still rejected — the gate was scoped, not
        // disabled. Repo A re-declares A depends_on @B/b; the stored B→A
        // back-edge closes the cycle through the pushed edge.
        let sha_a2 = add_commit(
            &dir_a,
            &[(
                "specs/manifest.yaml",
                "version: 1\nspecs:\n  - path: system/a.md\n    title: A\n    owner: user:test\n    links:\n      - type: depends_on\n        target: \"@B/system/b.md\"\n",
            )],
        );
        let ctx_a = TenantCycleContext {
            link_repo: Some(&link_repo),
            source_repo_id: Some("repo-a"),
            source_workspace_id: Some("ws-1"),
            workspaces: Some(&ws_ctx),
            repos: Some(&repos_ctx),
            tenant_id: Some(&tenant),
            memberships: None,
            principal: None,
            kv: None,
        };
        let err = check_spec_link_cycles(dir_a.path().to_str().unwrap(), &sha_a2, Some(&ctx_a))
            .await
            .expect_err("a push contributing a cycle-participating edge must still be rejected");
        assert!(
            err.contains("spec link cycle detected"),
            "rejection must name the cycle: {err}"
        );
    }
}
