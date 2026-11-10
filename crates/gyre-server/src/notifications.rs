//! Helper functions for creating notifications (HSI §2, user-management.md
//! §Notification Entity).

use crate::AppState;
use gyre_common::{Id, Notification, NotificationType};

/// Resolve a deep-link URL for a notification source
/// (user-management.md §Notification Entity: "Deep link to relevant page").
///
/// Entity pages follow the SPA routes (ui-navigation.md):
///   mr/gate  → /workspaces/{slug}/r/{repo}/mrs/{id}
///   agent    → /workspaces/{slug}/r/{repo}/agents/{id}
///   spec     → /workspaces/{slug}/r/{repo}/specs/{path}
/// Sources without a repo-scoped entity page (budget, workspace, system)
/// deep-link to the workspace home.
///
/// Best-effort: when the workspace or repo cannot be resolved, falls back to
/// the workspace home (or None when even the workspace is unknown) rather
/// than fabricating a link.
async fn deep_link(
    state: &AppState,
    entity_type: &str,
    entity_id: &str,
    workspace_id: &Id,
    repo_id: Option<&str>,
) -> Option<String> {
    let slug = match state.workspaces.find_by_id(workspace_id).await {
        Ok(Some(ws)) => ws.slug,
        _ => return None,
    };
    let repo_name = match repo_id {
        Some(rid) => match state.repos.find_by_id(&Id::new(rid.to_string())).await {
            Ok(Some(repo)) => Some(repo.name),
            _ => None,
        },
        None => None,
    };
    let base = format!("/workspaces/{}", urlencode(&slug));
    let Some(repo_name) = repo_name else {
        return Some(base);
    };
    let base = format!("{}/r/{}", base, urlencode(&repo_name));
    Some(match entity_type {
        "mr" | "gate" => format!("{}/mrs/{}", base, urlencode(entity_id)),
        "agent" => format!("{}/agents/{}", base, urlencode(entity_id)),
        "spec" => {
            // Spec paths contain '/' separators that are part of the path.
            format!("{}/specs/{}", base, urlencode(entity_id))
        }
        _ => base,
    })
}

/// Minimal percent-encoding for path segments (keeps '/' out of single
/// segments; the SPA decodes each segment on parse).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Create and persist a notification for a specific user.
/// Best-effort: logs errors but never panics (fire-and-forget for non-priority-1 paths).
pub async fn notify(
    state: &AppState,
    workspace_id: Id,
    user_id: Id,
    notification_type: NotificationType,
    title: impl Into<String>,
    tenant_id: impl Into<String>,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let id = Id::new(uuid::Uuid::new_v4().to_string());
    let notif = Notification::new(
        id,
        workspace_id,
        user_id,
        notification_type,
        title,
        tenant_id,
        now,
    );

    if let Err(e) = state.notifications.create(&notif).await {
        tracing::warn!("Failed to create notification: {e}");
    }
}

/// Create and persist a notification with structured body, source, and deep link
/// (user-management.md §Notification Entity).
///
/// - `body` is a JSON string that the UI parses for quick-link buttons
///   (agent_id, mr_id, spec_path).
/// - `entity_type` names the source entity kind ("agent", "mr", "spec",
///   "gate", "budget", "workspace", "system"); `entity_ref` is the source
///   entity id; `repo_id` scopes the source to a repo.
/// - `action_url` is the deep link. When None, one is derived from the
///   source entity (see [`deep_link`]).
#[allow(clippy::too_many_arguments)]
pub async fn notify_rich(
    state: &AppState,
    workspace_id: Id,
    user_id: Id,
    notification_type: NotificationType,
    title: impl Into<String>,
    tenant_id: impl Into<String>,
    body: Option<String>,
    entity_ref: Option<String>,
    repo_id: Option<String>,
    entity_type: &str,
    action_url: Option<String>,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let id = Id::new(uuid::Uuid::new_v4().to_string());
    let mut notif = Notification::new(
        id,
        workspace_id.clone(),
        user_id,
        notification_type,
        title,
        tenant_id,
        now,
    );
    notif.body = body;
    notif.source.entity_type = entity_type.to_string();
    notif.source.entity_id = entity_ref
        .clone()
        .unwrap_or_else(|| workspace_id.as_str().to_string());
    notif.source.repo_id = repo_id.clone().map(Id::new);

    notif.action_url = match action_url {
        Some(url) => Some(url),
        None => {
            deep_link(
                state,
                entity_type,
                notif.source.entity_id.as_str(),
                &workspace_id,
                repo_id.as_deref(),
            )
            .await
        }
    };

    if let Err(e) = state.notifications.create(&notif).await {
        tracing::warn!("Failed to create notification: {e}");
    }
}

/// Notify the spawning user that a gate failed on their MR.
///
/// GateFailure is High priority — created synchronously by the gate
/// evaluation handler per HSI §2.
pub async fn notify_gate_failure(
    state: &AppState,
    author_agent_id: &Id,
    workspace_id: &Id,
    mr_id: &str,
    gate_name: &str,
    tenant_id: &str,
) {
    let spawned_by = state
        .agents
        .find_by_id(author_agent_id)
        .await
        .ok()
        .flatten()
        .and_then(|a| a.spawned_by);

    let user_id = if let Some(ref sb) = spawned_by {
        Id::new(sb.clone())
    } else {
        author_agent_id.clone()
    };

    // Resolve MR title and repo for a human-friendly notification + deep link.
    let mr = state
        .merge_requests
        .find_by_id(&Id::new(mr_id))
        .await
        .ok()
        .flatten();
    let mr_label = mr
        .as_ref()
        .map(|mr| format!("'{}'", mr.title))
        .unwrap_or_else(|| mr_id[..8.min(mr_id.len())].to_string());
    let repo_id = mr.as_ref().map(|mr| mr.repository_id.to_string());

    let body_json = serde_json::json!({
        "gate_name": gate_name,
        "mr_id": mr_id,
        "agent_id": author_agent_id.as_str(),
    })
    .to_string();

    notify_rich(
        state,
        workspace_id.clone(),
        user_id,
        NotificationType::GateFailure,
        format!("Gate '{gate_name}' failed on MR {mr_label}"),
        tenant_id,
        Some(body_json),
        Some(mr_id.to_string()),
        repo_id,
        "gate",
        None,
    )
    .await;
}

/// Notify the spawning user that all gates passed on their MR
/// (user-management.md §Notification Entity: GatePassed, Low — informational).
///
/// De-duplicated per (user, MR): callers fire this from gate completion, so
/// without the guard the last-finishing gates would repeat the announcement.
pub async fn notify_gate_passed(
    state: &AppState,
    author_agent_id: &Id,
    workspace_id: &Id,
    mr_id: &str,
    tenant_id: &str,
) {
    let spawned_by = state
        .agents
        .find_by_id(author_agent_id)
        .await
        .ok()
        .flatten()
        .and_then(|a| a.spawned_by);

    let user_id = if let Some(ref sb) = spawned_by {
        Id::new(sb.clone())
    } else {
        author_agent_id.clone()
    };

    // De-duplicate: one GatePassed per (user, MR) while it stays active.
    if state
        .notifications
        .has_active_notification(
            workspace_id,
            &user_id,
            NotificationType::GatePassed.as_str(),
            mr_id,
            0,
        )
        .await
        .unwrap_or(false)
    {
        return;
    }

    let mr = state
        .merge_requests
        .find_by_id(&Id::new(mr_id))
        .await
        .ok()
        .flatten();
    let mr_label = mr
        .as_ref()
        .map(|mr| format!("'{}'", mr.title))
        .unwrap_or_else(|| mr_id[..8.min(mr_id.len())].to_string());
    let repo_id = mr.as_ref().map(|mr| mr.repository_id.to_string());

    let body_json = serde_json::json!({
        "mr_id": mr_id,
        "agent_id": author_agent_id.as_str(),
    })
    .to_string();

    notify_rich(
        state,
        workspace_id.clone(),
        user_id,
        NotificationType::GatePassed,
        format!("All gates passed on MR {mr_label}"),
        tenant_id,
        Some(body_json),
        Some(mr_id.to_string()),
        repo_id,
        "gate",
        None,
    )
    .await;
}

/// Notify the spawning user that their MR was merged
/// (user-management.md §Notification Entity: MrMerged, Low — informational).
pub async fn notify_mr_merged(
    state: &AppState,
    author_agent_id: &Id,
    workspace_id: &Id,
    mr_id: &str,
    tenant_id: &str,
) {
    let spawned_by = state
        .agents
        .find_by_id(author_agent_id)
        .await
        .ok()
        .flatten()
        .and_then(|a| a.spawned_by);

    let user_id = if let Some(ref sb) = spawned_by {
        Id::new(sb.clone())
    } else {
        return; // No human to notify
    };

    // Resolve MR title and repo for a human-friendly notification + deep link.
    let mr = state
        .merge_requests
        .find_by_id(&Id::new(mr_id))
        .await
        .ok()
        .flatten();
    let mr_label = mr
        .as_ref()
        .map(|mr| format!("'{}'", mr.title))
        .unwrap_or_else(|| mr_id[..8.min(mr_id.len())].to_string());
    let repo_id = mr.as_ref().map(|mr| mr.repository_id.to_string());

    let body_json = serde_json::json!({
        "mr_id": mr_id,
        "agent_id": author_agent_id.as_str(),
    })
    .to_string();

    notify_rich(
        state,
        workspace_id.clone(),
        user_id,
        NotificationType::MrMerged,
        format!("MR {mr_label} was merged"),
        tenant_id,
        Some(body_json),
        Some(mr_id.to_string()),
        repo_id,
        "mr",
        None,
    )
    .await;
}

/// Notify the MR author's spawning user that their merged MR was reverted by
/// the post-merge recovery protocol (platform-model.md §6).
pub async fn notify_mr_reverted(
    state: &AppState,
    author_agent_id: &Id,
    workspace_id: &Id,
    mr_id: &str,
    reason: &str,
) {
    let spawned_by = state
        .agents
        .find_by_id(author_agent_id)
        .await
        .ok()
        .flatten()
        .and_then(|a| a.spawned_by);

    let user_id = if let Some(sb) = &spawned_by {
        Id::new(sb.clone())
    } else {
        return; // No human to notify
    };

    let mr = state
        .merge_requests
        .find_by_id(&Id::new(mr_id))
        .await
        .ok()
        .flatten();
    let mr_label = mr
        .as_ref()
        .map(|mr| format!("'{}'", mr.title))
        .unwrap_or_else(|| mr_id[..8.min(mr_id.len())].to_string());
    let repo_id = mr.as_ref().map(|mr| mr.repository_id.to_string());

    let body_json = serde_json::json!({
        "mr_id": mr_id,
        "agent_id": author_agent_id.as_str(),
        "reason": reason,
    })
    .to_string();

    notify_rich(
        state,
        workspace_id.clone(),
        user_id,
        NotificationType::MrReverted,
        format!("MR {mr_label} was reverted: {reason}"),
        "default",
        Some(body_json),
        Some(mr_id.to_string()),
        repo_id,
        "mr",
        None,
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use gyre_domain::{
        Agent, MergeRequest, Repository, Workspace, WorkspaceMembership, WorkspaceRole,
    };

    /// Shared fixture: workspace (slug "ws-one"), repo ("myrepo"), an agent
    /// spawned by "user-n1", and an MR authored by that agent.
    async fn setup() -> (std::sync::Arc<crate::AppState>, Id) {
        let state = test_state();
        let ws = Workspace::new(Id::new("ws-n1"), Id::new("tenant-1"), "WS One", "ws-one", 0);
        state.workspaces.create(&ws).await.unwrap();
        let repo = Repository::new(
            Id::new("repo-n1"),
            Id::new("ws-n1"),
            "myrepo",
            "/tmp/myrepo.git",
            0,
        );
        state.repos.create(&repo).await.unwrap();
        let mut agent = Agent::new(Id::new("agent-n1"), "worker", 0);
        agent.spawned_by = Some("user-n1".to_string());
        agent.workspace_id = Id::new("ws-n1");
        state.agents.create(&agent).await.unwrap();
        let mut mr = MergeRequest::new(
            Id::new("mr-n1"),
            Id::new("repo-n1"),
            Id::new("ws-n1"),
            "Add feature",
            "feat/x",
            "main",
            0,
        );
        mr.author_agent_id = Some(Id::new("agent-n1"));
        state.merge_requests.create(&mr).await.unwrap();
        (state, Id::new("ws-n1"))
    }

    async fn notifs_for(state: &crate::AppState, user: &str) -> Vec<gyre_common::Notification> {
        state
            .notifications
            .list_for_user(&Id::new(user), None, None, None, None, 50, 0)
            .await
            .unwrap()
    }

    /// user-management.md §Notification Entity: MrMerged is its own taxonomy
    /// type (not the SuggestedSpecLink stand-in this call path used before),
    /// Low priority, and carries a deep link to the merged MR.
    #[tokio::test]
    async fn mr_merged_notification_is_typed_low_priority_with_deep_link() {
        let (state, ws) = setup().await;
        notify_mr_merged(&state, &Id::new("agent-n1"), &ws, "mr-n1", "tenant-1").await;

        let notifs = notifs_for(&state, "user-n1").await;
        assert_eq!(notifs.len(), 1, "spawner gets exactly one notification");
        let n = &notifs[0];
        assert_eq!(n.notification_type, NotificationType::MrMerged);
        assert_eq!(n.priority, gyre_common::NotificationPriority::Low);
        assert_eq!(n.priority_rank, 9);
        assert_eq!(n.source.entity_type, "mr");
        assert_eq!(n.source.entity_id, "mr-n1");
        assert_eq!(
            n.source.repo_id.as_ref().map(|r| r.as_str()),
            Some("repo-n1")
        );
        // Deep link resolved through the workspace slug + repo name, matching
        // the SPA's MR route (/workspaces/:slug/r/:repo/mrs/:id).
        assert_eq!(
            n.action_url.as_deref(),
            Some("/workspaces/ws-one/r/myrepo/mrs/mr-n1")
        );
    }

    /// GatePassed fires once per (user, MR) even when the helper is invoked
    /// repeatedly (gate completion is concurrent; the announcement must not
    /// spam), and is Low priority with an MR deep link.
    #[tokio::test]
    async fn gate_passed_notification_deduplicated_per_user_and_mr() {
        let (state, ws) = setup().await;
        notify_gate_passed(&state, &Id::new("agent-n1"), &ws, "mr-n1", "tenant-1").await;
        notify_gate_passed(&state, &Id::new("agent-n1"), &ws, "mr-n1", "tenant-1").await;
        notify_gate_passed(&state, &Id::new("agent-n1"), &ws, "mr-n1", "tenant-1").await;

        let notifs = notifs_for(&state, "user-n1").await;
        assert_eq!(
            notifs.len(),
            1,
            "repeated gate-passed events must not duplicate the announcement"
        );
        let n = &notifs[0];
        assert_eq!(n.notification_type, NotificationType::GatePassed);
        assert_eq!(n.priority, gyre_common::NotificationPriority::Low);
        assert_eq!(
            n.action_url.as_deref(),
            Some("/workspaces/ws-one/r/myrepo/mrs/mr-n1")
        );
    }

    /// GateFailure keeps its High band and now also carries a deep link.
    #[tokio::test]
    async fn gate_failure_notification_carries_deep_link() {
        let (state, ws) = setup().await;
        notify_gate_failure(
            &state,
            &Id::new("agent-n1"),
            &ws,
            "mr-n1",
            "tests",
            "tenant-1",
        )
        .await;

        let notifs = notifs_for(&state, "user-n1").await;
        assert_eq!(notifs.len(), 1);
        let n = &notifs[0];
        assert_eq!(n.notification_type, NotificationType::GateFailure);
        assert_eq!(n.priority, gyre_common::NotificationPriority::High);
        assert_eq!(
            n.action_url.as_deref(),
            Some("/workspaces/ws-one/r/myrepo/mrs/mr-n1")
        );
    }

    /// notify_rich deep-links to the workspace home when the source has no
    /// repo-scoped entity page, and returns no link when the workspace
    /// cannot be resolved (never fabricates one).
    #[tokio::test]
    async fn notify_rich_falls_back_to_workspace_home_link() {
        let (state, ws) = setup().await;
        notify_rich(
            &state,
            ws.clone(),
            Id::new("user-n1"),
            NotificationType::SystemAlert,
            "Alert",
            "tenant-1",
            None,
            None,
            None,
            "system",
            None,
        )
        .await;
        let notifs = notifs_for(&state, "user-n1").await;
        assert_eq!(notifs.len(), 1);
        assert_eq!(
            notifs[0].action_url.as_deref(),
            Some("/workspaces/ws-one"),
            "repo-less sources deep-link to the workspace home"
        );

        // Unknown workspace → no fabricated link.
        notify_rich(
            &state,
            Id::new("ws-does-not-exist"),
            Id::new("user-n1"),
            NotificationType::SystemAlert,
            "Alert",
            "tenant-1",
            None,
            None,
            None,
            "system",
            None,
        )
        .await;
        let notifs = notifs_for(&state, "user-n1").await;
        assert_eq!(notifs.len(), 2);
        assert!(
            notifs[1].action_url.is_none(),
            "unresolvable workspace must not fabricate a link"
        );
    }

    /// Agent-deep-link sources resolve to the agent page when the agent is
    /// repo-bound (repo passed as source scope).
    #[tokio::test]
    async fn agent_source_deep_links_to_agent_page() {
        let (state, ws) = setup().await;
        notify_rich(
            &state,
            ws.clone(),
            Id::new("user-n1"),
            NotificationType::AgentFailed,
            "Agent failed",
            "tenant-1",
            None,
            Some("agent-n1".to_string()),
            Some("repo-n1".to_string()),
            "agent",
            None,
        )
        .await;
        let notifs = notifs_for(&state, "user-n1").await;
        assert_eq!(notifs.len(), 1);
        assert_eq!(
            notifs[0].action_url.as_deref(),
            Some("/workspaces/ws-one/r/myrepo/agents/agent-n1")
        );
    }

    /// Membership helpers used by pause/persona tests share this fixture
    /// shape; keep a direct check that workspace members are addressable.
    #[tokio::test]
    async fn membership_fixture_is_addressable() {
        let (state, ws) = setup().await;
        let m = WorkspaceMembership::new(
            Id::new("mem-n1"),
            Id::new("user-n1"),
            ws.clone(),
            WorkspaceRole::Admin,
            Id::new("inviter"),
            0,
        );
        state.workspace_memberships.create(&m).await.unwrap();
        let members = state
            .workspace_memberships
            .list_by_workspace(&ws)
            .await
            .unwrap();
        assert_eq!(members.len(), 1);
        assert!(matches!(members[0].role, WorkspaceRole::Admin));
    }
}
