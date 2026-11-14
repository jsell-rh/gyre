//! Notification delivery channels & routing (user-management.md §Delivery Channels,
//! §Who Gets Notified, §Notification Routing for Agent Escalations).
//!
//! The dispatcher fans a persisted in-app notification out to the recipient's
//! configured channels (email / webhook / slack), applying per-channel
//! `min_priority` thresholds. The routing engine resolves the recipient list
//! for an event per the spec's "Who Gets Notified" table, including the
//! agent-escalation online-status fan-out.

use crate::AppState;
use async_trait::async_trait;
use gyre_common::{Id, Notification, NotificationType};
use gyre_domain::{NotificationChannels, NotificationPriority, WorkspaceRole};
use std::collections::HashSet;

/// KV namespace holding the email outbox (queued, digest-scheduled sends).
pub const EMAIL_OUTBOX_NS: &str = "email_outbox";

/// Latency bound for every outbound channel HTTP call (reqwest has no default
/// total timeout — see scripts/check-unbounded-external-http.sh).
const CHANNEL_HTTP_TIMEOUT_SECS: u64 = 10;

/// Abstracts the outbound HTTP POST so tests can capture deliveries.
#[async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
pub trait HttpSender: Send + Sync {
    async fn post_json(
        &self,
        url: &str,
        body: &str,
        headers: &[(&str, &str)],
    ) -> anyhow::Result<()>;
}

/// Production HTTP sender backed by reqwest, latency-bounded.
pub struct ReqwestSender {
    client: reqwest::Client,
}

impl Default for ReqwestSender {
    fn default() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(CHANNEL_HTTP_TIMEOUT_SECS))
                .build()
                .expect("reqwest client build"),
        }
    }
}

#[async_trait]
impl HttpSender for ReqwestSender {
    async fn post_json(
        &self,
        url: &str,
        body: &str,
        headers: &[(&str, &str)],
    ) -> anyhow::Result<()> {
        let mut req = self
            .client
            .post(url)
            .header("Content-Type", "application/json");
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = req.body(body.to_string()).send().await?;
        let status = resp.status();
        if !status.is_success() {
            anyhow::bail!("webhook POST {url} returned {status}");
        }
        Ok(())
    }
}

/// HMAC-SHA256 signature over the payload body, hex-encoded.
fn sign_payload(secret: &str, body: &str) -> String {
    use ring::hmac::{Key, HMAC_SHA256};
    let tag = ring::hmac::sign(&Key::new(HMAC_SHA256, secret.as_bytes()), body.as_bytes());
    hex::encode(tag.as_ref())
}

/// Resolve the user's effective channel configuration (single source of
/// truth: `user_channel_prefs`). When the user has no dedicated row —
/// e.g. preferences stored via `PUT /users/me` before the dedicated table
/// existed — fall back to the `NotificationChannels` embedded in the user's
/// stored `users.preferences` so no API-visible config is silently ignored
/// (single effective config; review bf1e11be F1).
pub async fn resolve_channels(state: &AppState, user_id: &Id) -> NotificationChannels {
    match state.user_channel_prefs.find(user_id).await {
        Ok(Some(c)) => c,
        Ok(None) => match state.users.find_by_id(user_id).await {
            Ok(Some(user)) => user.preferences.notification_channels,
            _ => NotificationChannels::default(),
        },
        Err(e) => {
            tracing::warn!(
                user_id = %user_id,
                error = %e,
                "dispatcher: failed to load channel preferences; in-app only"
            );
            NotificationChannels::default()
        }
    }
}

/// Routes a persisted in-app notification to the recipient's configured
/// channels (§Delivery Channels). Best-effort per channel: a channel failure
/// is logged, never propagated — in-app delivery already succeeded.
pub async fn dispatch_to_channels(state: &AppState, notif: &Notification) {
    let channels = resolve_channels(state, &notif.user_id).await;
    dispatch_with(state, notif, &channels, &ReqwestSender::default()).await;
}

/// Testable core of `dispatch_to_channels`.
pub async fn dispatch_with<S: HttpSender>(
    state: &AppState,
    notif: &Notification,
    channels: &NotificationChannels,
    sender: &S,
) {
    // In-app is always delivered (in_app can't be disabled) — the caller has
    // already persisted the notification, which IS in-app delivery.

    // Band derives from the notification type (user-management.md
    // §Notification Entity membership), never from the HSI §8 numeric
    // priority — see NotificationPriority::for_notification_type.
    let priority = NotificationPriority::for_notification_type(&notif.notification_type);

    // Email channel. A per-type unsubscribe (§Email Notifications) drops
    // the email even when the global channel config would deliver it.
    if channels.email.enabled
        && channels.email.digest != gyre_domain::DigestFrequency::Off
        && priority >= channels.email.min_priority
        && channels
            .email
            .type_overrides
            .get(notif.notification_type.as_str())
            != Some(&true)
    {
        queue_email(state, notif).await;
    }

    // Webhook channel (HMAC-SHA256 signed).
    if let Some(hook) = &channels.webhook {
        if priority >= hook.min_priority {
            let body = serde_json::json!({
                "id": notif.id.to_string(),
                "workspace_id": notif.workspace_id.to_string(),
                "user_id": notif.user_id.to_string(),
                "notification_type": notif.notification_type.as_str(),
                "priority": notif.priority,
                "title": notif.title,
                "body": notif.body,
                "entity_type": notif.source.entity_type,
                "entity_id": notif.source.entity_id,
                "repo_id": notif.source.repo_id,
                "action_url": notif.action_url,
                "created_at": notif.created_at,
            })
            .to_string();
            let signature = sign_payload(&hook.secret, &body);
            if let Err(e) = sender
                .post_json(&hook.url, &body, &[("X-Gyre-Signature", &signature)])
                .await
            {
                tracing::warn!(
                    user_id = %notif.user_id,
                    url = %hook.url,
                    error = %e,
                    "dispatcher: webhook delivery failed"
                );
            }
        }
    }

    // Slack channel.
    if let Some(slack) = &channels.slack {
        if priority >= slack.min_priority {
            let mut payload = serde_json::json!({
                "text": format!("[{}] {}", notif.notification_type.as_str(), notif.title),
            });
            if let Some(ch) = &slack.channel {
                payload["channel"] = serde_json::Value::String(ch.clone());
            }
            if let Err(e) = sender
                .post_json(&slack.webhook_url, &payload.to_string(), &[])
                .await
            {
                tracing::warn!(
                    user_id = %notif.user_id,
                    url = %slack.webhook_url,
                    error = %e,
                    "dispatcher: slack delivery failed"
                );
            }
        }
    }
}

/// Queue an email for the digest sender (§Email Notifications). The outbox is
/// durable (kv_store); the `email_digest` background job (email.rs) drains it
/// at the user's configured frequency, rendering templates per notification
/// type. SMTP transport is the deployment's concern — the interface contract
/// is the queue.
async fn queue_email(state: &AppState, notif: &Notification) {
    let entry = serde_json::json!({
        "notification_id": notif.id.to_string(),
        "user_id": notif.user_id.to_string(),
        "workspace_id": notif.workspace_id.to_string(),
        "subject": format!("[Gyre] {}", notif.title),
        "notification_type": notif.notification_type.as_str(),
        "priority": notif.priority,
        "body": notif.body,
        "entity_id": notif.source.entity_id,
        "action_url": notif.action_url,
        "queued_at": notif.created_at,
        // Full embedded notification: the digest renderer templates from the
        // complete record (type, title, body, deep link, timestamps) rather
        // than the flattened summary above.
        "notification": notif,
    });
    let key = format!("{}:{}", notif.user_id.as_str(), notif.id.as_str());
    if let Err(e) = state
        .kv_store
        .kv_set(EMAIL_OUTBOX_NS, &key, entry.to_string())
        .await
    {
        tracing::warn!(
            user_id = %notif.user_id,
            error = %e,
            "dispatcher: failed to queue email"
        );
    }
}

// ─── Routing engine (§Who Gets Notified) ─────────────────────────────────────

/// True when the user has at least one live presence entry (heartbeat within
/// the 60 s eviction window — see the presence janitor in lib.rs).
async fn user_is_online(state: &AppState, user_id: &Id) -> bool {
    let map = state.presence.read().await;
    map.keys().any(|(uid, _)| uid == user_id.as_str())
}

/// Members of the workspace holding one of the given roles.
async fn members_with_roles(
    state: &AppState,
    workspace_id: &Id,
    roles: &[WorkspaceRole],
) -> Vec<Id> {
    state
        .workspace_memberships
        .list_by_workspace(workspace_id)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|m| roles.contains(&m.role))
        .map(|m| m.user_id)
        .collect()
}

/// Recipients for an agent escalation to the Overseer
/// (§Notification Routing for Agent Escalations):
///
/// 1. The agent's `spawned_by` user is the primary recipient.
/// 2. If that user is offline, also notify the workspace Admins.
/// 3. For Urgent priority, always notify the workspace Owners.
///
/// Returns deduplicated user IDs.
pub async fn escalation_recipients(
    state: &AppState,
    agent: &gyre_domain::Agent,
    priority: NotificationPriority,
) -> Vec<Id> {
    let mut recipients: Vec<Id> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let push = |id: Id, recipients: &mut Vec<Id>, seen: &mut HashSet<String>| {
        if seen.insert(id.as_str().to_string()) {
            recipients.push(id);
        }
    };

    // 1. Primary recipient: the spawning user.
    let spawner_online = match &agent.spawned_by {
        Some(sb) => {
            let uid = Id::new(sb.clone());
            let online = user_is_online(state, &uid).await;
            push(uid, &mut recipients, &mut seen);
            online
        }
        None => false,
    };

    // 2. Offline spawner → workspace Admins.
    if !spawner_online {
        for admin in members_with_roles(state, &agent.workspace_id, &[WorkspaceRole::Admin]).await {
            push(admin, &mut recipients, &mut seen);
        }
    }

    // 3. Urgent → workspace Owners, always.
    if priority == NotificationPriority::Urgent {
        for owner in members_with_roles(state, &agent.workspace_id, &[WorkspaceRole::Owner]).await {
            push(owner, &mut recipients, &mut seen);
        }
    }

    recipients
}

/// Content of an agent-escalation notification (bundled to keep
/// `notify_agent_escalation` within clippy's 7-argument limit).
pub struct EscalationContent<'a> {
    pub notification_type: NotificationType,
    pub title: &'a str,
    pub tenant_id: &'a str,
    pub body: Option<String>,
    pub entity_ref: Option<String>,
    pub repo_id: Option<String>,
}

/// Deliver an agent-escalation notification to the routing-table recipients
/// (in-app record per recipient + channel fan-out per recipient config).
pub async fn notify_agent_escalation(
    state: &AppState,
    agent: &gyre_domain::Agent,
    content: EscalationContent<'_>,
) {
    let priority = NotificationPriority::for_notification_type(&content.notification_type);
    let recipients = escalation_recipients(state, agent, priority).await;
    let now = crate::api::now_secs() as i64;
    for user_id in recipients {
        let notif_id = Id::new(uuid::Uuid::new_v4().to_string());
        let mut notif = Notification::new(
            notif_id,
            agent.workspace_id.clone(),
            user_id.clone(),
            content.notification_type.clone(),
            content.title.to_string(),
            content.tenant_id,
            now,
        );
        notif.body = content.body.clone();
        notif.source.entity_type = "agent".to_string();
        notif.source.entity_id = content
            .entity_ref
            .clone()
            .unwrap_or_else(|| agent.id.to_string());
        notif.source.repo_id = content.repo_id.clone().map(Id::new);
        if let Err(e) = state.notifications.create(&notif).await {
            tracing::warn!(
                user_id = %user_id,
                error = %e,
                "escalation routing: failed to create notification"
            );
            continue;
        }
        dispatch_to_channels(state, &notif).await;
    }
}

/// Deliver a "persona approval requested" notification to the persona's
/// owner (user-management.md §Who Gets Notified). No-op when the persona has
/// no owner or the owner id is empty.
pub async fn notify_persona_approval_requested(state: &AppState, persona: &gyre_domain::Persona) {
    // Recipients per user-management.md §Who Gets Notified: the persona's
    // `owner`; when no owner is set, the workspace Admins/Owners (fallback
    // so a Pending persona is never silently unassigned).
    let mut recipients: Vec<Id> = Vec::new();
    if let Some(owner) = persona.owner.as_deref() {
        let owner = owner.strip_prefix("user:").unwrap_or(owner);
        if !owner.is_empty() {
            recipients.push(Id::new(owner.to_string()));
        }
    }
    let owner_missing = recipients.is_empty();
    // Workspace scope comes from the persona's scope; Tenant/Repo scopes
    // resolve to their entity's workspace when possible, else the
    // notification is skipped (never fabricated — see
    // check-fabricated-scope-defaults).
    let scope_ws: Option<Id> = match &persona.scope {
        gyre_domain::PersonaScope::Workspace(ws_id) => Some(ws_id.clone()),
        gyre_domain::PersonaScope::Tenant(tenant_id) => state
            .workspaces
            .list_by_tenant(tenant_id)
            .await
            .ok()
            .and_then(|ws_list| ws_list.into_iter().next().map(|ws| ws.id)),
        gyre_domain::PersonaScope::Repo(repo_id) => state
            .repos
            .find_by_id(repo_id)
            .await
            .ok()
            .flatten()
            .map(|r| r.workspace_id),
    };
    let Some(workspace_id) = scope_ws else {
        if owner_missing {
            tracing::info!(
                persona_id = %persona.id,
                "persona-approval routing: no routable owner (no owner set, non-workspace scope)"
            );
        } else {
            tracing::warn!(
                persona_id = %persona.id,
                "persona-approval routing: cannot resolve workspace scope — skipping notification"
            );
        }
        return;
    };
    if owner_missing {
        // No owner set: fall back to the workspace's Admins and Owners.
        let members = state
            .workspace_memberships
            .list_by_workspace(&workspace_id)
            .await
            .unwrap_or_default();
        recipients.extend(
            members
                .into_iter()
                .filter(|m| {
                    matches!(
                        m.role,
                        gyre_domain::WorkspaceRole::Admin | gyre_domain::WorkspaceRole::Owner
                    )
                })
                .map(|m| m.user_id),
        );
        if recipients.is_empty() {
            tracing::info!(
                persona_id = %persona.id,
                "persona-approval routing: no owner and no workspace Admins/Owners — skipping notification"
            );
            return;
        }
    }
    let tenant_id = match state.workspaces.find_by_id(&workspace_id).await {
        Ok(Some(ws)) => ws.tenant_id.to_string(),
        _ => {
            tracing::warn!(
                persona_id = %persona.id,
                workspace_id = %workspace_id,
                "persona-approval routing: cannot resolve tenant for workspace — skipping notification"
            );
            return;
        }
    };
    for user_id in recipients {
        notify_single_persona_approval(state, &workspace_id, user_id, persona, &tenant_id).await;
    }
}

async fn notify_single_persona_approval(
    state: &AppState,
    workspace_id: &Id,
    user_id: Id,
    persona: &gyre_domain::Persona,
    tenant_id: &str,
) {
    crate::notifications::notify_rich(
        state,
        workspace_id.clone(),
        user_id,
        NotificationType::PersonaApprovalRequested,
        format!("Persona '{}' requires your approval", persona.name),
        tenant_id,
        Some(
            serde_json::json!({
                "persona_id": persona.id.to_string(),
                "persona_slug": persona.slug,
                "approval_status": "Pending",
            })
            .to_string(),
        ),
        Some(persona.id.to_string()),
        None,
        "persona",
        None,
    )
    .await;
}

/// Deliver a "merge queue paused" notification to all workspace
/// Admins/Owners (user-management.md §Who Gets Notified). Skips (with a
/// log) when the workspace's tenant cannot be resolved.
pub async fn notify_merge_queue_paused(
    state: &AppState,
    workspace_id: &Id,
    repo: &gyre_domain::Repository,
    reason: &str,
) {
    let tenant_id = match state.workspaces.find_by_id(workspace_id).await {
        Ok(Some(ws)) => ws.tenant_id.to_string(),
        _ => {
            tracing::warn!(
                workspace_id = %workspace_id,
                repo_id = %repo.id,
                "merge-queue-pause routing: cannot resolve tenant for workspace — skipping notification"
            );
            return;
        }
    };
    let recipients = members_with_roles(
        state,
        workspace_id,
        &[WorkspaceRole::Admin, WorkspaceRole::Owner],
    )
    .await;
    let body = serde_json::json!({
        "repo_id": repo.id.to_string(),
        "repo_name": repo.name,
        "reason": reason,
    })
    .to_string();
    for user_id in recipients {
        crate::notifications::notify_rich(
            state,
            workspace_id.clone(),
            user_id,
            NotificationType::MergeQueuePaused,
            format!("Merge queue paused on {}: {}", repo.name, reason),
            &tenant_id,
            Some(body.clone()),
            Some(repo.id.to_string()),
            Some(repo.id.to_string()),
            "repo",
            None,
        )
        .await;
    }
}

/// Emission context for a budget-level notification (user-management.md
/// §Notification Entity: `AgentBudgetWarning` ≥ 80% of a daily limit,
/// High; `AgentBudgetExhausted` ≥ 100%, Urgent). Built by the production
/// trigger — the usage-recording threshold check in api/budget.rs
/// (`maybe_notify_budget_threshold`), which classifies the level and then
/// calls the helpers below. The helpers own recipient routing (§Who Gets
/// Notified budget rows) and per-period de-duplication, so the routing
/// contract has exactly one implementation.
pub struct BudgetNotificationContext {
    pub title: String,
    pub body: String,
    /// De-dup scope entity — the workspace whose budget crossed the level.
    pub entity_ref: String,
    pub repo_id: Option<String>,
    /// Budget period start (unix seconds): a user is notified at most once
    /// per period per level (usage counters increment on every call, so
    /// without this guard each call past the threshold would re-notify).
    pub period_start: i64,
}

/// Recipients for a budget-level notification: the usage's spawning user
/// (when it is agent-attributed), the spawning users of the workspace's
/// *active* agents (the population enforcement acts on — a spawner whose
/// agent is warned or stopped by budget enforcement is notified even when
/// the crossing report is unattributed or was filed by a different agent),
/// plus the workspace members holding `roles`. Deduplicated.
async fn budget_recipients(
    state: &AppState,
    workspace_id: &Id,
    spawned_by: Option<&Id>,
    roles: &[WorkspaceRole],
) -> Vec<Id> {
    // caller-scope:ok — recipients are resolved FROM this workspace
    // (list_by_workspace / members_with_roles), so the parameter is each
    // member's own scope by construction; there is no per-entity lookup to
    // make. This function only returns a recipient list — it creates
    // nothing.
    let mut recipients: Vec<Id> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let push = |id: &Id, recipients: &mut Vec<Id>, seen: &mut HashSet<String>| {
        if seen.insert(id.as_str().to_string()) {
            recipients.push(id.clone());
        }
    };
    if let Some(sb) = spawned_by {
        push(sb, &mut recipients, &mut seen);
    }
    if let Ok(agents) = state.agents.list_by_workspace(workspace_id).await {
        for agent in agents
            .into_iter()
            .filter(|a| a.status == gyre_domain::AgentStatus::Active)
        {
            if let Some(sb) = agent.spawned_by {
                push(&gyre_common::Id::new(sb), &mut recipients, &mut seen);
            }
        }
    }
    for member in members_with_roles(state, workspace_id, roles).await {
        push(&member, &mut recipients, &mut seen);
    }
    recipients
}

/// Shared delivery core for the budget rows: resolve recipients, skip
/// users already notified this period for this level/entity, deliver via
/// `notify_rich` (in-app record + channel fan-out). Returns the number of
/// notifications newly created — the level-crossing signal the
/// BudgetWarning bus event rides on.
async fn notify_budget_level(
    state: &AppState,
    workspace_id: &Id,
    spawned_by: Option<&Id>,
    ntype: NotificationType,
    roles: &[WorkspaceRole],
    tenant_id: &str,
    ctx: &BudgetNotificationContext,
) -> usize {
    let mut created = 0usize;
    for user_id in budget_recipients(state, workspace_id, spawned_by, roles).await {
        let already_notified = state
            .notifications
            .has_active_notification(
                workspace_id,
                &user_id,
                ntype.as_str(),
                &ctx.entity_ref,
                ctx.period_start,
            )
            .await
            .unwrap_or(false);
        if already_notified {
            continue;
        }
        crate::notifications::notify_rich(
            state,
            workspace_id.clone(),
            user_id,
            ntype.clone(),
            ctx.title.clone(),
            tenant_id,
            Some(ctx.body.clone()),
            Some(ctx.entity_ref.clone()),
            ctx.repo_id.clone(),
            "budget",
            None,
        )
        .await;
        created += 1;
    }
    created
}

/// Deliver an "agent budget warning" notification to the usage's spawning
/// user, the workspace's active agents' spawning users, and the workspace
/// Admins (user-management.md §Who Gets Notified: budget warning row).
/// Called by the production threshold check in api/budget.rs when
/// workspace utilization crosses 80% of a daily limit. Returns the number
/// of notifications newly created (0 when every recipient was already
/// notified this period).
pub async fn notify_agent_budget_warning(
    state: &AppState,
    workspace_id: &Id,
    spawned_by: Option<&Id>,
    tenant_id: &str,
    ctx: &BudgetNotificationContext,
) -> usize {
    notify_budget_level(
        state,
        workspace_id,
        spawned_by,
        NotificationType::AgentBudgetWarning,
        &[WorkspaceRole::Admin],
        tenant_id,
        ctx,
    )
    .await
}

/// Deliver an "agent budget exhausted" notification to the usage's spawning
/// user, the workspace's active agents' spawning users, and the workspace
/// Owner (user-management.md §Who Gets Notified: budget exhausted row).
/// Called by the production threshold check in api/budget.rs when
/// workspace utilization crosses 100% of a daily limit. Returns the number
/// of notifications newly created (0 when every recipient was already
/// notified this period).
pub async fn notify_agent_budget_exhausted(
    state: &AppState,
    workspace_id: &Id,
    spawned_by: Option<&Id>,
    tenant_id: &str,
    ctx: &BudgetNotificationContext,
) -> usize {
    notify_budget_level(
        state,
        workspace_id,
        spawned_by,
        NotificationType::AgentBudgetExhausted,
        &[WorkspaceRole::Owner],
        tenant_id,
        ctx,
    )
    .await
}

/// Deliver a "security finding" notification to the workspace Owners plus
/// the tenant Admins (user-management.md §Who Gets Notified: "Security
/// finding (Critical/High) → Workspace Owner + tenant Admin").
///
/// Routing contract for the "security finding" row (§Who Gets Notified:
/// "Security finding (Critical/High) → Workspace Owner + tenant Admin").
/// The emission *trigger* — a Critical/High severity finding from the
/// security gate (agent-gates.md §Gate Failure Feedback Escalation) — is
/// the security-scanner emitter, a product-wide gap recorded against
/// task-137; no caller exists yet. `severity` must be "Critical" or
/// "High" per the routing row — callers gate lower severities before
/// invoking.
pub async fn notify_security_finding(
    state: &AppState,
    workspace_id: &Id,
    severity: &str,
    title: &str,
    detail: &str,
    entity_ref: Option<String>,
    repo_id: Option<String>,
) {
    let tenant_id = match state.workspaces.find_by_id(workspace_id).await {
        Ok(Some(ws)) => ws.tenant_id.to_string(),
        _ => {
            tracing::warn!(
                workspace_id = %workspace_id,
                "security-finding routing: cannot resolve tenant for workspace — skipping notification"
            );
            return;
        }
    };

    let mut recipients: Vec<Id> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    // Workspace Owners.
    for owner in members_with_roles(state, workspace_id, &[WorkspaceRole::Owner]).await {
        if seen.insert(owner.as_str().to_string()) {
            recipients.push(owner);
        }
    }
    // Tenant Admins — platform-wide users holding the Admin role
    // (create_user maps UserRole::Admin → GlobalRole::TenantAdmin).
    for admin in state
        .users
        .list()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|u| u.roles.contains(&gyre_domain::UserRole::Admin))
    {
        if seen.insert(admin.id.as_str().to_string()) {
            recipients.push(admin.id);
        }
    }

    let body = serde_json::json!({
        "severity": severity,
        "detail": detail,
    })
    .to_string();
    for user_id in recipients {
        crate::notifications::notify_rich(
            state,
            workspace_id.clone(),
            user_id,
            NotificationType::SecurityFinding,
            format!("[{severity}] {title}"),
            &tenant_id,
            Some(body.clone()),
            entity_ref.clone(),
            repo_id.clone(),
            "workspace",
            None,
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use gyre_domain::{
        Agent, EmailConfig, NotificationChannels, NotificationPriority, SlackConfig, WebhookConfig,
    };
    use parking_lot::Mutex;
    use std::sync::Arc;

    fn band(t: NotificationType) -> NotificationPriority {
        NotificationPriority::for_notification_type(&t)
    }

    #[test]
    fn priority_bands_match_spec_membership() {
        // user-management.md §Notification Entity band comments (authoritative
        // examples) + the task-112 review amendment recording the full table.
        // Urgent: security finding, agent escalation, MR reverted.
        assert_eq!(
            band(NotificationType::SecurityFinding),
            NotificationPriority::Urgent
        );
        assert_eq!(
            band(NotificationType::AgentEscalation),
            NotificationPriority::Urgent
        );
        assert_eq!(
            band(NotificationType::MrReverted),
            NotificationPriority::Urgent
        );
        // High: budget warning, gate failure.
        assert_eq!(
            band(NotificationType::AgentBudgetWarning),
            NotificationPriority::High
        );
        assert_eq!(
            band(NotificationType::GateFailure),
            NotificationPriority::High
        );
        // High: secret approaching expiry (platform-model §7, merged from
        // main) — same approaching-deadline class as budget warning.
        assert_eq!(
            band(NotificationType::SecretExpiring),
            NotificationPriority::High
        );
        // High: persona change awaiting spec-lifecycle approval (merged
        // from main's task-116) — same pending-approval decision class as
        // SpecPendingApproval / PersonaApprovalRequested.
        assert_eq!(
            band(NotificationType::PersonaChangePendingApproval),
            NotificationPriority::High
        );
        // Urgent: worker spawn failed on the compute target after all
        // retries — the task is Blocked (agent-runtime.md §1 Phase 7,
        // arrived with the task-125 base): work blocked, immediate action,
        // same class as agent escalation.
        assert_eq!(
            band(NotificationType::ComputeTargetUnavailable),
            NotificationPriority::Urgent
        );
        // High: workspace has no active Owner and tenant admins must
        // reassign ownership (user-management.md §Ownership Transfer &
        // Reclamation, arrived with the task-125 base): needs attention
        // soon.
        assert_eq!(
            band(NotificationType::WorkspaceOwnerless),
            NotificationPriority::High
        );
        // Medium: stale dependency, spec drift.
        assert_eq!(
            band(NotificationType::MetaSpecDrift),
            NotificationPriority::Medium
        );
        assert_eq!(
            band(NotificationType::DependencyChainTooDeep),
            NotificationPriority::Medium
        );
        // Low: informational (MR merged, gate passed).
        assert_eq!(
            band(NotificationType::AgentCompleted),
            NotificationPriority::Low
        );
        assert_eq!(
            band(NotificationType::SuggestedSpecLink),
            NotificationPriority::Low
        );
    }

    /// One captured outbound HTTP delivery: (url, body, headers).
    type CapturedCall = (String, String, Vec<(String, String)>);

    /// Captures every outbound HTTP delivery for assertions.
    struct CapturingSender {
        calls: parking_lot::Mutex<Vec<CapturedCall>>,
    }

    #[async_trait]
    impl HttpSender for CapturingSender {
        async fn post_json(
            &self,
            url: &str,
            body: &str,
            headers: &[(&str, &str)],
        ) -> anyhow::Result<()> {
            self.calls.lock().push((
                url.to_string(),
                body.to_string(),
                headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            ));
            Ok(())
        }
    }

    async fn seed_member(
        state: &AppState,
        ws_id: &Id,
        user: &str,
        role: WorkspaceRole,
    ) -> gyre_domain::WorkspaceMembership {
        let m = gyre_domain::WorkspaceMembership::new(
            Id::new(format!("membership-{user}")),
            Id::new(user),
            ws_id.clone(),
            role,
            Id::new("inviter"),
            1000,
        );
        state.workspace_memberships.create(&m).await.unwrap();
        m
    }

    /// Seed the workspace so tenant resolution succeeds — the routing
    /// functions skip (never fabricate) when the workspace is missing.
    async fn seed_workspace(state: &AppState, ws_id: &Id) {
        let ws = gyre_domain::Workspace::new(
            ws_id.clone(),
            Id::new("tenant-1"),
            "Test Workspace",
            "test-workspace",
            1000,
        );
        state.workspaces.create(&ws).await.unwrap();
    }

    fn sample_notif(user: &str, notification_type: NotificationType) -> Notification {
        let title = format!("{} event", notification_type.as_str());
        Notification::new(
            Id::new("notif-1"),
            Id::new("ws-1"),
            Id::new(user),
            notification_type,
            title,
            "tenant-1",
            2_000_000,
        )
    }

    #[tokio::test]
    async fn webhook_delivery_is_signed_and_threshold_filtered() {
        let state = test_state();
        // Priority 3 (GateFailure) = Urgent band; webhook threshold High → delivered.
        let channels = NotificationChannels {
            in_app: true,
            email: EmailConfig::default(),
            webhook: Some(WebhookConfig {
                url: "https://hooks.example.test/gyre".to_string(),
                secret: "s3cret".to_string(),
                min_priority: NotificationPriority::High,
            }),
            slack: None,
        };
        // GateFailure is a High-band type (spec: "needs attention soon");
        // webhook threshold High → delivered (>= is inclusive).
        let notif = sample_notif("user-1", NotificationType::GateFailure);
        let sender = Arc::new(CapturingSender {
            calls: Mutex::new(Vec::new()),
        });
        dispatch_with(&state, &notif, &channels, sender.as_ref()).await;

        let calls = sender.calls.lock();
        assert_eq!(calls.len(), 1, "high band >= high threshold must deliver");
        let (url, body, headers) = &calls[0];
        assert_eq!(url, "https://hooks.example.test/gyre");
        // HMAC-SHA256 over the exact body bytes with the configured secret.
        let expected = sign_payload("s3cret", body);
        assert_eq!(
            headers
                .iter()
                .find(|(k, _)| k == "X-Gyre-Signature")
                .map(|(_, v)| v.clone()),
            Some(expected),
            "signature must be HMAC-SHA256 over the sent body"
        );
        // Body carries the notification fields the receiver needs.
        let parsed: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(parsed["notification_type"], "GateFailure");
        assert_eq!(parsed["title"], "GateFailure event");
    }

    #[tokio::test]
    async fn webhook_below_threshold_is_not_sent() {
        let state = test_state();
        // AgentCompleted is a Low-band type; webhook threshold High → suppressed.
        let channels = NotificationChannels {
            in_app: true,
            email: EmailConfig::default(),
            webhook: Some(WebhookConfig {
                url: "https://hooks.example.test/gyre".to_string(),
                secret: "s".to_string(),
                min_priority: NotificationPriority::High,
            }),
            slack: None,
        };
        let notif = sample_notif("user-1", NotificationType::AgentCompleted);
        let sender = Arc::new(CapturingSender {
            calls: Mutex::new(Vec::new()),
        });
        dispatch_with(&state, &notif, &channels, sender.as_ref()).await;
        assert!(
            sender.calls.lock().is_empty(),
            "low-priority notification must not reach a High-threshold webhook"
        );
    }

    #[tokio::test]
    async fn slack_delivery_posts_and_respects_channel_override() {
        let state = test_state();
        let channels = NotificationChannels {
            in_app: true,
            email: EmailConfig::default(),
            webhook: None,
            slack: Some(SlackConfig {
                webhook_url: "https://hooks.slack.test/T/B/X".to_string(),
                channel: Some("#ops".to_string()),
                min_priority: NotificationPriority::Low,
            }),
        };
        let notif = sample_notif("user-1", NotificationType::AgentCompleted);
        let sender = Arc::new(CapturingSender {
            calls: Mutex::new(Vec::new()),
        });
        dispatch_with(&state, &notif, &channels, sender.as_ref()).await;
        let calls = sender.calls.lock();
        assert_eq!(calls.len(), 1);
        let (url, body, _) = &calls[0];
        assert_eq!(url, "https://hooks.slack.test/T/B/X");
        let parsed: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(parsed["channel"], "#ops");
        assert!(parsed["text"]
            .as_str()
            .unwrap()
            .contains("AgentCompleted event"));
    }

    #[tokio::test]
    async fn email_queue_is_durable_and_threshold_filtered() {
        let state = test_state();
        // Enabled email, digest Immediate, threshold Medium; GateFailure
        // (High band) → queued in the kv outbox.
        let channels = NotificationChannels {
            in_app: true,
            email: EmailConfig {
                enabled: true,
                digest: gyre_domain::DigestFrequency::Immediate,
                min_priority: NotificationPriority::Medium,
                type_overrides: Default::default(),
            },
            webhook: None,
            slack: None,
        };
        let notif = sample_notif("user-1", NotificationType::GateFailure);
        let sender = Arc::new(CapturingSender {
            calls: Mutex::new(Vec::new()),
        });
        dispatch_with(&state, &notif, &channels, sender.as_ref()).await;

        let outbox = state.kv_store.kv_list(EMAIL_OUTBOX_NS).await.unwrap();
        assert_eq!(outbox.len(), 1, "high-band notification must be queued");
        let (key, value) = &outbox[0];
        assert!(key.starts_with("user-1:"));
        let parsed: serde_json::Value = serde_json::from_str(value).unwrap();
        assert_eq!(parsed["subject"], "[Gyre] GateFailure event");
        assert_eq!(parsed["user_id"], "user-1");

        // Low band (AgentCompleted) below Medium threshold → no queue entry.
        let notif_low = sample_notif("user-1", NotificationType::AgentCompleted);
        dispatch_with(&state, &notif_low, &channels, sender.as_ref()).await;
        let outbox = state.kv_store.kv_list(EMAIL_OUTBOX_NS).await.unwrap();
        assert_eq!(outbox.len(), 1, "below-threshold email must not queue");

        // Disabled email → no queue entry even at Urgent.
        let channels_off = NotificationChannels {
            email: EmailConfig {
                enabled: false,
                digest: gyre_domain::DigestFrequency::Immediate,
                min_priority: NotificationPriority::Low,
                type_overrides: Default::default(),
            },
            ..channels
        };
        let notif2 = sample_notif("user-2", NotificationType::SecurityFinding);
        dispatch_with(&state, &notif2, &channels_off, sender.as_ref()).await;
        let outbox = state.kv_store.kv_list(EMAIL_OUTBOX_NS).await.unwrap();
        assert_eq!(
            outbox.len(),
            1,
            "disabled email channel must not queue anything"
        );
    }

    /// Per-type unsubscribe (user-management.md §Email Notifications): a
    /// `type_overrides` entry for the notification's type suppresses the
    /// email queue entry even when the global channel config would deliver
    /// it — and the queued entry embeds the full notification for the
    /// digest renderer.
    #[tokio::test]
    async fn email_queue_respects_per_type_unsubscribe() {
        let state = test_state();
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("GateFailure".to_string(), true);
        let channels = NotificationChannels {
            in_app: true,
            email: EmailConfig {
                enabled: true,
                digest: gyre_domain::DigestFrequency::Daily,
                min_priority: NotificationPriority::Low,
                type_overrides: overrides,
            },
            webhook: None,
            slack: None,
        };
        let sender = Arc::new(CapturingSender {
            calls: Mutex::new(Vec::new()),
        });

        // Unsubscribed type (would pass every global gate: High band, Low
        // threshold, enabled) → NOT queued.
        let notif = sample_notif("user-1", NotificationType::GateFailure);
        dispatch_with(&state, &notif, &channels, sender.as_ref()).await;
        let outbox = state.kv_store.kv_list(EMAIL_OUTBOX_NS).await.unwrap();
        assert!(
            outbox.is_empty(),
            "per-type unsubscribe must suppress the queue entry"
        );

        // A different, subscribed type → queued, with the embedded
        // notification the digest renderer templates from.
        let notif2 = sample_notif("user-1", NotificationType::MergeQueuePaused);
        dispatch_with(&state, &notif2, &channels, sender.as_ref()).await;
        let outbox = state.kv_store.kv_list(EMAIL_OUTBOX_NS).await.unwrap();
        assert_eq!(outbox.len(), 1);
        let parsed: serde_json::Value = serde_json::from_str(&outbox[0].1).unwrap();
        assert_eq!(
            parsed["notification"]["notification_type"],
            "MergeQueuePaused",
            "queued entry embeds the full notification"
        );
    }

    /// Single effective config (review bf1e11be F1): when the user has no
    /// dedicated `user_channel_prefs` row — e.g. preferences stored via
    /// `PUT /users/me` before the dedicated table existed — the dispatcher
    /// must fall back to the channels embedded in the user's stored
    /// `users.preferences`, not silently default them away.
    #[tokio::test]
    async fn dispatch_falls_back_to_stored_user_preferences() {
        let state = test_state();
        let user_id = Id::new("legacy-prefs-user");
        let mut user =
            gyre_domain::User::new(user_id.clone(), "ext-legacy", "legacy-prefs-user", 1_000);
        user.preferences.notification_channels = NotificationChannels {
            in_app: true,
            email: EmailConfig {
                enabled: true,
                digest: gyre_domain::DigestFrequency::Immediate,
                min_priority: NotificationPriority::Low,
                type_overrides: Default::default(),
            },
            webhook: None,
            slack: None,
        };
        state.users.create(&user).await.unwrap();
        // No user_channel_prefs row: the fallback path must find the config.
        let notif = sample_notif("legacy-prefs-user", NotificationType::GateFailure);
        dispatch_to_channels(&state, &notif).await;

        let outbox = state.kv_store.kv_list(EMAIL_OUTBOX_NS).await.unwrap();
        assert_eq!(
            outbox.len(),
            1,
            "channels stored only in users.preferences must still deliver"
        );
        assert!(outbox[0].0.starts_with("legacy-prefs-user:"));
    }

    // ─── Escalation routing (§Notification Routing for Agent Escalations) ────

    fn agent_spawned_by(ws_id: &Id, spawner: Option<&str>) -> Agent {
        let mut a = Agent::new(Id::new("agent-1"), "worker-1", 1000);
        a.workspace_id = ws_id.clone();
        a.spawned_by = spawner.map(|s| s.to_string());
        a
    }

    #[tokio::test]
    async fn escalation_primary_recipient_is_spawning_user() {
        let state = test_state();
        let ws = Id::new("ws-1");
        seed_member(&state, &ws, "admin-1", WorkspaceRole::Admin).await;
        seed_member(&state, &ws, "owner-1", WorkspaceRole::Owner).await;

        // Bring spawner online: presence entry keyed by user id string.
        state.presence.write().await.insert(
            ("spawner-1".to_string(), "sess-1".to_string()),
            crate::PresenceEntry {
                workspace_id: "ws-1".to_string(),
                view: "inbox".to_string(),
                editing_entity: None,
                timestamp: 1,
                server_last_seen: u64::MAX, // never idle-evicted
                connection_id: 1,
            },
        );

        let agent = agent_spawned_by(&ws, Some("spawner-1"));
        let recipients = escalation_recipients(&state, &agent, NotificationPriority::High).await;
        assert_eq!(
            recipients,
            vec![Id::new("spawner-1")],
            "online spawner at High priority: no admin/owner fan-out"
        );
    }

    #[tokio::test]
    async fn offline_spawner_escalates_to_workspace_admins() {
        let state = test_state();
        let ws = Id::new("ws-1");
        seed_member(&state, &ws, "admin-1", WorkspaceRole::Admin).await;
        seed_member(&state, &ws, "dev-1", WorkspaceRole::Developer).await;
        seed_member(&state, &ws, "owner-1", WorkspaceRole::Owner).await;

        // Spawner has no presence entry → offline.
        let agent = agent_spawned_by(&ws, Some("spawner-1"));
        let recipients = escalation_recipients(&state, &agent, NotificationPriority::High).await;
        assert_eq!(
            recipients,
            vec![Id::new("spawner-1"), Id::new("admin-1")],
            "offline spawner adds workspace Admins (not Developers/Owners) at High"
        );
    }

    #[tokio::test]
    async fn urgent_priority_always_notifies_owners() {
        let state = test_state();
        let ws = Id::new("ws-1");
        seed_member(&state, &ws, "admin-1", WorkspaceRole::Admin).await;
        seed_member(&state, &ws, "owner-1", WorkspaceRole::Owner).await;

        // Spawner online, but Urgent forces Owners.
        state.presence.write().await.insert(
            ("spawner-1".to_string(), "sess-1".to_string()),
            crate::PresenceEntry {
                workspace_id: "ws-1".to_string(),
                view: "inbox".to_string(),
                editing_entity: None,
                timestamp: 1,
                server_last_seen: u64::MAX,
                connection_id: 1,
            },
        );

        let agent = agent_spawned_by(&ws, Some("spawner-1"));
        let recipients = escalation_recipients(&state, &agent, NotificationPriority::Urgent).await;
        assert_eq!(
            recipients,
            vec![Id::new("spawner-1"), Id::new("owner-1")],
            "Urgent always notifies workspace Owners even when spawner is online"
        );
    }

    #[tokio::test]
    async fn agent_escalation_notifies_owners_even_when_spawner_online() {
        // Regression (task-112 review): AgentEscalation is an Urgent-band
        // type per user-management.md §Notification Entity, so §Notification
        // Routing step 3 must fan out to workspace Owners even when the
        // spawning user is online. The old numeric from_band(5) mapping
        // classified it High, leaving the Owner branch unreachable in the
        // only production escalation path (fail_agent → notify_agent_escalation).
        let state = test_state();
        let ws = Id::new("ws-1");
        seed_member(&state, &ws, "admin-1", WorkspaceRole::Admin).await;
        seed_member(&state, &ws, "owner-1", WorkspaceRole::Owner).await;

        // Spawner online: presence entry keyed by user id string.
        state.presence.write().await.insert(
            ("spawner-1".to_string(), "sess-1".to_string()),
            crate::PresenceEntry {
                workspace_id: "ws-1".to_string(),
                view: "inbox".to_string(),
                editing_entity: None,
                timestamp: 1,
                server_last_seen: u64::MAX, // never idle-evicted
                connection_id: 1,
            },
        );

        let agent = agent_spawned_by(&ws, Some("spawner-1"));
        notify_agent_escalation(
            &state,
            &agent,
            EscalationContent {
                notification_type: NotificationType::AgentEscalation,
                title: "Agent 'worker-1' failed and needs attention",
                tenant_id: "tenant-1",
                body: None,
                entity_ref: Some("agent-1".to_string()),
                repo_id: None,
            },
        )
        .await;

        let owner_notifs = state
            .notifications
            .list_for_user(&Id::new("owner-1"), None, None, None, None, None, 50, 0)
            .await
            .unwrap();
        let spawner_notifs = state
            .notifications
            .list_for_user(&Id::new("spawner-1"), None, None, None, None, None, 50, 0)
            .await
            .unwrap();
        let admin_notifs = state
            .notifications
            .list_for_user(&Id::new("admin-1"), None, None, None, None, None, 50, 0)
            .await
            .unwrap();
        assert_eq!(
            owner_notifs.len(),
            1,
            "Urgent agent escalation must notify workspace Owners even when the spawner is online"
        );
        assert_eq!(
            spawner_notifs.len(),
            1,
            "spawner remains the primary recipient"
        );
        assert!(admin_notifs.is_empty(), "online spawner: no Admin fan-out");
    }

    #[tokio::test]
    async fn notify_agent_escalation_creates_inapp_notifications() {
        let state = test_state();
        let ws = Id::new("ws-1");
        seed_member(&state, &ws, "admin-1", WorkspaceRole::Admin).await;

        let agent = agent_spawned_by(&ws, Some("spawner-1")); // offline
        notify_agent_escalation(
            &state,
            &agent,
            EscalationContent {
                notification_type: NotificationType::AgentEscalation,
                title: "Agent 'worker-1' failed and needs attention",
                tenant_id: "tenant-1",
                body: None,
                entity_ref: Some("agent-1".to_string()),
                repo_id: None,
            },
        )
        .await;

        let admin_notifs = state
            .notifications
            .list_for_user(&Id::new("admin-1"), None, None, None, None, None, 50, 0)
            .await
            .unwrap();
        let spawner_notifs = state
            .notifications
            .list_for_user(&Id::new("spawner-1"), None, None, None, None, None, 50, 0)
            .await
            .unwrap();
        assert_eq!(admin_notifs.len(), 1);
        assert_eq!(spawner_notifs.len(), 1);
        assert_eq!(
            spawner_notifs[0].notification_type,
            NotificationType::AgentEscalation
        );
    }

    // ─── Routing table rows: persona approval, merge-queue pause, budget ──

    #[tokio::test]
    async fn persona_approval_requested_notifies_owner() {
        let state = test_state();
        let ws = Id::new("ws-1");
        seed_workspace(&state, &ws).await;
        seed_member(&state, &ws, "owner-1", WorkspaceRole::Owner).await;

        let mut persona = gyre_domain::Persona::new(
            Id::new("persona-1"),
            "security",
            "security",
            gyre_domain::PersonaScope::Workspace(ws.clone()),
            "You are a security reviewer.",
            1000,
        );
        persona.owner = Some("user:persona-owner".to_string());
        // Persona starts Pending (PersonaApprovalStatus::default()).
        assert_eq!(
            persona.approval_status,
            gyre_domain::PersonaApprovalStatus::Pending
        );

        notify_persona_approval_requested(&state, &persona).await;

        // Owner (with "user:" prefix stripped) got the notification.
        let owner_notifs = state
            .notifications
            .list_for_user(
                &Id::new("persona-owner"),
                Some(&ws),
                None,
                None,
                None,
                None,
                50,
                0,
            )
            .await
            .unwrap();
        assert_eq!(owner_notifs.len(), 1, "persona owner must be notified");
        assert_eq!(
            owner_notifs[0].notification_type,
            NotificationType::PersonaApprovalRequested
        );
        assert_eq!(
            owner_notifs[0].source.entity_id.as_str(),
            "persona-1",
            "source entity_id must deep-link the persona"
        );

        // Nobody else was notified (routing table: owner only).
        let owner_member_notifs = state
            .notifications
            .list_for_user(
                &Id::new("owner-1"),
                Some(&ws),
                None,
                None,
                None,
                None,
                50,
                0,
            )
            .await
            .unwrap();
        assert!(
            owner_notifs.len() == 1 && owner_member_notifs.is_empty(),
            "workspace members must not receive persona approval requests"
        );
    }

    #[tokio::test]
    async fn persona_approval_requested_without_owner_or_admins_is_noop() {
        let state = test_state();
        let persona = gyre_domain::Persona::new(
            Id::new("persona-2"),
            "reviewer",
            "reviewer",
            gyre_domain::PersonaScope::Workspace(Id::new("ws-1")),
            "Review code.",
            1000,
        );
        // owner: None (default), and ws-1 has no Admin/Owner members → no
        // routable recipient (merged fallback from main's task-123 routes
        // ownerless personas to workspace Admins/Owners).
        notify_persona_approval_requested(&state, &persona).await;
        let all = state
            .notifications
            .list_for_user(&Id::new("reviewer"), None, None, None, None, None, 50, 0)
            .await
            .unwrap();
        assert!(
            all.is_empty(),
            "no owner and no Admins/Owners → no notification"
        );
    }

    #[tokio::test]
    async fn merge_queue_paused_notifies_all_admins_and_owners() {
        let state = test_state();
        let ws = Id::new("ws-1");
        seed_workspace(&state, &ws).await;
        seed_member(&state, &ws, "admin-1", WorkspaceRole::Admin).await;
        seed_member(&state, &ws, "admin-2", WorkspaceRole::Admin).await;
        seed_member(&state, &ws, "owner-1", WorkspaceRole::Owner).await;
        seed_member(&state, &ws, "dev-1", WorkspaceRole::Developer).await;
        seed_member(&state, &ws, "viewer-1", WorkspaceRole::Viewer).await;

        let repo = gyre_domain::Repository::new(
            Id::new("repo-pause"),
            ws.clone(),
            "repo-pause",
            "/tmp/repo-pause",
            1000,
        );
        state.repos.create(&repo).await.unwrap();

        notify_merge_queue_paused(&state, &ws, &repo, "post-merge validation failed").await;

        for user in ["admin-1", "admin-2", "owner-1"] {
            let notifs = state
                .notifications
                .list_for_user(&Id::new(user), Some(&ws), None, None, None, None, 50, 0)
                .await
                .unwrap();
            assert_eq!(
                notifs.len(),
                1,
                "{user} (Admin/Owner) must receive the pause notification"
            );
            assert_eq!(
                notifs[0].notification_type,
                NotificationType::MergeQueuePaused
            );
            assert_eq!(
                notifs[0].source.repo_id.as_ref().map(|r| r.as_str()),
                Some("repo-pause")
            );
        }
        for user in ["dev-1", "viewer-1"] {
            let notifs = state
                .notifications
                .list_for_user(&Id::new(user), Some(&ws), None, None, None, None, 50, 0)
                .await
                .unwrap();
            assert!(
                notifs.is_empty(),
                "{user} (non-Admin/Owner) must not receive the pause notification"
            );
        }
    }

    fn budget_ctx(title: &str) -> BudgetNotificationContext {
        BudgetNotificationContext {
            title: title.to_string(),
            body: serde_json::json!({"usage_pct": 85.0}).to_string(),
            entity_ref: "ws-1".to_string(),
            repo_id: None,
            period_start: 0,
        }
    }

    #[tokio::test]
    async fn budget_warning_notifies_spawner_and_admins() {
        let state = test_state();
        let ws = Id::new("ws-1");
        seed_member(&state, &ws, "admin-1", WorkspaceRole::Admin).await;
        seed_member(&state, &ws, "owner-1", WorkspaceRole::Owner).await;

        let spawner = Id::new("spawner-1");
        notify_agent_budget_warning(
            &state,
            &ws,
            Some(&spawner),
            "tenant-1",
            &budget_ctx("Workspace budget at 85% of the daily limit"),
        )
        .await;

        for user in ["spawner-1", "admin-1"] {
            let notifs = state
                .notifications
                .list_for_user(&Id::new(user), Some(&ws), None, None, None, None, 50, 0)
                .await
                .unwrap();
            assert_eq!(
                notifs.len(),
                1,
                "{user} must receive the budget warning (spawner + Admins)"
            );
            assert_eq!(
                notifs[0].notification_type,
                NotificationType::AgentBudgetWarning
            );
        }
        let owner_notifs = state
            .notifications
            .list_for_user(
                &Id::new("owner-1"),
                Some(&ws),
                None,
                None,
                None,
                None,
                50,
                0,
            )
            .await
            .unwrap();
        assert!(
            owner_notifs.is_empty(),
            "budget warning must not notify Owners (that is the exhausted row)"
        );
    }

    /// The recipient set is the population budget enforcement acts on
    /// (semantics merged from the task-125 base's api/budget.rs): the
    /// spawners of the workspace's ACTIVE agents are notified even when
    /// the crossing report is unattributed (no explicit spawned_by), while
    /// an Idle agent's spawner is not — enforcement only stops active
    /// work, so only its owners need the warning.
    #[tokio::test]
    async fn budget_warning_notifies_active_agents_spawners() {
        let state = test_state();
        let ws = Id::new("ws-1");
        seed_member(&state, &ws, "admin-1", WorkspaceRole::Admin).await;

        let mut active = agent_spawned_by(&ws, Some("active-spawner"));
        active.id = Id::new("agent-active");
        active.name = "worker-active".to_string();
        active.status = gyre_domain::AgentStatus::Active;
        state.agents.create(&active).await.unwrap();
        let mut idle = agent_spawned_by(&ws, Some("idle-spawner"));
        idle.id = Id::new("agent-idle");
        idle.name = "worker-idle".to_string();
        idle.status = gyre_domain::AgentStatus::Idle;
        state.agents.create(&idle).await.unwrap();

        notify_agent_budget_warning(
            &state,
            &ws,
            None,
            "tenant-1",
            &budget_ctx("Workspace budget at 85% of the daily limit"),
        )
        .await;

        let active_notifs = state
            .notifications
            .list_for_user(
                &Id::new("active-spawner"),
                Some(&ws),
                None,
                None,
                None,
                None,
                50,
                0,
            )
            .await
            .unwrap();
        assert_eq!(
            active_notifs.len(),
            1,
            "active agent's spawner must be notified (enforcement acts on active agents)"
        );
        let idle_notifs = state
            .notifications
            .list_for_user(
                &Id::new("idle-spawner"),
                Some(&ws),
                None,
                None,
                None,
                None,
                50,
                0,
            )
            .await
            .unwrap();
        assert!(
            idle_notifs.is_empty(),
            "idle agent's spawner must not be notified (no work is stopped)"
        );
        // The explicit spawned_by recipient is still notified alongside
        // the active agents' spawners (deduplicated when they coincide).
        let admin_notifs = state
            .notifications
            .list_for_user(
                &Id::new("admin-1"),
                Some(&ws),
                None,
                None,
                None,
                None,
                50,
                0,
            )
            .await
            .unwrap();
        assert_eq!(admin_notifs.len(), 1, "Admins stay on the warning row");
    }

    #[tokio::test]
    async fn budget_level_notifies_each_user_once_per_period() {
        // The per-period de-dup guard: usage counters increment on every
        // call, so a second emission within the same period must not
        // re-notify any recipient.
        let state = test_state();
        let ws = Id::new("ws-1");
        seed_member(&state, &ws, "admin-1", WorkspaceRole::Admin).await;

        let spawner = Id::new("spawner-1");
        for _ in 0..2 {
            notify_agent_budget_warning(
                &state,
                &ws,
                Some(&spawner),
                "tenant-1",
                &budget_ctx("Workspace budget at 85% of the daily limit"),
            )
            .await;
        }

        for user in ["spawner-1", "admin-1"] {
            let notifs = state
                .notifications
                .list_for_user(&Id::new(user), Some(&ws), None, None, None, None, 50, 0)
                .await
                .unwrap();
            assert_eq!(
                notifs.len(),
                1,
                "{user} must be notified exactly once per budget period"
            );
        }
    }

    #[tokio::test]
    async fn budget_exhausted_notifies_spawner_and_owner() {
        let state = test_state();
        let ws = Id::new("ws-1");
        seed_member(&state, &ws, "admin-1", WorkspaceRole::Admin).await;
        seed_member(&state, &ws, "owner-1", WorkspaceRole::Owner).await;

        let spawner = Id::new("spawner-1");
        notify_agent_budget_exhausted(
            &state,
            &ws,
            Some(&spawner),
            "tenant-1",
            &budget_ctx("Workspace budget exhausted"),
        )
        .await;

        for user in ["spawner-1", "owner-1"] {
            let notifs = state
                .notifications
                .list_for_user(&Id::new(user), Some(&ws), None, None, None, None, 50, 0)
                .await
                .unwrap();
            assert_eq!(
                notifs.len(),
                1,
                "{user} must receive the budget-exhausted notification (spawner + Owner)"
            );
            assert_eq!(
                notifs[0].notification_type,
                NotificationType::AgentBudgetExhausted
            );
        }
        let admin_notifs = state
            .notifications
            .list_for_user(
                &Id::new("admin-1"),
                Some(&ws),
                None,
                None,
                None,
                None,
                50,
                0,
            )
            .await
            .unwrap();
        assert!(
            admin_notifs.is_empty(),
            "budget exhausted must not notify Admins (that is the warning row)"
        );
    }

    #[tokio::test]
    async fn security_finding_notifies_owners_and_tenant_admins() {
        let state = test_state();
        let ws = Id::new("ws-1");
        seed_workspace(&state, &ws).await;
        seed_member(&state, &ws, "owner-1", WorkspaceRole::Owner).await;
        seed_member(&state, &ws, "admin-1", WorkspaceRole::Admin).await;
        seed_member(&state, &ws, "dev-1", WorkspaceRole::Developer).await;
        // Platform tenant admin (not a workspace member).
        let mut tenant_admin = gyre_domain::User::new(
            Id::new("tenant-admin-1"),
            "ext-tenant-admin",
            "tenant-admin",
            1000,
        );
        tenant_admin.roles = vec![gyre_domain::UserRole::Admin];
        state.users.create(&tenant_admin).await.unwrap();

        notify_security_finding(
            &state,
            &ws,
            "Critical",
            "SQL injection in query builder",
            "handlers.rs:47 uses string interpolation",
            Some("mr-42".to_string()),
            Some("repo-1".to_string()),
        )
        .await;

        // Routing row: Workspace Owner + tenant Admin.
        for user in ["owner-1", "tenant-admin-1"] {
            let notifs = state
                .notifications
                .list_for_user(&Id::new(user), Some(&ws), None, None, None, None, 50, 0)
                .await
                .unwrap();
            assert_eq!(
                notifs.len(),
                1,
                "{user} (Owner / tenant Admin) must receive the security finding"
            );
            assert_eq!(
                notifs[0].notification_type,
                NotificationType::SecurityFinding
            );
            assert_eq!(notifs[0].title, "[Critical] SQL injection in query builder");
            assert_eq!(notifs[0].source.entity_id.as_str(), "mr-42");
            assert_eq!(
                notifs[0].source.repo_id.as_ref().map(|r| r.as_str()),
                Some("repo-1")
            );
        }
        // Non-privileged members must not be notified.
        for user in ["admin-1", "dev-1"] {
            let notifs = state
                .notifications
                .list_for_user(&Id::new(user), Some(&ws), None, None, None, None, 50, 0)
                .await
                .unwrap();
            assert!(
                notifs.is_empty(),
                "{user} must not receive the security finding"
            );
        }
    }
}
