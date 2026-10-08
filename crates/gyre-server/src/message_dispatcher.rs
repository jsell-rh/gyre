//! Message bus consumer dispatch (message-bus.md §Relationship to Notifications).
//!
//! The server's message-send path (`AppState::emit_event`, the REST send handler,
//! and the MCP `message.send` tool) clones each stored message into the bounded
//! `message_dispatch_tx` mpsc channel (capacity 256). `spawn_message_consumer`
//! drains that channel in a background task and dispatches every message to all
//! registered [`MessageConsumer`]s off the send hot path.
//!
//! The first consumer is [`NotificationBridge`] — the notification system's
//! bridge onto the message bus. It derives user-facing Inbox notifications from
//! Event-tier messages:
//!
//! - `GateFailure`            → priority-3 `GateFailure` notification for the MR
//!   author agent's spawning user (HSI §8 p3).
//! - `BudgetWarning`          → priority-7 `BudgetWarning` notification for the
//!   workspace's Admin/Developer/Owner members (agent-runtime.md budget table).
//! - `AgentError`             → priority-5 `AgentEscalation` notification for the
//!   failing agent's spawning user (HSI §8 p5).
//! - `ReconciliationCompleted` → priority-6 `MetaSpecDrift` notification for the
//!   workspace's Admin/Developer/Owner members (HSI §8 p6 — explicitly routed
//!   "Via MessageConsumer consuming ReconciliationCompleted events").
//!
//! Consumer drops (channel full) do not affect delivery guarantees — consumers
//! are downstream observers (message-bus.md §Relationship to Notifications).

use crate::AppState;
use gyre_common::message::{Destination, Message, MessageKind, MessageOrigin};
use gyre_common::{Id, Notification, NotificationType};
use gyre_domain::WorkspaceRole;
use gyre_ports::MessageConsumer;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;

/// Registry of consumers invoked for every message drained from the bus.
pub struct MessageDispatcher {
    consumers: Vec<Arc<dyn MessageConsumer>>,
}

impl MessageDispatcher {
    /// Build a dispatcher over the given consumers.
    pub fn new(consumers: Vec<Arc<dyn MessageConsumer>>) -> Self {
        Self { consumers }
    }

    /// The dispatcher's registered consumers (for inspection in tests).
    pub fn consumers(&self) -> &[Arc<dyn MessageConsumer>] {
        &self.consumers
    }

    /// Dispatch one message to every consumer.
    async fn dispatch(&self, message: &Message) {
        for consumer in &self.consumers {
            consumer.on_message(message).await;
        }
    }

    /// Drain the channel until closed, dispatching each message.
    pub async fn run(&self, mut rx: Receiver<Message>) {
        while let Some(msg) = rx.recv().await {
            self.dispatch(&msg).await;
        }
        tracing::info!("message dispatch channel closed; consumer task exiting");
    }
}

/// Spawn the background task that drains the message-dispatch channel and
/// dispatches to the registered consumers.
///
/// The channel is created alongside `AppState`; this function takes ownership
/// of the receiver half (via [`AppState::take_message_dispatch_rx`]) and hands
/// it to the dispatch task. Call once at startup (main.rs), after `build_state`.
pub fn spawn_message_consumer(state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
    let dispatcher = MessageDispatcher::new(vec![Arc::new(NotificationBridge::new(
        Arc::clone(&state),
    ))]);

    let rx = state
        .take_message_dispatch_rx()
        .expect("message dispatch receiver already consumed");

    tokio::spawn(async move {
        dispatcher.run(rx).await;
    })
}

/// The notification system's [`MessageConsumer`] implementation: derives
/// user-facing Inbox notifications from bus messages (message-bus.md
/// §Relationship to Notifications — "The notification system implements
/// MessageConsumer").
pub struct NotificationBridge {
    state: Arc<AppState>,
}

impl NotificationBridge {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    /// Resolve the tenant for a workspace without fabricating a scope identity
    /// (AGENTS.md: never default tenant_id to "default" on lookup failure).
    /// Returns `None` when the workspace cannot be resolved — the caller skips
    /// the notification rather than mis-attributing it.
    async fn tenant_for_workspace(&self, workspace_id: &Id) -> Option<String> {
        match self.state.workspaces.find_by_id(workspace_id).await {
            Ok(Some(ws)) => Some(ws.tenant_id.to_string()),
            Ok(None) => {
                tracing::warn!(
                    "NotificationBridge: workspace {workspace_id} not found; skipping notification"
                );
                None
            }
            Err(e) => {
                tracing::warn!("NotificationBridge: failed to resolve workspace tenant: {e}");
                None
            }
        }
    }

    /// Workspace members with Admin/Developer/Owner role — the human roles
    /// that receive workspace-level notification fan-out.
    async fn workspace_human_members(&self, workspace_id: &Id) -> Vec<Id> {
        match self
            .state
            .workspace_memberships
            .list_by_workspace(workspace_id)
            .await
        {
            Ok(members) => members
                .into_iter()
                .filter(|m| {
                    matches!(
                        m.role,
                        WorkspaceRole::Admin | WorkspaceRole::Developer | WorkspaceRole::Owner
                    )
                })
                .map(|m| m.user_id)
                .collect(),
            Err(e) => {
                tracing::warn!("NotificationBridge: failed to list workspace members: {e}");
                Vec::new()
            }
        }
    }

    fn now_secs() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    }

    /// GateFailure → priority-3 GateFailure notification for the MR author's
    /// spawning user (HSI §8 p3). Payload per message-bus.md §Payload Schemas:
    /// `{mr_id, gate_name, gate_type, status, output, spec_ref, gate_agent_id}`.
    async fn on_gate_failure(&self, msg: &Message) {
        let Some(ws_id) = msg.workspace_id.clone() else {
            return;
        };
        let Some(payload) = msg.payload.as_ref() else {
            return;
        };
        let mr_id = payload.get("mr_id").and_then(|v| v.as_str()).unwrap_or("");
        let gate_name = payload
            .get("gate_name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let Some(tenant_id) = self.tenant_for_workspace(&ws_id).await else {
            return;
        };

        // Resolve the MR's author agent, then its spawning user. If either
        // lookup fails there is no human to notify — skip rather than notify
        // a fabricated identity.
        let author_agent_id = self
            .state
            .merge_requests
            .find_by_id(&Id::new(mr_id.to_string()))
            .await
            .ok()
            .flatten()
            .and_then(|mr| mr.author_agent_id);
        let Some(author_agent_id) = author_agent_id else {
            tracing::debug!(
                "NotificationBridge: GateFailure for MR {mr_id} with no author agent; skipping"
            );
            return;
        };
        let spawned_by = self
            .state
            .agents
            .find_by_id(&author_agent_id)
            .await
            .ok()
            .flatten()
            .and_then(|a| a.spawned_by);
        let Some(spawned_by) = spawned_by else {
            tracing::debug!(
                "NotificationBridge: GateFailure author agent {author_agent_id} has no spawning \
                 user; skipping"
            );
            return;
        };

        let body = serde_json::json!({
            "mr_id": mr_id,
            "gate_name": gate_name,
            "agent_id": author_agent_id.as_str(),
        })
        .to_string();

        self.create(
            &ws_id,
            Id::new(spawned_by),
            NotificationType::GateFailure,
            format!("Gate '{gate_name}' failed on MR {mr_id}"),
            tenant_id,
            Some(body),
            Some(mr_id.to_string()),
        )
        .await;
    }

    /// BudgetWarning → priority-7 BudgetWarning notification for workspace
    /// Admin/Developer/Owner members (agent-runtime.md budget table: at 80%
    /// usage, "BudgetWarning notification created (Inbox priority 7)").
    /// Payload: `{agent_id, workspace_id, usage_pct}`.
    async fn on_budget_warning(&self, msg: &Message) {
        let Some(ws_id) = msg.workspace_id.clone() else {
            return;
        };
        let usage_pct = msg
            .payload
            .as_ref()
            .and_then(|p| p.get("usage_pct"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let Some(tenant_id) = self.tenant_for_workspace(&ws_id).await else {
            return;
        };

        let title = format!("Agent budget warning — {usage_pct:.0}% of budget used");
        for user_id in self.workspace_human_members(&ws_id).await {
            self.create(
                &ws_id,
                user_id,
                NotificationType::BudgetWarning,
                title.clone(),
                tenant_id.clone(),
                msg.payload.as_ref().map(|p| p.to_string()),
                None,
            )
            .await;
        }
    }

    /// AgentError → priority-5 AgentEscalation notification for the failing
    /// agent's spawning user (HSI §8 p5: "an agent has escalated or failed
    /// and needs human attention"). Payload: `{agent_id, error, context?}`.
    async fn on_agent_error(&self, msg: &Message) {
        let Some(ws_id) = msg.workspace_id.clone() else {
            return;
        };
        let Some(agent_id) = msg
            .payload
            .as_ref()
            .and_then(|p| p.get("agent_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
        else {
            return;
        };
        let error = msg
            .payload
            .as_ref()
            .and_then(|p| p.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");

        let Some(tenant_id) = self.tenant_for_workspace(&ws_id).await else {
            return;
        };

        let spawned_by = self
            .state
            .agents
            .find_by_id(&Id::new(agent_id.clone()))
            .await
            .ok()
            .flatten()
            .and_then(|a| a.spawned_by);
        let Some(spawned_by) = spawned_by else {
            tracing::debug!(
                "NotificationBridge: AgentError for agent {agent_id} with no spawning user; \
                 skipping"
            );
            return;
        };

        self.create(
            &ws_id,
            Id::new(spawned_by),
            NotificationType::AgentEscalation,
            format!("Agent error: {error}"),
            tenant_id,
            msg.payload.as_ref().map(|p| p.to_string()),
            Some(agent_id),
        )
        .await;
    }

    /// ReconciliationCompleted → priority-6 MetaSpecDrift notification for
    /// workspace Admin/Developer/Owner members (HSI §8 p6: "Via
    /// MessageConsumer consuming ReconciliationCompleted events";
    /// meta-spec-reconciliation.md §11).
    async fn on_reconciliation_completed(&self, msg: &Message) {
        let Some(ws_id) = msg.workspace_id.clone() else {
            return;
        };

        let Some(tenant_id) = self.tenant_for_workspace(&ws_id).await else {
            return;
        };

        let title = "Meta-spec reconciliation completed — workspace specs may have drifted";
        for user_id in self.workspace_human_members(&ws_id).await {
            self.create(
                &ws_id,
                user_id,
                NotificationType::MetaSpecDrift,
                title,
                tenant_id.clone(),
                msg.payload.as_ref().map(|p| p.to_string()),
                None,
            )
            .await;
        }
    }

    async fn create(
        &self,
        workspace_id: &Id,
        user_id: Id,
        notification_type: NotificationType,
        title: String,
        tenant_id: String,
        body: Option<String>,
        entity_ref: Option<String>,
    ) {
        let notif = Notification {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            workspace_id: workspace_id.clone(),
            user_id,
            notification_type,
            priority: notification_type.default_priority(),
            title,
            body,
            entity_ref,
            repo_id: None,
            resolved_at: None,
            dismissed_at: None,
            created_at: Self::now_secs(),
            tenant_id,
        };
        if let Err(e) = self.state.notifications.create(&notif).await {
            tracing::warn!("NotificationBridge: failed to create notification: {e}");
        }
    }
}

#[async_trait::async_trait]
impl MessageConsumer for NotificationBridge {
    async fn on_message(&self, message: &Message) {
        match &message.kind {
            MessageKind::GateFailure => self.on_gate_failure(message).await,
            MessageKind::BudgetWarning => self.on_budget_warning(message).await,
            MessageKind::AgentError => self.on_agent_error(message).await,
            MessageKind::ReconciliationCompleted => self.on_reconciliation_completed(message).await,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_domain::WorkspaceMembership;

    fn test_message(kind: MessageKind, ws: &str, payload: serde_json::Value) -> Message {
        Message {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            tenant_id: Id::new("default"),
            from: MessageOrigin::Server,
            workspace_id: Some(Id::new(ws.to_string())),
            to: Destination::Workspace(Id::new(ws.to_string())),
            kind,
            payload: Some(payload),
            created_at: 0,
            signature: None,
            key_id: None,
            acknowledged: false,
        }
    }

    async fn seed_workspace(state: &AppState, ws: &str, tenant: &str) {
        let ws_entity = gyre_domain::Workspace::new(Id::new(ws.to_string()), tenant.to_string());
        state.workspaces.create(&ws_entity).await.unwrap();
    }

    async fn seed_member(state: &AppState, ws: &str, user: &str, role: WorkspaceRole) {
        let m = WorkspaceMembership {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            workspace_id: Id::new(ws.to_string()),
            user_id: Id::new(user.to_string()),
            role,
            accepted_at: Some(0),
            created_at: 0,
        };
        state.workspace_memberships.create(&m).await.unwrap();
    }

    async fn seed_agent(state: &AppState, id: &str, ws: &str, spawned_by: Option<&str>) {
        let mut agent = gyre_domain::Agent::new(Id::new(id.to_string()), id, 1000);
        agent.workspace_id = Id::new(ws.to_string());
        agent.spawned_by = spawned_by.map(|s| s.to_string());
        state.agents.create(&agent).await.unwrap();
    }

    async fn list_notifications(state: &AppState, user: &str) -> Vec<Notification> {
        state
            .notifications
            .list_for_user(&Id::new(user.to_string()), None, None, None, None, 100, 0)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn notification_bridge_creates_gate_failure_notification() {
        let state = crate::mem::test_state();
        seed_workspace(&state, "ws-1", "tenant-1").await;
        seed_member(&state, "ws-1", "user-1", WorkspaceRole::Admin).await;
        seed_agent(&state, "agent-1", "ws-1", Some("user-1")).await;

        let mr = gyre_domain::MergeRequest::new(
            Id::new("mr-1".to_string()),
            Id::new("repo-1".to_string()),
            "feature".to_string(),
            "main".to_string(),
            Some(Id::new("agent-1".to_string())),
        );
        state.merge_requests.create(&mr).await.unwrap();

        let bridge = NotificationBridge::new(state.clone());
        let msg = test_message(
            MessageKind::GateFailure,
            "ws-1",
            serde_json::json!({
                "mr_id": "mr-1",
                "gate_name": "tests",
                "gate_type": "TestCommand",
                "status": "Failed",
                "output": "1 test failed",
                "gate_agent_id": "gate-agent:1",
            }),
        );
        bridge.on_message(&msg).await;

        let notifs = list_notifications(&state, "user-1").await;
        assert_eq!(notifs.len(), 1);
        assert_eq!(notifs[0].notification_type, NotificationType::GateFailure);
        assert_eq!(notifs[0].priority, 3);
        assert_eq!(notifs[0].tenant_id, "tenant-1");
        assert_eq!(notifs[0].entity_ref.as_deref(), Some("mr-1"));
    }

    #[tokio::test]
    async fn notification_bridge_creates_meta_spec_drift_notification() {
        let state = crate::mem::test_state();
        seed_workspace(&state, "ws-1", "tenant-1").await;
        seed_member(&state, "ws-1", "user-admin", WorkspaceRole::Admin).await;
        seed_member(&state, "ws-1", "user-dev", WorkspaceRole::Developer).await;
        seed_member(&state, "ws-1", "user-viewer", WorkspaceRole::Viewer).await;

        let bridge = NotificationBridge::new(state.clone());
        let msg = test_message(
            MessageKind::ReconciliationCompleted,
            "ws-1",
            serde_json::json!({ "workspace_id": "ws-1" }),
        );
        bridge.on_message(&msg).await;

        let admin_notifs = list_notifications(&state, "user-admin").await;
        assert_eq!(admin_notifs.len(), 1);
        assert_eq!(
            admin_notifs[0].notification_type,
            NotificationType::MetaSpecDrift
        );
        assert_eq!(admin_notifs[0].priority, 6);

        let dev_notifs = list_notifications(&state, "user-dev").await;
        assert_eq!(dev_notifs.len(), 1);

        // Viewer role is excluded from workspace-level notification fan-out.
        let viewer_notifs = list_notifications(&state, "user-viewer").await;
        assert!(viewer_notifs.is_empty());
    }

    #[tokio::test]
    async fn notification_bridge_creates_budget_warning_notification() {
        let state = crate::mem::test_state();
        seed_workspace(&state, "ws-1", "tenant-1").await;
        seed_member(&state, "ws-1", "user-1", WorkspaceRole::Owner).await;

        let bridge = NotificationBridge::new(state.clone());
        let msg = test_message(
            MessageKind::BudgetWarning,
            "ws-1",
            serde_json::json!({
                "agent_id": "agent-1",
                "workspace_id": "ws-1",
                "usage_pct": 85.0,
            }),
        );
        bridge.on_message(&msg).await;

        let notifs = list_notifications(&state, "user-1").await;
        assert_eq!(notifs.len(), 1);
        assert_eq!(
            notifs[0].notification_type,
            NotificationType::BudgetWarning
        );
        assert_eq!(notifs[0].priority, 7);
    }

    #[tokio::test]
    async fn notification_bridge_creates_agent_error_notification() {
        let state = crate::mem::test_state();
        seed_workspace(&state, "ws-1", "tenant-1").await;
        seed_agent(&state, "agent-1", "ws-1", Some("user-1")).await;

        let bridge = NotificationBridge::new(state.clone());
        let msg = test_message(
            MessageKind::AgentError,
            "ws-1",
            serde_json::json!({
                "agent_id": "agent-1",
                "error": "container exited with code 137",
            }),
        );
        bridge.on_message(&msg).await;

        let notifs = list_notifications(&state, "user-1").await;
        assert_eq!(notifs.len(), 1);
        assert_eq!(
            notifs[0].notification_type,
            NotificationType::AgentEscalation
        );
        assert_eq!(notifs[0].priority, 5);
        assert_eq!(notifs[0].entity_ref.as_deref(), Some("agent-1"));
    }

    #[tokio::test]
    async fn notification_bridge_skips_unknown_workspace() {
        let state = crate::mem::test_state();
        let bridge = NotificationBridge::new(state.clone());
        let msg = test_message(
            MessageKind::ReconciliationCompleted,
            "ws-does-not-exist",
            serde_json::json!({}),
        );
        bridge.on_message(&msg).await;
        let notifs = list_notifications(&state, "user-anything").await;
        assert!(notifs.is_empty());
    }

    #[tokio::test]
    async fn dispatcher_dispatches_to_all_consumers() {
        #[derive(Default)]
        struct CountingConsumer {
            count: std::sync::atomic::AtomicUsize,
        }
        #[async_trait::async_trait]
        impl MessageConsumer for CountingConsumer {
            async fn on_message(&self, _message: &Message) {
                self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }

        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let consumer = Arc::new(CountingConsumer::default());
        let dispatcher = MessageDispatcher::new(vec![consumer.clone()]);
        let handle = tokio::spawn(async move {
            dispatcher.run(rx).await;
        });

        tx.send(test_message(MessageKind::DataSeeded, "ws-x", serde_json::json!({})))
            .await
            .unwrap();
        tx.send(test_message(MessageKind::DataSeeded, "ws-x", serde_json::json!({})))
            .await
            .unwrap();
        drop(tx);

        handle.await.unwrap();
        assert_eq!(
            consumer.count.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "dispatcher must deliver both messages to the consumer"
        );
    }
}
