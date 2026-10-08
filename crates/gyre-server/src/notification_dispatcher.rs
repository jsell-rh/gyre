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
pub trait HttpSender: Send + Sync {
    async fn post_json(&self, url: &str, body: &str, headers: &[(&str, &str)]) -> anyhow::Result<()>;
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
    async fn post_json(&self, url: &str, body: &str, headers: &[(&str, &str)]) -> anyhow::Result<()> {
        let mut req = self.client.post(url).header("Content-Type", "application/json");
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

/// Routes a persisted in-app notification to the recipient's configured
/// channels (§Delivery Channels). Best-effort per channel: a channel failure
/// is logged, never propagated — in-app delivery already succeeded.
pub async fn dispatch_to_channels(state: &AppState, notif: &Notification) {
    let channels = match state.user_channel_prefs.find(&notif.user_id).await {
        Ok(Some(c)) => c,
        Ok(None) => NotificationChannels::default(),
        Err(e) => {
            tracing::warn!(
                user_id = %notif.user_id,
                error = %e,
                "dispatcher: failed to load channel preferences; in-app only"
            );
            return;
        }
    };
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

    let priority = NotificationPriority::from_band(notif.priority);

    // Email channel.
    if channels.email.enabled
        && channels.email.digest != gyre_domain::DigestFrequency::Off
        && priority >= channels.email.min_priority
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
                "entity_ref": notif.entity_ref,
                "repo_id": notif.repo_id,
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
/// durable (kv_store); a background digest job or operator drains it. SMTP
/// transport is the deployment's concern — the interface contract is the queue.
async fn queue_email(state: &AppState, notif: &Notification) {
    let entry = serde_json::json!({
        "notification_id": notif.id.to_string(),
        "user_id": notif.user_id.to_string(),
        "workspace_id": notif.workspace_id.to_string(),
        "subject": format!("[Gyre] {}", notif.title),
        "notification_type": notif.notification_type.as_str(),
        "priority": notif.priority,
        "body": notif.body,
        "entity_ref": notif.entity_ref,
        "queued_at": notif.created_at,
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

/// Deliver an agent-escalation notification to the routing-table recipients
/// (in-app record per recipient + channel fan-out per recipient config).
pub async fn notify_agent_escalation(
    state: &AppState,
    agent: &gyre_domain::Agent,
    notification_type: NotificationType,
    title: &str,
    tenant_id: &str,
    body: Option<String>,
    entity_ref: Option<String>,
    repo_id: Option<String>,
) {
    let priority = NotificationPriority::from_band(notification_type.default_priority());
    let recipients = escalation_recipients(state, agent, priority).await;
    let now = crate::api::now_secs() as i64;
    for user_id in recipients {
        let notif_id = Id::new(uuid::Uuid::new_v4().to_string());
        let mut notif = Notification::new(
            notif_id,
            agent.workspace_id.clone(),
            user_id.clone(),
            notification_type.clone(),
            title.to_string(),
            tenant_id,
            now,
        );
        notif.body = body.clone();
        notif.entity_ref = entity_ref.clone();
        notif.repo_id = repo_id.clone();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use gyre_domain::{
        Agent, EmailConfig, NotificationChannels, NotificationPriority, SlackConfig, WebhookConfig,
    };
    use parking_lot::Mutex;
    use std::sync::Arc;

    fn band(p: u8) -> NotificationPriority {
        NotificationPriority::from_band(p)
    }

    #[test]
    fn priority_bands_match_spec_examples() {
        // Low: informational (MR merged=9, gate passed) — Urgent: security
        // finding, agent escalation, MR reverted (priority 1-3 per HSI §8).
        assert_eq!(band(9), NotificationPriority::Low);
        assert_eq!(band(10), NotificationPriority::Low);
        assert_eq!(band(7), NotificationPriority::Medium);
        assert_eq!(band(6), NotificationPriority::High);
        assert_eq!(band(4), NotificationPriority::High);
        assert_eq!(band(3), NotificationPriority::Urgent);
        assert_eq!(band(1), NotificationPriority::Urgent);
    }

    /// Captures every outbound HTTP delivery for assertions.
    struct CapturingSender {
        calls: parking_lot::Mutex<Vec<(String, String, Vec<(String, String)>)>>,
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

    fn sample_notif(user: &str, priority: u8) -> Notification {
        Notification::new(
            Id::new("notif-1"),
            Id::new("ws-1"),
            Id::new(user),
            NotificationType::GateFailure,
            "Gate failed",
            "tenant-1",
            2_000_000,
        )
        .with_priority(priority)
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
        let notif = sample_notif("user-1", 3);
        let sender = Arc::new(CapturingSender {
            calls: Mutex::new(Vec::new()),
        });
        dispatch_with(&state, &notif, &channels, sender.as_ref()).await;

        let calls = sender.calls.lock();
        assert_eq!(calls.len(), 1, "urgent >= high threshold must deliver");
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
        assert_eq!(parsed["title"], "Gate failed");
    }

    #[tokio::test]
    async fn webhook_below_threshold_is_not_sent() {
        let state = test_state();
        // Priority 9 = Low band; webhook threshold High → suppressed.
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
        let notif = sample_notif("user-1", 9);
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
        let notif = sample_notif("user-1", 9);
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
        assert!(parsed["text"].as_str().unwrap().contains("Gate failed"));
    }

    #[tokio::test]
    async fn email_queue_is_durable_and_threshold_filtered() {
        let state = test_state();
        // Enabled email, digest Immediate, threshold Medium; notification
        // priority 3 (Urgent band) → queued in the kv outbox.
        let channels = NotificationChannels {
            in_app: true,
            email: EmailConfig {
                enabled: true,
                digest: gyre_domain::DigestFrequency::Immediate,
                min_priority: NotificationPriority::Medium,
            },
            webhook: None,
            slack: None,
        };
        let notif = sample_notif("user-1", 3);
        let sender = Arc::new(CapturingSender {
            calls: Mutex::new(Vec::new()),
        });
        dispatch_with(&state, &notif, &channels, sender.as_ref()).await;

        let outbox = state.kv_store.kv_list(EMAIL_OUTBOX_NS).await.unwrap();
        assert_eq!(outbox.len(), 1, "urgent notification must be queued");
        let (key, value) = &outbox[0];
        assert!(key.starts_with("user-1:"));
        let parsed: serde_json::Value = serde_json::from_str(value).unwrap();
        assert_eq!(parsed["subject"], "[Gyre] Gate failed");
        assert_eq!(parsed["user_id"], "user-1");

        // Low priority (9) below Medium threshold → no queue entry.
        let notif_low = sample_notif("user-1", 9);
        dispatch_with(&state, &notif_low, &channels, sender.as_ref()).await;
        let outbox = state.kv_store.kv_list(EMAIL_OUTBOX_NS).await.unwrap();
        assert_eq!(outbox.len(), 1, "below-threshold email must not queue");

        // Disabled email → no queue entry even at Urgent.
        let channels_off = NotificationChannels {
            email: EmailConfig {
                enabled: false,
                digest: gyre_domain::DigestFrequency::Immediate,
                min_priority: NotificationPriority::Low,
            },
            ..channels
        };
        let notif2 = sample_notif("user-2", 1);
        dispatch_with(&state, &notif2, &channels_off, sender.as_ref()).await;
        let outbox = state.kv_store.kv_list(EMAIL_OUTBOX_NS).await.unwrap();
        assert_eq!(
            outbox.len(),
            1,
            "disabled email channel must not queue anything"
        );
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
        state
            .presence
            .write()
            .await
            .insert(
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
        state
            .presence
            .write()
            .await
            .insert(
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
    async fn notify_agent_escalation_creates_inapp_notifications() {
        let state = test_state();
        let ws = Id::new("ws-1");
        seed_member(&state, &ws, "admin-1", WorkspaceRole::Admin).await;

        let agent = agent_spawned_by(&ws, Some("spawner-1")); // offline
        notify_agent_escalation(
            &state,
            &agent,
            NotificationType::AgentEscalation,
            "Agent 'worker-1' failed and needs attention",
            "tenant-1",
            None,
            Some("agent-1".to_string()),
            None,
        )
        .await;

        let admin_notifs = state
            .notifications
            .list_for_user(&Id::new("admin-1"), None, None, None, None, 50, 0)
            .await
            .unwrap();
        let spawner_notifs = state
            .notifications
            .list_for_user(&Id::new("spawner-1"), None, None, None, None, 50, 0)
            .await
            .unwrap();
        assert_eq!(admin_notifs.len(), 1);
        assert_eq!(spawner_notifs.len(), 1);
        assert_eq!(
            spawner_notifs[0].notification_type,
            NotificationType::AgentEscalation
        );
    }
}
