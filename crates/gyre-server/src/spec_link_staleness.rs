//! Spec link staleness background job (spec-links.md §Automatic Staleness Detection).
//!
//! Runs daily. For each `SpecLinkEntry` in the spec links store:
//! - Checks if the target spec's current SHA in the ledger differs from the link's pinned `target_sha`.
//! - If they differ, marks the link as `"stale"` and sets `stale_since`.
//! - For cross-repo/cross-workspace links (target_display starts with `@`), re-resolves the
//!   workspace/repo — tenant-scoped from the source link (source repo → workspace → tenant),
//!   with the target path required to be declared in the target repo's manifest. Links denied
//!   for read access at push time are never re-resolved.
//! - Creates `CrossWorkspaceSpecChange` notifications for workspace Admin/Developer members.

use std::sync::Arc;

use gyre_common::{Id, Notification, NotificationType};
use gyre_domain::WorkspaceRole;
use tracing::{debug, info, warn};

use crate::AppState;

/// Run one pass of the staleness check across all spec links.
pub async fn run_once(state: &Arc<AppState>) -> anyhow::Result<()> {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Snapshot all links so we don't hold the lock across await points.
    let links: Vec<_> = {
        let store = state.spec_links_store.lock().await;
        store.clone()
    };

    debug!(count = links.len(), "spec_link_staleness: checking links");

    let mut stale_count = 0u32;

    for link in &links {
        // Only check links that have a pinned target_sha and are currently active.
        let Some(pinned_sha) = &link.target_sha else {
            continue;
        };

        if link.status == "stale" || link.status == "broken" {
            // Already known stale/broken — skip.
            continue;
        }

        // Resolve current SHA from the ledger.
        let current_sha = match state.spec_ledger.find_by_path(&link.target_path).await {
            Ok(Some(entry)) => entry.current_sha,
            Ok(None) => {
                // Target spec not in ledger — mark as broken.
                mark_link_status(state, &link.id, "broken", now_secs).await;
                info!(
                    source = %link.source_path,
                    target = %link.target_path,
                    "spec_link_staleness: target not in ledger — marking broken"
                );
                continue;
            }
            Err(e) => {
                warn!(
                    target = %link.target_path,
                    error = %e,
                    "spec_link_staleness: failed to look up target in ledger"
                );
                continue;
            }
        };

        if !current_sha.is_empty() && &current_sha != pinned_sha {
            mark_link_status(state, &link.id, "stale", now_secs).await;
            stale_count += 1;

            info!(
                source = %link.source_path,
                target = %link.target_path,
                link_type = %link.link_type,
                pinned = %pinned_sha,
                current = %current_sha,
                "spec_link_staleness: SHA mismatch — marking stale"
            );
            // Create notifications for workspace members.
            if let Some(source_repo_id) = &link.source_repo_id {
                notify_workspace_members(
                    state,
                    source_repo_id,
                    &link.source_path,
                    &link.target_path,
                    now_secs,
                )
                .await;
            }

            // user-management.md §Who Gets Notified — "Spec drift detected"
            // also notifies the spec owner.
            notify_spec_owner_of_drift(state, &link.source_path, &link.target_path).await;
        }
    }

    // Re-resolve cross-workspace links that are currently unresolved.
    resolve_unresolved_cross_workspace_links(state, &links).await;

    info!(
        stale_count,
        total = links.len(),
        "spec_link_staleness: check complete"
    );

    Ok(())
}

/// Mark a link's status and stale_since in the spec links store and persist
/// the mutation to the durable graph (spec-links.md §Forge-Maintained Spec
/// Graph — the SQL table is authoritative).
async fn mark_link_status(state: &AppState, link_id: &str, status: &str, now: u64) {
    let updated: Option<crate::spec_registry::SpecLinkEntry> = {
        let mut store = state.spec_links_store.lock().await;
        store.iter_mut().find(|l| l.id == link_id).map(|link| {
            link.status = status.to_string();
            if status == "stale" {
                link.stale_since = Some(now);
            }
            link.clone()
        })
    };
    if let Some(entry) = updated {
        if let Err(e) = state.spec_link_repo.save(&entry).await {
            warn!(link_id = %link_id, "spec_link_staleness: failed to persist link status: {e}");
        }
    }
}

/// Re-resolve unresolved cross-repo / cross-workspace links (target_display
/// starts with `@`) — the spec_links table comment: "NULL for unresolved
/// cross-workspace links (resolved later by staleness checker)".
///
/// Scope derivation never fabricates: the tenant comes from the source link
/// (source repo → workspace → tenant), never a literal default tenant id
/// (check-fabricated-scope-defaults invariant). When any hop is
/// unresolvable, the link is skipped with a log line.
///
/// Access-denied links are NOT re-resolved: push-time denial records a
/// durable marker (the linking principal is not recoverable from the link
/// row — the spec's spec_links schema has no principal column), and this
/// pass must not silently flip a denied link "active". The denial stands
/// until the source repo is pushed again by a principal with read access.
///
/// Re-resolution applies the same target-manifest requirement as push-time
/// resolution: a link whose target path is not declared in the target
/// repo's current manifest stays unresolved. A link whose activation
/// would create a cycle in the durable spec link graph also stays
/// unresolved (task-293 round 3: the push-time cycle gate must not be
/// bypassed by activating a pending row into a cycle).
///
/// Ambiguous three-segment displays (`@{a}/{b}/{c}/...`) disambiguate the
/// same way as push-time resolution: the workspace form takes precedence,
/// and when no workspace with that slug exists in the source tenant the
/// first segment is a repo name in the source workspace (the
/// `@{repo_name}/{spec_path}` form with a slashed spec path).
async fn resolve_unresolved_cross_workspace_links(
    state: &AppState,
    links: &[crate::spec_registry::SpecLinkEntry],
) {
    for link in links {
        if link.status != "unresolved" {
            continue;
        }
        let Some(ref target_display) = link.target_display else {
            continue;
        };
        if !target_display.starts_with('@') {
            continue;
        }

        // Access-denied at push time — the denial stands until the source
        // manifest is pushed again by a principal with read access.
        if crate::spec_registry::link_denial(&state.kv_store, &link.id)
            .await
            .is_some()
        {
            debug!(
                link_id = %link.id,
                "spec_link_staleness: link denied for read access at push time — not re-resolving"
            );
            continue;
        }

        // Source scope: source repo → workspace (→ tenant). Skip and log
        // when underivable — never fabricate a scope identity.
        let Some(source_repo_id) = &link.source_repo_id else {
            debug!(
                link_id = %link.id,
                "spec_link_staleness: link has no source repo — cannot derive workspace scope"
            );
            continue;
        };
        let source_repo = match state.repos.find_by_id(&Id::new(source_repo_id)).await {
            Ok(Some(repo)) => repo,
            _ => {
                warn!(
                    link_id = %link.id,
                    source_repo_id,
                    "spec_link_staleness: source repo not found — skipping cross-workspace re-resolution"
                );
                continue;
            }
        };
        let source_ws = match state.workspaces.find_by_id(&source_repo.workspace_id).await {
            Ok(Some(ws)) => ws,
            _ => {
                warn!(
                    link_id = %link.id,
                    source_repo_id,
                    workspace_id = %source_repo.workspace_id,
                    "spec_link_staleness: source workspace not found — skipping cross-workspace re-resolution"
                );
                continue;
            }
        };

        let parsed = crate::spec_registry::parse_cross_workspace_target(target_display);
        let resolved_repo = match &parsed {
            crate::spec_registry::CrossWorkspaceTarget::CrossWorkspace {
                workspace_slug,
                repo_name,
                ..
            } => {
                // Tenant-scoped workspace lookup under the SOURCE link's
                // tenant (source repo → workspace → tenant) — never a
                // fabricated literal tenant id.
                match state
                    .workspaces
                    .find_by_slug(&source_ws.tenant_id, workspace_slug)
                    .await
                {
                    Ok(Some(target_ws)) => state
                        .repos
                        .find_by_name_and_workspace(&target_ws.id, repo_name)
                        .await
                        .ok()
                        .flatten(),
                    Ok(None) => {
                        // No workspace with that slug in the source tenant:
                        // the display is the ambiguous three-segment form
                        // and the target is the same-workspace cross-repo
                        // link (@{repo_name}/{spec_path} with a slashed
                        // path) — the first segment is the repo name,
                        // resolved within the source link's workspace.
                        state
                            .repos
                            .find_by_name_and_workspace(&source_ws.id, workspace_slug)
                            .await
                            .ok()
                            .flatten()
                    }
                    // Lookup failed — cannot disambiguate; leave unresolved.
                    Err(_) => None,
                }
            }
            crate::spec_registry::CrossWorkspaceTarget::CrossRepo { repo_name, .. } => {
                // Same-workspace cross-repo: the source link's repo →
                // workspace gives the scope the repo name resolves within.
                state
                    .repos
                    .find_by_name_and_workspace(&source_ws.id, repo_name)
                    .await
                    .ok()
                    .flatten()
            }
            _ => None,
        };

        let Some(target_repo) = resolved_repo else {
            debug!(
                link_id = %link.id,
                target = %target_display,
                "spec_link_staleness: cross-workspace target still unresolved"
            );
            continue;
        };

        // Same manifest-declaration requirement as push-time resolution:
        // a target path absent from the target repo's manifest stays
        // unresolved (spec-links.md: "Target spec path exists in the target
        // repo's manifest").
        if !crate::spec_registry::target_manifest_declares_path(&target_repo, &link.target_path)
            .await
        {
            debug!(
                link_id = %link.id,
                target = %target_display,
                target_path = %link.target_path,
                "spec_link_staleness: target path not declared in target repo manifest — leaving unresolved"
            );
            continue;
        }

        let repo_id = target_repo.id.to_string();
        // task-293 round 3 (activation gate): refuse to flip a link active
        // when that would create a cycle in the durable spec link graph —
        // the graph the push-time gate protects must not acquire a cycle
        // through unresolved-link activation either. The link stays
        // unresolved with a warning naming the cycle.
        let graph_rows: Vec<_> = state.spec_links_store.lock().await.clone();
        if let Some(cycle) =
            crate::spec_registry::link_activation_creates_cycle(&graph_rows, link, &repo_id)
        {
            warn!(
                link_id = %link.id,
                cycle = %cycle,
                "spec_link_staleness: refusing to activate cross-repo link — activation would create a spec link cycle"
            );
            continue;
        }
        let updated: Option<crate::spec_registry::SpecLinkEntry> = {
            let mut store = state.spec_links_store.lock().await;
            store.iter_mut().find(|l| l.id == link.id).map(|entry| {
                entry.target_repo_id = Some(repo_id);
                entry.status = "active".to_string();
                entry.clone()
            })
        };
        if let Some(entry) = updated {
            if let Err(e) = state.spec_link_repo.save(&entry).await {
                warn!(
                    link_id = %link.id,
                    "spec_link_staleness: failed to persist resolved link: {e}"
                );
            }
            info!(
                link_id = %link.id,
                "spec_link_staleness: cross-repo/cross-workspace link resolved"
            );
        }
    }
}

/// Create `CrossWorkspaceSpecChange` notifications for Admin/Developer members
/// of the workspace that owns the source spec.
async fn notify_workspace_members(
    state: &AppState,
    source_repo_id: &str,
    source_path: &str,
    target_path: &str,
    now_secs: u64,
) {
    // Resolve the workspace that owns the source repo.
    let repo = match state.repos.find_by_id(&Id::new(source_repo_id)).await {
        Ok(Some(r)) => r,
        _ => return,
    };
    let workspace_id = &repo.workspace_id;

    // Resolve tenant_id from workspace.
    let tenant_id = match state.workspaces.find_by_id(workspace_id).await {
        Ok(Some(ws)) => ws.tenant_id.to_string(),
        _ => return,
    };

    let members = match state
        .workspace_memberships
        .list_by_workspace(workspace_id)
        .await
    {
        Ok(m) => m,
        Err(e) => {
            warn!(
                workspace_id = %workspace_id,
                error = %e,
                "spec_link_staleness: failed to list workspace members"
            );
            return;
        }
    };

    let now_i64 = now_secs as i64;

    for member in &members {
        if !matches!(member.role, WorkspaceRole::Admin | WorkspaceRole::Developer) {
            continue;
        }

        let id = Id::new(uuid::Uuid::new_v4().to_string());
        let title = format!(
            "Spec link stale: '{}' → '{}' — target spec has changed",
            source_path, target_path
        );
        let mut notif = Notification::new(
            id,
            workspace_id.clone(),
            member.user_id.clone(),
            NotificationType::CrossWorkspaceSpecChange,
            title,
            &tenant_id,
            now_i64,
        );
        notif.body = Some(
            serde_json::json!({
                "source_path": source_path,
                "target_path": target_path,
                "link_status": "stale",
            })
            .to_string(),
        );
        notif.source.entity_type = "spec".to_string();
        notif.source.entity_id = source_path.to_string();

        if let Err(e) = state.create_notification(&notif).await {
            warn!(
                user = %member.user_id,
                error = %e,
                "spec_link_staleness: failed to create notification"
            );
        } else {
            // Channel fan-out per user-management.md §Delivery Channels.
            crate::notification_dispatcher::dispatch_to_channels(state, &notif).await;
        }
    }
}

/// Notify the drifted spec's owner (user-management.md §Who Gets Notified:
/// "Spec drift detected" → Spec owner). The owner comes from the source
/// spec's ledger entry (`owner`, `user:<name>` form); the workspace scope
/// also comes from the entry. No-op when the spec has no ledger entry or
/// no owner.
async fn notify_spec_owner_of_drift(state: &AppState, source_path: &str, target_path: &str) {
    let Some(entry) = state
        .spec_ledger
        .find_by_path(source_path)
        .await
        .ok()
        .flatten()
    else {
        return;
    };
    let owner = entry.owner.strip_prefix("user:").unwrap_or(&entry.owner);
    if owner.is_empty() {
        return;
    }
    let Some(ws_id) = entry.workspace_id.clone() else {
        return;
    };
    let workspace_id = Id::new(ws_id);
    // Resolve tenant_id from the workspace (skip + log when unresolvable —
    // never fabricate a scope identity, see check-fabricated-scope-defaults).
    let tenant_id = match state.workspaces.find_by_id(&workspace_id).await {
        Ok(Some(ws)) => ws.tenant_id.to_string(),
        _ => {
            warn!(
                spec_path = %source_path,
                workspace_id = %workspace_id,
                "spec_link_staleness: cannot resolve workspace for spec-owner drift notification — skipping"
            );
            return;
        }
    };
    let owner_id = Id::new(owner.to_string());

    let notif_body = serde_json::json!({
        "source_path": source_path,
        "target_path": target_path,
        "link_status": "stale",
        "reason": "spec_drift",
    })
    .to_string();
    // Channel fan-out per user-management.md §Delivery Channels.
    crate::notifications::notify_rich(
        state,
        workspace_id.clone(),
        owner_id,
        NotificationType::CrossWorkspaceSpecChange,
        format!(
            "Spec drift detected: '{}' → '{}' — target spec has changed",
            source_path, target_path
        ),
        tenant_id,
        Some(notif_body),
        Some(source_path.to_string()),
        None,
        "spec",
        None,
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use crate::spec_registry::{SpecLinkEntry, SpecLinkType};
    use gyre_domain::{ApprovalStatus, SpecLedgerEntry, WorkspaceMembership, WorkspaceRole};

    fn make_ledger_entry(path: &str, sha: &str, status: ApprovalStatus) -> SpecLedgerEntry {
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

    fn make_link(
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
            max_agents: None,
            max_agent_iterations: None,
            budget: None,
        }
    }

    #[tokio::test]
    async fn staleness_job_detects_sha_mismatch() {
        let state = test_state();

        // Set up target spec with current SHA different from pinned.
        let target = make_ledger_entry(
            "system/target.md",
            "current_sha_456",
            ApprovalStatus::Approved,
        );
        state.spec_ledger.save(&target).await.unwrap();

        // Set up a link with a pinned SHA that no longer matches.
        let link = make_link(
            "link1",
            "system/source.md",
            "system/target.md",
            SpecLinkType::DependsOn,
            Some("old_sha_123"),
        );
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(link);
        }

        run_once(&state).await.unwrap();

        // Verify the link is now stale.
        let store = state.spec_links_store.lock().await;
        let updated = store.iter().find(|l| l.id == "link1").unwrap();
        assert_eq!(updated.status, "stale");
        assert!(updated.stale_since.is_some());
    }

    #[tokio::test]
    async fn staleness_job_skips_matching_sha() {
        let state = test_state();

        // Target spec SHA matches pinned SHA.
        let target =
            make_ledger_entry("system/target.md", "matching_sha", ApprovalStatus::Approved);
        state.spec_ledger.save(&target).await.unwrap();

        let link = make_link(
            "link2",
            "system/source.md",
            "system/target.md",
            SpecLinkType::DependsOn,
            Some("matching_sha"),
        );
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(link);
        }

        run_once(&state).await.unwrap();

        // Link should still be active.
        let store = state.spec_links_store.lock().await;
        let updated = store.iter().find(|l| l.id == "link2").unwrap();
        assert_eq!(updated.status, "active");
        assert!(updated.stale_since.is_none());
    }

    #[tokio::test]
    async fn staleness_job_marks_missing_target_as_broken() {
        let state = test_state();

        // No target spec in ledger.
        let link = make_link(
            "link3",
            "system/source.md",
            "system/nonexistent.md",
            SpecLinkType::Extends,
            Some("some_sha"),
        );
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(link);
        }

        run_once(&state).await.unwrap();

        let store = state.spec_links_store.lock().await;
        let updated = store.iter().find(|l| l.id == "link3").unwrap();
        assert_eq!(updated.status, "broken");
    }

    #[tokio::test]
    async fn staleness_job_creates_notifications_for_workspace_members() {
        let state = test_state();

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

        // Set up stale link scenario.
        let target = make_ledger_entry("system/target.md", "new_sha", ApprovalStatus::Approved);
        state.spec_ledger.save(&target).await.unwrap();

        let link = make_link(
            "link4",
            "system/source.md",
            "system/target.md",
            SpecLinkType::DependsOn,
            Some("old_sha"),
        );
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(link);
        }

        run_once(&state).await.unwrap();

        // Admin and Developer should get notifications; Viewer should not.
        let admin_notifs = state
            .notifications
            .list_for_user(
                &Id::new("admin1"),
                Some(&Id::new("ws1")),
                None,
                None,
                None,
                None,
                &[],
                10,
                0,
            )
            .await
            .unwrap();
        assert!(
            admin_notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::CrossWorkspaceSpecChange),
            "Admin should receive stale link notification"
        );

        let dev_notifs = state
            .notifications
            .list_for_user(
                &Id::new("dev1"),
                Some(&Id::new("ws1")),
                None,
                None,
                None,
                None,
                &[],
                10,
                0,
            )
            .await
            .unwrap();
        assert!(
            dev_notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::CrossWorkspaceSpecChange),
            "Developer should receive stale link notification"
        );

        let viewer_notifs = state
            .notifications
            .list_for_user(
                &Id::new("viewer1"),
                Some(&Id::new("ws1")),
                None,
                None,
                None,
                None,
                &[],
                10,
                0,
            )
            .await
            .unwrap();
        assert!(
            !viewer_notifs
                .iter()
                .any(|n| n.notification_type == NotificationType::CrossWorkspaceSpecChange),
            "Viewer should NOT receive stale link notification"
        );
    }

    #[tokio::test]
    async fn staleness_job_notifies_spec_owner_of_drift() {
        let state = test_state();

        let ws = make_workspace("ws1");
        state.workspaces.create(&ws).await.unwrap();

        // The SOURCE spec's ledger entry names the owner; its SHA-mismatched
        // target triggers drift.
        let mut source = make_ledger_entry("system/source.md", "src_sha", ApprovalStatus::Approved);
        source.owner = "user:spec-owner".to_string();
        state.spec_ledger.save(&source).await.unwrap();

        let target = make_ledger_entry("system/target.md", "new_sha", ApprovalStatus::Approved);
        state.spec_ledger.save(&target).await.unwrap();

        let link = make_link(
            "link7",
            "system/source.md",
            "system/target.md",
            SpecLinkType::DependsOn,
            Some("old_sha"),
        );
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(link);
        }

        run_once(&state).await.unwrap();

        // The spec owner receives a drift notification even though they hold
        // no workspace membership (user-management.md §Who Gets Notified:
        // "Spec drift detected" → Spec owner).
        let owner_notifs = state
            .notifications
            .list_for_user(
                &Id::new("spec-owner"),
                Some(&Id::new("ws1")),
                None,
                None,
                None,
                None,
                &[],
                10,
                0,
            )
            .await
            .unwrap();
        assert!(
            owner_notifs.iter().any(|n| {
                n.notification_type == NotificationType::CrossWorkspaceSpecChange
                    && n.title.starts_with("Spec drift detected")
            }),
            "spec owner should receive a drift notification, got {:?}",
            owner_notifs.iter().map(|n| &n.title).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn staleness_job_skips_already_stale_links() {
        let state = test_state();

        let target = make_ledger_entry("system/target.md", "new_sha", ApprovalStatus::Approved);
        state.spec_ledger.save(&target).await.unwrap();

        let mut link = make_link(
            "link5",
            "system/source.md",
            "system/target.md",
            SpecLinkType::DependsOn,
            Some("old_sha"),
        );
        link.status = "stale".to_string();
        link.stale_since = Some(999_000);
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(link);
        }

        run_once(&state).await.unwrap();

        // stale_since should remain unchanged (not re-stamped).
        let store = state.spec_links_store.lock().await;
        let updated = store.iter().find(|l| l.id == "link5").unwrap();
        assert_eq!(updated.status, "stale");
        assert_eq!(updated.stale_since, Some(999_000));
    }

    #[tokio::test]
    async fn staleness_job_skips_links_without_target_sha() {
        let state = test_state();

        // Link without a pinned SHA should be skipped entirely.
        let link = make_link(
            "link6",
            "system/source.md",
            "system/target.md",
            SpecLinkType::References,
            None,
        );
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(link);
        }

        run_once(&state).await.unwrap();

        let store = state.spec_links_store.lock().await;
        let updated = store.iter().find(|l| l.id == "link6").unwrap();
        assert_eq!(updated.status, "active");
    }

    // -----------------------------------------------------------------------
    // task-287: re-resolution of unresolved cross-repo / cross-workspace
    // links (spec-links.md §Cross-Repo and Cross-Workspace Links)
    // -----------------------------------------------------------------------

    fn make_workspace_in_tenant(id: &str, tenant: &str, slug: &str) -> gyre_domain::Workspace {
        gyre_domain::Workspace {
            id: Id::new(id),
            tenant_id: Id::new(tenant),
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

    fn make_named_repo(id: &str, name: &str, ws_id: &str, path: &str) -> gyre_domain::Repository {
        gyre_domain::Repository {
            id: Id::new(id),
            name: name.to_string(),
            path: path.to_string(),
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
            max_agents: None,
            max_agent_iterations: None,
            budget: None,
        }
    }

    /// Real git repo with a manifest declaring system/auth.md (the target
    /// repo whose manifest the re-resolution must consult).
    fn make_target_git_repo() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
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
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.email", "test@test.com"]);
        run(&["config", "user.name", "Test"]);
        let manifest =
            "version: 1\nspecs:\n  - path: system/auth.md\n    title: Auth\n    owner: user:test\n";
        for (file, content) in [
            ("specs/manifest.yaml", manifest),
            ("specs/system/auth.md", "# Auth\n"),
        ] {
            let full = path.join(file);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(&full, content).unwrap();
            run(&["add", file]);
        }
        run(&["commit", "-m", "initial"]);
        dir
    }

    fn make_unresolved_link(
        id: &str,
        source_repo: &str,
        target_path: &str,
        target_display: &str,
    ) -> SpecLinkEntry {
        let mut link = make_link(
            id,
            "system/a.md",
            target_path,
            SpecLinkType::DependsOn,
            None,
        );
        link.source_repo_id = Some(source_repo.to_string());
        link.target_display = Some(target_display.to_string());
        link.status = "unresolved".to_string();
        link
    }

    /// AC 4: in a tenant whose id is not "default", an unresolved
    /// cross-workspace link whose target workspace exists in that tenant
    /// re-resolves on the next staleness pass.
    ///
    /// Against the pre-task code this fails: re-resolution looked the
    /// workspace slug up under the fabricated literal tenant id "default",
    /// so a workspace in any other tenant never resolved.
    #[tokio::test]
    async fn staleness_pass_resolves_cross_workspace_link_under_source_tenant() {
        let state = test_state();

        // tenant-287 ≠ literal "default": both workspaces live in it.
        let ws_x = make_workspace_in_tenant("ws-x", "tenant-287", "ws-x");
        let ws_y = make_workspace_in_tenant("ws-y", "tenant-287", "ws-y");
        state.workspaces.create(&ws_x).await.unwrap();
        state.workspaces.create(&ws_y).await.unwrap();

        let state_repo_a = make_named_repo("repo-a", "A", "ws-x", "/repos/repo-a");
        state.repos.create(&state_repo_a).await.unwrap();

        // Target repo in ws-y really declares the target path.
        let dir_y = make_target_git_repo();
        let repo_y = make_named_repo("repo-y", "repo-y", "ws-y", dir_y.path().to_str().unwrap());
        state.repos.create(&repo_y).await.unwrap();

        // Unresolved cross-workspace link from repo-a (in ws-x) to
        // @ws-y/repo-y/system/auth.md.
        let link = make_unresolved_link(
            "link-cw",
            "repo-a",
            "system/auth.md",
            "@ws-y/repo-y/system/auth.md",
        );
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(link);
        }

        run_once(&state).await.unwrap();

        let store = state.spec_links_store.lock().await;
        let updated = store.iter().find(|l| l.id == "link-cw").unwrap();
        assert_eq!(
            updated.status, "active",
            "cross-workspace link must re-resolve under the source link's tenant, got {:?}",
            updated
        );
        assert_eq!(updated.target_repo_id.as_deref(), Some("repo-y"));

        // The resolution is written through to the durable graph.
        let persisted = state.spec_link_repo.list_all().await.unwrap();
        let row = persisted.iter().find(|l| l.id == "link-cw").unwrap();
        assert_eq!(row.status, "active");
        assert_eq!(row.target_repo_id.as_deref(), Some("repo-y"));
    }

    /// Requirement 1 (staleness half): unresolved same-workspace cross-repo
    /// links re-resolve via the source link's repo → workspace. A link
    /// whose target path is not declared in the target repo's manifest
    /// stays unresolved.
    ///
    /// Against the pre-task code this fails: the staleness checker
    /// explicitly refused to re-resolve CrossRepo links.
    #[tokio::test]
    async fn staleness_pass_resolves_cross_repo_same_workspace_link() {
        let state = test_state();

        let ws_x = make_workspace_in_tenant("ws-x", "tenant-287", "ws-x");
        state.workspaces.create(&ws_x).await.unwrap();

        let state_repo_a = make_named_repo("repo-a", "A", "ws-x", "/repos/repo-a");
        state.repos.create(&state_repo_a).await.unwrap();
        let dir_y = make_target_git_repo();
        let repo_y = make_named_repo("repo-y", "repo-y", "ws-x", dir_y.path().to_str().unwrap());
        state.repos.create(&repo_y).await.unwrap();

        // One resolvable link (path declared in repo-y's manifest) and one
        // whose target path repo-y does not declare.
        let resolvable = make_unresolved_link(
            "link-cr-ok",
            "repo-a",
            "system/auth.md",
            "@repo-y/system/auth.md",
        );
        let missing_path = make_unresolved_link(
            "link-cr-missing",
            "repo-a",
            "system/missing.md",
            "@repo-y/system/missing.md",
        );
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(resolvable);
            store.push(missing_path);
        }

        run_once(&state).await.unwrap();

        let store = state.spec_links_store.lock().await;
        let ok = store.iter().find(|l| l.id == "link-cr-ok").unwrap();
        assert_eq!(
            ok.status, "active",
            "same-workspace cross-repo link must re-resolve via source repo → workspace, got {:?}",
            ok
        );
        assert_eq!(ok.target_repo_id.as_deref(), Some("repo-y"));

        let missing = store.iter().find(|l| l.id == "link-cr-missing").unwrap();
        assert_eq!(
            missing.status, "unresolved",
            "a target path absent from the target repo's manifest must stay unresolved"
        );
        assert_eq!(missing.target_repo_id, None);
    }

    /// AC 2 (staleness half): a link denied for read access at push time is
    /// never flipped "active" by the staleness pass — the durable denial
    /// marker blocks re-resolution until the source manifest is pushed
    /// again by a principal with read access.
    #[tokio::test]
    async fn staleness_does_not_flip_access_denied_link_active() {
        let state = test_state();

        let ws_x = make_workspace_in_tenant("ws-x", "tenant-287", "ws-x");
        let ws_y = make_workspace_in_tenant("ws-y", "tenant-287", "ws-y");
        state.workspaces.create(&ws_x).await.unwrap();
        state.workspaces.create(&ws_y).await.unwrap();

        let state_repo_a = make_named_repo("repo-a", "A", "ws-x", "/repos/repo-a");
        state.repos.create(&state_repo_a).await.unwrap();
        let dir_y = make_target_git_repo();
        let repo_y = make_named_repo("repo-y", "repo-y", "ws-y", dir_y.path().to_str().unwrap());
        state.repos.create(&repo_y).await.unwrap();

        let link = make_unresolved_link(
            "link-denied",
            "repo-a",
            "system/auth.md",
            "@ws-y/repo-y/system/auth.md",
        );
        let link_id = link.id.clone();
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(link);
        }

        // Push-time denial marker, recorded via the production helper.
        let principal = crate::spec_registry::LinkPrincipal {
            agent_id: "user-one".to_string(),
            user_id: Some(Id::new("u-1")),
            workspace_id: Some("ws-x".to_string()),
            is_system: false,
        };
        crate::spec_registry::record_link_denial(
            &state.kv_store,
            &link_id,
            Some(&principal),
            "@ws-y/repo-y/system/auth.md",
            1_700_000_000,
        )
        .await;

        run_once(&state).await.unwrap();

        // Everything the re-resolution needs exists (tenant-scoped workspace,
        // repo, declared path) — the denial must still hold.
        let store = state.spec_links_store.lock().await;
        let updated = store.iter().find(|l| l.id == "link-denied").unwrap();
        assert_eq!(
            updated.status, "unresolved",
            "the staleness pass must not flip an access-denied link active, got {:?}",
            updated
        );
        assert_eq!(updated.target_repo_id, None);
    }

    /// task-293 round 3 (activation gate): a pending cross-repo row whose
    /// activation would create a cycle in the durable spec link graph
    /// (the stored active edge A→B exists, the pending row is B→A) must
    /// stay unresolved — the staleness pass must not land a cycle the
    /// push-time gate would have rejected. Against the pre-gate candidate
    /// this fails: the row flips active and the A→B→A cycle lives
    /// permanently in the forge graph. The companion tests above
    /// (`staleness_pass_resolves_cross_repo_same_workspace_link` etc.)
    /// prove activation still works when no cycle would be created — the
    /// gate was added, not made indiscriminate.
    #[tokio::test]
    async fn staleness_refuses_activation_that_creates_cycle() {
        let state = test_state();

        let ws_x = make_workspace_in_tenant("ws-x", "tenant-287", "ws-x");
        state.workspaces.create(&ws_x).await.unwrap();

        // Source repo of the pending row (repo-b): its repo → workspace
        // hop scopes the @A/... display's resolution.
        let repo_b = make_named_repo("repo-b", "B", "ws-x", "/repos/repo-b");
        state.repos.create(&repo_b).await.unwrap();
        // Target repo A really declares the target path in its manifest.
        let dir_a = make_target_git_repo();
        let repo_a = make_named_repo("repo-a", "A", "ws-x", dir_a.path().to_str().unwrap());
        state.repos.create(&repo_a).await.unwrap();

        // Stored ACTIVE edge A→B (attributed): activating the pending B→A
        // row below would close the cycle A→B→A.
        let mut back_edge = make_link(
            "link-ab",
            "system/auth.md",
            "system/b.md",
            SpecLinkType::DependsOn,
            None,
        );
        back_edge.source_repo_id = Some("repo-a".to_string());
        back_edge.target_repo_id = Some("repo-b".to_string());
        back_edge.target_display = Some("@B/system/b.md".to_string());
        // Pending unresolved row B→A: everything the activation needs
        // exists (repo A found by name in ws-x, path declared in repo A's
        // manifest), so without the gate this row flips active.
        let mut pending = make_link(
            "link-ba",
            "system/b.md",
            "system/auth.md",
            SpecLinkType::DependsOn,
            None,
        );
        pending.source_repo_id = Some("repo-b".to_string());
        pending.target_display = Some("@A/system/auth.md".to_string());
        pending.status = "unresolved".to_string();
        {
            let mut store = state.spec_links_store.lock().await;
            store.push(back_edge);
            store.push(pending);
        }

        run_once(&state).await.unwrap();

        let store = state.spec_links_store.lock().await;
        let updated = store.iter().find(|l| l.id == "link-ba").unwrap();
        assert_eq!(
            updated.status, "unresolved",
            "activation that would create the A→B→A cycle must be refused, got {:?}",
            updated
        );
        assert_eq!(updated.target_repo_id, None);
    }
}
