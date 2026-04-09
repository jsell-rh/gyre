//! Spec graph patrol — accountability agent integration (TASK-023).
//!
//! spec-links.md §Accountability Agent Integration: "The Accountability agent's
//! patrol gains spec-graph awareness" with five checks:
//! - Stale links: flag specs with stale links that haven't been reviewed
//! - Orphaned supersessions: a spec is superseded but code still references it
//! - Unresolved conflicts: two conflicting specs are both approved
//! - Dangling implementations: an `implements` link points to a spec that was deleted
//! - Deep dependency chains: specs with >5 levels of `depends_on` (decomposition smell)

use std::collections::{HashMap, HashSet, VecDeque};

use gyre_common::{Id, Notification, NotificationType};
use gyre_domain::{ApprovalStatus, WorkspaceRole};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::spec_registry::{SpecLinkEntry, SpecLinkType};
use crate::AppState;

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

/// Optional query parameters for the patrol endpoint.
#[derive(Debug, Deserialize)]
pub struct PatrolRequest {
    /// Stale link threshold in seconds (default: 7 days).
    pub stale_threshold_secs: Option<u64>,
}

/// A single finding from the spec-graph patrol.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PatrolFinding {
    /// Finding type: `stale_link`, `orphaned_supersession`, `unresolved_conflict`,
    /// `dangling_implementation`, `deep_dependency_chain`.
    #[serde(rename = "type")]
    pub finding_type: String,
    /// Severity: `error`, `warning`, or `info`.
    pub severity: String,
    /// The spec path where the finding originates.
    pub spec_path: String,
    /// Human-readable description of the finding.
    pub detail: String,
    /// Suggested remediation action.
    pub suggested_action: String,
}

/// Response from the patrol endpoint.
#[derive(Debug, Serialize)]
pub struct PatrolResponse {
    pub findings: Vec<PatrolFinding>,
}

// ---------------------------------------------------------------------------
// Patrol execution
// ---------------------------------------------------------------------------

/// Run all 5 spec-graph patrol checks and return findings.
///
/// This function is the core logic shared by the API handler and tests.
pub async fn run_patrol(
    state: &AppState,
    now_secs: u64,
    stale_threshold_secs: u64,
) -> Vec<PatrolFinding> {
    let mut findings = Vec::new();

    // Snapshot links and ledger to avoid holding locks across checks.
    let links: Vec<SpecLinkEntry> = {
        let store = state.spec_links_store.lock().await;
        store.clone()
    };
    let all_entries = state.spec_ledger.list_all().await.unwrap_or_default();
    let ledger_map: HashMap<String, &gyre_domain::SpecLedgerEntry> =
        all_entries.iter().map(|e| (e.path.clone(), e)).collect();

    // 1. Stale links beyond threshold
    check_stale_links(&links, now_secs, stale_threshold_secs, &mut findings);

    // 2. Orphaned supersessions
    check_orphaned_supersessions(&links, &ledger_map, &mut findings);

    // 3. Unresolved conflicts
    check_unresolved_conflicts(&links, &ledger_map, &mut findings);

    // 4. Dangling implementations
    check_dangling_implementations(&links, &ledger_map, &mut findings);

    // 5. Deep dependency chains
    check_deep_dependency_chains(&links, &mut findings);

    findings
}

/// Check 1: Flag specs with stale links that haven't been reviewed.
///
/// Finds links with `status == "stale"` whose `stale_since` is older than
/// the configured threshold.
fn check_stale_links(
    links: &[SpecLinkEntry],
    now_secs: u64,
    threshold_secs: u64,
    findings: &mut Vec<PatrolFinding>,
) {
    for link in links {
        if link.status != "stale" {
            continue;
        }
        if let Some(stale_since) = link.stale_since {
            let elapsed = now_secs.saturating_sub(stale_since);
            if elapsed >= threshold_secs {
                let days = elapsed / 86400;
                findings.push(PatrolFinding {
                    finding_type: "stale_link".to_string(),
                    severity: "warning".to_string(),
                    spec_path: link.source_path.clone(),
                    detail: format!(
                        "Stale since {} days ago, linked to '{}'",
                        days, link.target_path
                    ),
                    suggested_action: format!(
                        "Review the {} link from '{}' to '{}' and update the target_sha",
                        link.link_type, link.source_path, link.target_path
                    ),
                });
            }
        }
    }
}

/// Check 2: Orphaned supersessions — a spec is superseded (deprecated) but
/// other specs still reference it via non-supersedes links.
///
/// spec-links.md §Link Types: "Old spec marked deprecated in registry.
/// Code referencing old spec gets flagged."
fn check_orphaned_supersessions(
    links: &[SpecLinkEntry],
    ledger_map: &HashMap<String, &gyre_domain::SpecLedgerEntry>,
    findings: &mut Vec<PatrolFinding>,
) {
    // Find all specs that are deprecated (superseded).
    let deprecated_specs: HashSet<&str> = ledger_map
        .iter()
        .filter(|(_, e)| e.approval_status == ApprovalStatus::Deprecated)
        .map(|(path, _)| path.as_str())
        .collect();

    if deprecated_specs.is_empty() {
        return;
    }

    // Find supersedes links to identify what superseded what.
    let superseded_by: HashMap<&str, &str> = links
        .iter()
        .filter(|l| l.link_type == SpecLinkType::Supersedes)
        .map(|l| (l.target_path.as_str(), l.source_path.as_str()))
        .collect();

    // Check all links that target a deprecated spec (except supersedes links themselves).
    for link in links {
        if link.link_type == SpecLinkType::Supersedes {
            continue;
        }
        if deprecated_specs.contains(link.target_path.as_str()) {
            let superseder = superseded_by
                .get(link.target_path.as_str())
                .unwrap_or(&"unknown");
            findings.push(PatrolFinding {
                finding_type: "orphaned_supersession".to_string(),
                severity: "warning".to_string(),
                spec_path: link.source_path.clone(),
                detail: format!(
                    "References superseded spec '{}' (superseded by '{}')",
                    link.target_path, superseder
                ),
                suggested_action: format!(
                    "Update the {} link in '{}' to reference '{}' instead of the superseded '{}'",
                    link.link_type, link.source_path, superseder, link.target_path
                ),
            });
        }
    }
}

/// Check 3: Unresolved conflicts — two conflicting specs are both approved.
///
/// spec-links.md §Approval Gates: "Both specs cannot have approval_status: approved
/// simultaneously. The forge rejects the second approval."
///
/// Checks both directions of conflicts_with links since they are bidirectional.
fn check_unresolved_conflicts(
    links: &[SpecLinkEntry],
    ledger_map: &HashMap<String, &gyre_domain::SpecLedgerEntry>,
    findings: &mut Vec<PatrolFinding>,
) {
    for link in links {
        if link.link_type != SpecLinkType::ConflictsWith {
            continue;
        }
        let source_approved = ledger_map
            .get(link.source_path.as_str())
            .map_or(false, |e| e.approval_status == ApprovalStatus::Approved);
        let target_approved = ledger_map
            .get(link.target_path.as_str())
            .map_or(false, |e| e.approval_status == ApprovalStatus::Approved);

        if source_approved && target_approved {
            findings.push(PatrolFinding {
                finding_type: "unresolved_conflict".to_string(),
                severity: "error".to_string(),
                spec_path: link.source_path.clone(),
                detail: format!("Conflicts with '{}', both approved", link.target_path),
                suggested_action: format!(
                    "Resolve the conflict between '{}' and '{}' — revoke approval \
                     from one or remove the conflicts_with link",
                    link.source_path, link.target_path
                ),
            });
        }
    }
}

/// Check 4: Dangling implementations — an `implements` link points to a spec
/// that no longer exists in the ledger (was deleted from the manifest).
fn check_dangling_implementations(
    links: &[SpecLinkEntry],
    ledger_map: &HashMap<String, &gyre_domain::SpecLedgerEntry>,
    findings: &mut Vec<PatrolFinding>,
) {
    for link in links {
        if link.link_type != SpecLinkType::Implements {
            continue;
        }
        if !ledger_map.contains_key(link.target_path.as_str()) {
            findings.push(PatrolFinding {
                finding_type: "dangling_implementation".to_string(),
                severity: "error".to_string(),
                spec_path: link.source_path.clone(),
                detail: format!("Implements '{}' which no longer exists", link.target_path),
                suggested_action: format!(
                    "Remove or update the implements link in '{}' — the target spec \
                     '{}' has been deleted from the manifest",
                    link.source_path, link.target_path
                ),
            });
        }
    }
}

/// Check 5: Deep dependency chains — specs with >5 levels of `depends_on`.
///
/// spec-links.md §Accountability Agent Integration: "Deep dependency chains:
/// specs with >5 levels of depends_on (decomposition smell)."
fn check_deep_dependency_chains(links: &[SpecLinkEntry], findings: &mut Vec<PatrolFinding>) {
    // Build adjacency list for depends_on links only.
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for link in links {
        if link.link_type == SpecLinkType::DependsOn {
            adj.entry(link.source_path.as_str())
                .or_default()
                .push(link.target_path.as_str());
        }
    }

    if adj.is_empty() {
        return;
    }

    // Collect all nodes.
    let all_nodes: HashSet<&str> = adj
        .keys()
        .copied()
        .chain(adj.values().flat_map(|vs| vs.iter().copied()))
        .collect();

    // BFS from each node to find max depth of depends_on chain.
    for &start in &all_nodes {
        let depth = compute_chain_depth(start, &adj);
        if depth > 5 {
            findings.push(PatrolFinding {
                finding_type: "deep_dependency_chain".to_string(),
                severity: "info".to_string(),
                spec_path: start.to_string(),
                detail: format!("Dependency chain depth: {}", depth),
                suggested_action: format!(
                    "Consider decomposing the dependency chain starting at '{}' — \
                     chains deeper than 5 levels indicate a decomposition smell",
                    start
                ),
            });
        }
    }
}

/// Compute the longest `depends_on` chain depth from a given node using BFS.
fn compute_chain_depth<'a>(start: &'a str, adj: &HashMap<&'a str, Vec<&'a str>>) -> usize {
    let mut max_depth = 0usize;
    let mut queue: VecDeque<(&str, usize)> = VecDeque::new();
    let mut visited: HashSet<&str> = HashSet::new();
    visited.insert(start);
    queue.push_back((start, 0));

    while let Some((node, depth)) = queue.pop_front() {
        if depth > max_depth {
            max_depth = depth;
        }
        if let Some(targets) = adj.get(node) {
            for &target in targets {
                if !visited.contains(target) {
                    visited.insert(target);
                    queue.push_back((target, depth + 1));
                }
            }
        }
    }

    max_depth
}

// ---------------------------------------------------------------------------
// Notification creation for error-severity findings
// ---------------------------------------------------------------------------

/// Create priority-3 notifications for workspace Admin/Developer members
/// when error-severity findings are detected.
///
/// spec-links.md §Accountability Agent Integration: error-severity findings
/// (unresolved conflicts, dangling implementations) warrant notifications.
pub async fn create_notifications_for_error_findings(
    state: &AppState,
    findings: &[PatrolFinding],
    now_secs: u64,
) {
    let error_findings: Vec<&PatrolFinding> =
        findings.iter().filter(|f| f.severity == "error").collect();

    if error_findings.is_empty() {
        return;
    }

    // Resolve workspace/tenant context from the links store.
    // Group findings by source spec's workspace.
    let links: Vec<SpecLinkEntry> = {
        let store = state.spec_links_store.lock().await;
        store.clone()
    };

    // Build a map: source_path -> (source_repo_id, workspace_id).
    let mut spec_context: HashMap<String, (String, String)> = HashMap::new();
    for link in &links {
        if let Some(ref repo_id) = link.source_repo_id {
            if !spec_context.contains_key(&link.source_path) {
                // Resolve workspace from repo.
                if let Ok(Some(repo)) = state.repos.find_by_id(&Id::new(repo_id)).await {
                    spec_context.insert(
                        link.source_path.clone(),
                        (repo_id.clone(), repo.workspace_id.to_string()),
                    );
                }
            }
        }
    }

    // Also try to resolve from ledger entries for specs that might not have links.
    let all_entries = state.spec_ledger.list_all().await.unwrap_or_default();
    for entry in &all_entries {
        if !spec_context.contains_key(&entry.path) {
            if let (Some(ref repo_id), Some(ref ws_id)) = (&entry.repo_id, &entry.workspace_id) {
                spec_context.insert(entry.path.clone(), (repo_id.clone(), ws_id.clone()));
            }
        }
    }

    // Create notifications per workspace.
    let mut notified_workspaces: HashSet<String> = HashSet::new();

    for finding in &error_findings {
        let Some((_, workspace_id)) = spec_context.get(&finding.spec_path) else {
            continue;
        };

        // Avoid duplicate notifications per workspace for multiple findings.
        let ws_key = format!("{}:{}", workspace_id, finding.finding_type);
        if notified_workspaces.contains(&ws_key) {
            continue;
        }
        notified_workspaces.insert(ws_key);

        // Resolve tenant_id from workspace.
        let tenant_id = match state.workspaces.find_by_id(&Id::new(workspace_id)).await {
            Ok(Some(ws)) => ws.tenant_id.to_string(),
            _ => continue,
        };

        // Get workspace members.
        let members = match state
            .workspace_memberships
            .list_by_workspace(&Id::new(workspace_id))
            .await
        {
            Ok(m) => m,
            Err(e) => {
                warn!(
                    workspace_id = %workspace_id,
                    error = %e,
                    "spec_patrol: failed to list workspace members"
                );
                continue;
            }
        };

        let now_i64 = now_secs as i64;

        for member in &members {
            if !matches!(member.role, WorkspaceRole::Admin | WorkspaceRole::Developer) {
                continue;
            }

            let id = Id::new(uuid::Uuid::new_v4().to_string());
            let title = match finding.finding_type.as_str() {
                "unresolved_conflict" => format!("Spec graph patrol: {}", finding.detail),
                "dangling_implementation" => format!("Spec graph patrol: {}", finding.detail),
                _ => format!("Spec graph patrol: {}", finding.detail),
            };

            let mut notif = Notification::new(
                id,
                Id::new(workspace_id),
                member.user_id.clone(),
                NotificationType::GateFailure,
                title,
                &tenant_id,
                now_i64,
            );
            // Override priority to 3 per task plan.
            notif.priority = 3;
            notif.body = Some(
                serde_json::json!({
                    "finding_type": finding.finding_type,
                    "severity": finding.severity,
                    "spec_path": finding.spec_path,
                    "detail": finding.detail,
                    "suggested_action": finding.suggested_action,
                })
                .to_string(),
            );
            notif.entity_ref = Some(finding.spec_path.clone());

            if let Err(e) = state.notifications.create(&notif).await {
                warn!(
                    user = %member.user_id,
                    error = %e,
                    "spec_patrol: failed to create notification"
                );
            }
        }
    }

    if !error_findings.is_empty() {
        info!(
            count = error_findings.len(),
            "spec_patrol: notifications created for error-severity findings"
        );
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use crate::spec_registry::{SpecLinkEntry, SpecLinkType};
    use gyre_domain::{ApprovalStatus, SpecLedgerEntry, WorkspaceMembership, WorkspaceRole};

    fn make_ledger_entry(path: &str, sha: &str, status: ApprovalStatus) -> SpecLedgerEntry {
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

    fn make_link(
        id: &str,
        source: &str,
        target: &str,
        link_type: SpecLinkType,
        status: &str,
        stale_since: Option<u64>,
    ) -> SpecLinkEntry {
        SpecLinkEntry {
            id: id.to_string(),
            source_path: source.to_string(),
            source_repo_id: Some("repo1".to_string()),
            link_type,
            target_path: target.to_string(),
            target_repo_id: None,
            target_display: None,
            target_sha: Some("sha123".to_string()),
            reason: None,
            status: status.to_string(),
            created_at: 1_000_000,
            stale_since,
        }
    }

    fn make_workspace(id: &str) -> gyre_domain::Workspace {
        gyre_domain::Workspace {
            id: Id::new(id),
            tenant_id: Id::new("default"),
            name: format!("ws-{id}"),
            slug: format!("ws-{id}"),
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

    fn make_membership(user_id: &str, ws_id: &str, role: WorkspaceRole) -> WorkspaceMembership {
        WorkspaceMembership {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            user_id: Id::new(user_id),
            workspace_id: Id::new(ws_id),
            role,
            invited_by: Id::new("admin"),
            accepted: true,
            accepted_at: Some(1_000_000),
            created_at: 1_000_000,
        }
    }

    fn make_repo(id: &str, ws_id: &str) -> gyre_domain::Repository {
        gyre_domain::Repository {
            id: Id::new(id),
            name: format!("repo-{id}"),
            path: format!("/repos/{id}"),
            workspace_id: Id::new(ws_id),
            default_branch: "main".to_string(),
            is_mirror: false,
            mirror_url: None,
            mirror_interval_secs: None,
            last_mirror_sync: None,
            description: None,
            status: gyre_domain::RepoStatus::Active,
            created_at: 1_000_000,
            updated_at: 1_000_000,
        }
    }

    // -----------------------------------------------------------------------
    // Check 1: Stale links
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn patrol_detects_stale_links_beyond_threshold() {
        let state = test_state();
        let now = 2_000_000u64;
        let threshold = 7 * 24 * 60 * 60; // 7 days

        // Stale link that has been stale for 10 days (older than threshold).
        let stale_link = make_link(
            "link-stale",
            "system/a.md",
            "system/b.md",
            SpecLinkType::DependsOn,
            "stale",
            Some(now - 10 * 86400), // 10 days ago
        );
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(stale_link);
        }

        let findings = run_patrol(&state, now, threshold).await;
        let stale_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.finding_type == "stale_link")
            .collect();
        assert_eq!(stale_findings.len(), 1);
        assert_eq!(stale_findings[0].severity, "warning");
        assert_eq!(stale_findings[0].spec_path, "system/a.md");
        assert!(stale_findings[0].detail.contains("system/b.md"));
        assert!(stale_findings[0].suggested_action.contains("depends_on"));
    }

    #[tokio::test]
    async fn patrol_skips_stale_links_within_threshold() {
        let state = test_state();
        let now = 2_000_000u64;
        let threshold = 7 * 24 * 60 * 60;

        // Stale link that has been stale for only 1 day (within threshold).
        let recent_stale = make_link(
            "link-recent-stale",
            "system/a.md",
            "system/b.md",
            SpecLinkType::DependsOn,
            "stale",
            Some(now - 1 * 86400), // 1 day ago
        );
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(recent_stale);
        }

        let findings = run_patrol(&state, now, threshold).await;
        let stale_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.finding_type == "stale_link")
            .collect();
        assert_eq!(
            stale_findings.len(),
            0,
            "stale link within threshold should not be flagged"
        );
    }

    #[tokio::test]
    async fn patrol_skips_active_links() {
        let state = test_state();
        let now = 2_000_000u64;

        let active_link = make_link(
            "link-active",
            "system/a.md",
            "system/b.md",
            SpecLinkType::DependsOn,
            "active",
            None,
        );
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(active_link);
        }

        let findings = run_patrol(&state, now, 7 * 24 * 60 * 60).await;
        let stale_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.finding_type == "stale_link")
            .collect();
        assert_eq!(stale_findings.len(), 0);
    }

    // -----------------------------------------------------------------------
    // Check 2: Orphaned supersessions
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn patrol_detects_orphaned_supersession() {
        let state = test_state();
        let now = 2_000_000u64;

        // old-spec.md is deprecated (superseded).
        let old_spec = make_ledger_entry("system/old-spec.md", "sha1", ApprovalStatus::Deprecated);
        state.spec_ledger.save(&old_spec).await.unwrap();

        // new-spec.md supersedes old-spec.md.
        let new_spec = make_ledger_entry("system/new-spec.md", "sha2", ApprovalStatus::Approved);
        state.spec_ledger.save(&new_spec).await.unwrap();

        // Supersedes link.
        let supersedes_link = make_link(
            "link-sup",
            "system/new-spec.md",
            "system/old-spec.md",
            SpecLinkType::Supersedes,
            "active",
            None,
        );

        // Another spec still references old-spec.md via depends_on.
        let orphan_link = make_link(
            "link-orphan",
            "system/consumer.md",
            "system/old-spec.md",
            SpecLinkType::DependsOn,
            "active",
            None,
        );

        {
            let mut store = state.spec_links_store.lock().await;
            store.push(supersedes_link);
            store.push(orphan_link);
        }

        let findings = run_patrol(&state, now, 7 * 24 * 60 * 60).await;
        let orphan_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.finding_type == "orphaned_supersession")
            .collect();
        assert_eq!(orphan_findings.len(), 1);
        assert_eq!(orphan_findings[0].severity, "warning");
        assert_eq!(orphan_findings[0].spec_path, "system/consumer.md");
        assert!(orphan_findings[0].detail.contains("system/old-spec.md"));
        assert!(orphan_findings[0].detail.contains("system/new-spec.md"));
        assert!(orphan_findings[0]
            .suggested_action
            .contains("system/new-spec.md"));
    }

    #[tokio::test]
    async fn patrol_no_orphaned_supersession_when_no_deprecated_specs() {
        let state = test_state();
        let now = 2_000_000u64;

        let spec = make_ledger_entry("system/a.md", "sha1", ApprovalStatus::Approved);
        state.spec_ledger.save(&spec).await.unwrap();

        let link = make_link(
            "link-normal",
            "system/b.md",
            "system/a.md",
            SpecLinkType::DependsOn,
            "active",
            None,
        );
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(link);
        }

        let findings = run_patrol(&state, now, 7 * 24 * 60 * 60).await;
        let orphan_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.finding_type == "orphaned_supersession")
            .collect();
        assert_eq!(orphan_findings.len(), 0);
    }

    // -----------------------------------------------------------------------
    // Check 3: Unresolved conflicts
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn patrol_detects_unresolved_conflict() {
        let state = test_state();
        let now = 2_000_000u64;

        // Both conflicting specs are approved — violation.
        let spec_a = make_ledger_entry("system/spec-a.md", "sha1", ApprovalStatus::Approved);
        let spec_b = make_ledger_entry("system/spec-b.md", "sha2", ApprovalStatus::Approved);
        state.spec_ledger.save(&spec_a).await.unwrap();
        state.spec_ledger.save(&spec_b).await.unwrap();

        let conflict_link = make_link(
            "link-conflict",
            "system/spec-a.md",
            "system/spec-b.md",
            SpecLinkType::ConflictsWith,
            "active",
            None,
        );
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(conflict_link);
        }

        let findings = run_patrol(&state, now, 7 * 24 * 60 * 60).await;
        let conflict_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.finding_type == "unresolved_conflict")
            .collect();
        assert_eq!(conflict_findings.len(), 1);
        assert_eq!(conflict_findings[0].severity, "error");
        assert_eq!(conflict_findings[0].spec_path, "system/spec-a.md");
        assert!(conflict_findings[0].detail.contains("system/spec-b.md"));
    }

    #[tokio::test]
    async fn patrol_no_conflict_when_one_spec_pending() {
        let state = test_state();
        let now = 2_000_000u64;

        // Negative test: one spec is Pending → no conflict finding.
        let spec_a = make_ledger_entry("system/spec-a.md", "sha1", ApprovalStatus::Approved);
        let spec_b = make_ledger_entry("system/spec-b.md", "sha2", ApprovalStatus::Pending);
        state.spec_ledger.save(&spec_a).await.unwrap();
        state.spec_ledger.save(&spec_b).await.unwrap();

        let conflict_link = make_link(
            "link-conflict",
            "system/spec-a.md",
            "system/spec-b.md",
            SpecLinkType::ConflictsWith,
            "active",
            None,
        );
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(conflict_link);
        }

        let findings = run_patrol(&state, now, 7 * 24 * 60 * 60).await;
        let conflict_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.finding_type == "unresolved_conflict")
            .collect();
        assert_eq!(
            conflict_findings.len(),
            0,
            "no conflict when one spec is Pending"
        );
    }

    // -----------------------------------------------------------------------
    // Check 4: Dangling implementations
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn patrol_detects_dangling_implementation() {
        let state = test_state();
        let now = 2_000_000u64;

        // The source spec exists but the target spec has been deleted from the manifest.
        // Don't add target to ledger — simulates deletion.
        let source = make_ledger_entry("system/impl.md", "sha1", ApprovalStatus::Pending);
        state.spec_ledger.save(&source).await.unwrap();

        let link = make_link(
            "link-dangling",
            "system/impl.md",
            "system/deleted-parent.md",
            SpecLinkType::Implements,
            "active",
            None,
        );
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(link);
        }

        let findings = run_patrol(&state, now, 7 * 24 * 60 * 60).await;
        let dangling_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.finding_type == "dangling_implementation")
            .collect();
        assert_eq!(dangling_findings.len(), 1);
        assert_eq!(dangling_findings[0].severity, "error");
        assert_eq!(dangling_findings[0].spec_path, "system/impl.md");
        assert!(dangling_findings[0]
            .detail
            .contains("system/deleted-parent.md"));
    }

    #[tokio::test]
    async fn patrol_no_dangling_when_target_exists() {
        let state = test_state();
        let now = 2_000_000u64;

        // Both specs exist in ledger — no dangling.
        let source = make_ledger_entry("system/impl.md", "sha1", ApprovalStatus::Pending);
        let target = make_ledger_entry("system/parent.md", "sha2", ApprovalStatus::Approved);
        state.spec_ledger.save(&source).await.unwrap();
        state.spec_ledger.save(&target).await.unwrap();

        let link = make_link(
            "link-ok",
            "system/impl.md",
            "system/parent.md",
            SpecLinkType::Implements,
            "active",
            None,
        );
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(link);
        }

        let findings = run_patrol(&state, now, 7 * 24 * 60 * 60).await;
        let dangling_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.finding_type == "dangling_implementation")
            .collect();
        assert_eq!(dangling_findings.len(), 0);
    }

    // -----------------------------------------------------------------------
    // Check 5: Deep dependency chains
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn patrol_detects_deep_dependency_chain() {
        let state = test_state();
        let now = 2_000_000u64;

        // Create a chain: a → b → c → d → e → f → g (depth 6, >5).
        let chain = [
            ("system/a.md", "system/b.md"),
            ("system/b.md", "system/c.md"),
            ("system/c.md", "system/d.md"),
            ("system/d.md", "system/e.md"),
            ("system/e.md", "system/f.md"),
            ("system/f.md", "system/g.md"),
        ];
        {
            let mut store = state.spec_links_store.lock().await;
            for (i, (src, tgt)) in chain.iter().enumerate() {
                store.push(make_link(
                    &format!("chain-{i}"),
                    src,
                    tgt,
                    SpecLinkType::DependsOn,
                    "active",
                    None,
                ));
            }
        }

        let findings = run_patrol(&state, now, 7 * 24 * 60 * 60).await;
        let deep_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.finding_type == "deep_dependency_chain")
            .collect();
        assert!(!deep_findings.is_empty(), "should detect deep chain");
        // The root node "system/a.md" has depth 6.
        let root_finding = deep_findings.iter().find(|f| f.spec_path == "system/a.md");
        assert!(root_finding.is_some(), "root node should be flagged");
        assert_eq!(root_finding.unwrap().severity, "info");
        assert!(root_finding.unwrap().detail.contains("6"));
    }

    #[tokio::test]
    async fn patrol_no_deep_chain_for_short_dependency() {
        let state = test_state();
        let now = 2_000_000u64;

        // Chain of depth 3 (a → b → c → d) — not deep enough.
        let chain = [
            ("system/a.md", "system/b.md"),
            ("system/b.md", "system/c.md"),
            ("system/c.md", "system/d.md"),
        ];
        {
            let mut store = state.spec_links_store.lock().await;
            for (i, (src, tgt)) in chain.iter().enumerate() {
                store.push(make_link(
                    &format!("short-{i}"),
                    src,
                    tgt,
                    SpecLinkType::DependsOn,
                    "active",
                    None,
                ));
            }
        }

        let findings = run_patrol(&state, now, 7 * 24 * 60 * 60).await;
        let deep_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.finding_type == "deep_dependency_chain")
            .collect();
        assert_eq!(
            deep_findings.len(),
            0,
            "chain of depth 3 should not be flagged"
        );
    }

    // -----------------------------------------------------------------------
    // Empty findings (no issues)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn patrol_returns_empty_findings_when_no_issues() {
        let state = test_state();
        let now = 2_000_000u64;

        let findings = run_patrol(&state, now, 7 * 24 * 60 * 60).await;
        assert!(findings.is_empty(), "no links → no findings");
    }

    // -----------------------------------------------------------------------
    // Notification integration
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn patrol_creates_notifications_for_error_findings() {
        let state = test_state();
        let now = 2_000_000u64;

        // Set up workspace + repo + members.
        let ws = make_workspace("ws1");
        state.workspaces.create(&ws).await.unwrap();

        let repo = make_repo("repo1", "ws1");
        state.repos.create(&repo).await.unwrap();

        let admin = make_membership("admin1", "ws1", WorkspaceRole::Admin);
        let dev = make_membership("dev1", "ws1", WorkspaceRole::Developer);
        let viewer = make_membership("viewer1", "ws1", WorkspaceRole::Viewer);
        state.workspace_memberships.create(&admin).await.unwrap();
        state.workspace_memberships.create(&dev).await.unwrap();
        state.workspace_memberships.create(&viewer).await.unwrap();

        // Seed ledger entries for context resolution.
        let spec = make_ledger_entry("system/spec-a.md", "sha1", ApprovalStatus::Approved);
        state.spec_ledger.save(&spec).await.unwrap();

        // Create an error-severity finding.
        let findings = vec![PatrolFinding {
            finding_type: "unresolved_conflict".to_string(),
            severity: "error".to_string(),
            spec_path: "system/spec-a.md".to_string(),
            detail: "Conflicts with 'system/spec-b.md', both approved".to_string(),
            suggested_action: "Resolve the conflict".to_string(),
        }];

        create_notifications_for_error_findings(&state, &findings, now).await;

        // Admin should get notification.
        let admin_notifs = state
            .notifications
            .list_for_user(
                &Id::new("admin1"),
                Some(&Id::new("ws1")),
                None,
                None,
                None,
                10,
                0,
            )
            .await
            .unwrap();
        assert_eq!(admin_notifs.len(), 1, "Admin should receive notification");
        assert_eq!(admin_notifs[0].priority, 3);
        assert!(admin_notifs[0].title.contains("Spec graph patrol"));
        assert_eq!(
            admin_notifs[0].entity_ref,
            Some("system/spec-a.md".to_string())
        );
        // Verify body contains expected fields.
        let body: serde_json::Value =
            serde_json::from_str(admin_notifs[0].body.as_deref().unwrap()).unwrap();
        assert_eq!(body["finding_type"], "unresolved_conflict");
        assert_eq!(body["severity"], "error");
        assert_eq!(body["spec_path"], "system/spec-a.md");
        assert!(body["detail"].as_str().unwrap().contains("Conflicts"));
        assert!(body["suggested_action"]
            .as_str()
            .unwrap()
            .contains("Resolve"));

        // Developer should get notification.
        let dev_notifs = state
            .notifications
            .list_for_user(
                &Id::new("dev1"),
                Some(&Id::new("ws1")),
                None,
                None,
                None,
                10,
                0,
            )
            .await
            .unwrap();
        assert_eq!(dev_notifs.len(), 1, "Developer should receive notification");
        assert_eq!(dev_notifs[0].priority, 3);

        // Viewer should NOT get notification.
        let viewer_notifs = state
            .notifications
            .list_for_user(
                &Id::new("viewer1"),
                Some(&Id::new("ws1")),
                None,
                None,
                None,
                10,
                0,
            )
            .await
            .unwrap();
        assert_eq!(
            viewer_notifs.len(),
            0,
            "Viewer should NOT receive notification"
        );
    }

    #[tokio::test]
    async fn patrol_no_notifications_for_warning_findings() {
        let state = test_state();
        let now = 2_000_000u64;

        // Set up workspace + repo + members.
        let ws = make_workspace("ws1");
        state.workspaces.create(&ws).await.unwrap();

        let repo = make_repo("repo1", "ws1");
        state.repos.create(&repo).await.unwrap();

        let admin = make_membership("admin1", "ws1", WorkspaceRole::Admin);
        state.workspace_memberships.create(&admin).await.unwrap();

        // Only warning-severity finding — no notifications.
        let findings = vec![PatrolFinding {
            finding_type: "stale_link".to_string(),
            severity: "warning".to_string(),
            spec_path: "system/a.md".to_string(),
            detail: "Stale link".to_string(),
            suggested_action: "Review link".to_string(),
        }];

        create_notifications_for_error_findings(&state, &findings, now).await;

        let admin_notifs = state
            .notifications
            .list_for_user(
                &Id::new("admin1"),
                Some(&Id::new("ws1")),
                None,
                None,
                None,
                10,
                0,
            )
            .await
            .unwrap();
        assert_eq!(
            admin_notifs.len(),
            0,
            "warning findings should NOT create notifications"
        );
    }

    // -----------------------------------------------------------------------
    // compute_chain_depth unit tests
    // -----------------------------------------------------------------------

    #[test]
    fn chain_depth_linear() {
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
        adj.insert("a", vec!["b"]);
        adj.insert("b", vec!["c"]);
        adj.insert("c", vec!["d"]);
        assert_eq!(compute_chain_depth("a", &adj), 3);
        assert_eq!(compute_chain_depth("b", &adj), 2);
        assert_eq!(compute_chain_depth("d", &adj), 0);
    }

    #[test]
    fn chain_depth_branching() {
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
        adj.insert("a", vec!["b", "c"]);
        adj.insert("b", vec!["d"]);
        adj.insert("c", vec!["d", "e"]);
        adj.insert("e", vec!["f"]);
        // a→b→d (2), a→c→d (2), a→c→e→f (3) — max is 3
        assert_eq!(compute_chain_depth("a", &adj), 3);
    }

    #[test]
    fn chain_depth_no_edges() {
        let adj: HashMap<&str, Vec<&str>> = HashMap::new();
        assert_eq!(compute_chain_depth("a", &adj), 0);
    }
}
