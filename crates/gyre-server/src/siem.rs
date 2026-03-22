//! SIEM (Security Information and Event Management) forwarding.
//!
//! Forwards audit events to external SIEM targets via syslog (RFC 5424) or webhook (HTTP POST).
//! Events can be formatted in JSON or CEF (Common Event Format).

use anyhow::Result;
use gyre_domain::AuditEvent;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use crate::AppState;

// ── Domain ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TargetType {
    Syslog,
    Webhook,
}

impl std::fmt::Display for TargetType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Syslog => write!(f, "syslog"),
            Self::Webhook => write!(f, "webhook"),
        }
    }
}

impl TargetType {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "syslog" => Some(Self::Syslog),
            "webhook" => Some(Self::Webhook),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    Json,
    Cef,
}

impl OutputFormat {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "cef" => Self::Cef,
            _ => Self::Json,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiemTarget {
    pub id: String,
    pub name: String,
    pub target_type: TargetType,
    /// Configuration for this target (keys depend on target_type).
    /// Syslog: { "host": "...", "port": 514 }
    /// Webhook: { "url": "...", "auth_header": "Bearer ..." }
    pub config: serde_json::Value,
    pub enabled: bool,
}

// ── Store ─────────────────────────────────────────────────────────────────────

/// In-memory SIEM target store. Targets survive process lifetime only (no DB persistence needed
/// for MVP since they're configured via env/admin API at startup).
#[derive(Clone, Default)]
pub struct SiemStore {
    targets: Arc<Mutex<HashMap<String, SiemTarget>>>,
    /// Unix epoch seconds of the last event we forwarded.
    last_forwarded_ts: Arc<Mutex<u64>>,
}

impl SiemStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn add(&self, target: SiemTarget) {
        self.targets.lock().await.insert(target.id.clone(), target);
    }

    pub async fn list(&self) -> Vec<SiemTarget> {
        self.targets.lock().await.values().cloned().collect()
    }

    pub async fn get(&self, id: &str) -> Option<SiemTarget> {
        self.targets.lock().await.get(id).cloned()
    }

    pub async fn update(&self, target: SiemTarget) -> bool {
        let mut targets = self.targets.lock().await;
        if targets.contains_key(&target.id) {
            targets.insert(target.id.clone(), target);
            true
        } else {
            false
        }
    }

    pub async fn remove(&self, id: &str) -> bool {
        self.targets.lock().await.remove(id).is_some()
    }

    pub async fn last_forwarded_ts(&self) -> u64 {
        *self.last_forwarded_ts.lock().await
    }

    pub async fn set_last_forwarded_ts(&self, ts: u64) {
        *self.last_forwarded_ts.lock().await = ts;
    }
}

// ── Formatting ────────────────────────────────────────────────────────────────

/// Format an audit event as RFC 5424 syslog message.
///
/// Priority 134 = facility 16 (local0) + severity 6 (informational).
pub fn format_syslog(event: &AuditEvent) -> String {
    let ts = chrono_like_iso8601(event.timestamp);
    let msg = serde_json::json!({
        "id": event.id.as_str(),
        "agent_id": event.agent_id.as_str(),
        "event_type": event.event_type.as_str(),
        "path": event.path,
        "pid": event.pid,
        "details": event.details,
    });
    format!(
        "<134>1 {} gyre - - - {} {}\n",
        ts,
        event.event_type.as_str(),
        msg
    )
}

/// Format an audit event as CEF (Common Event Format).
///
/// CEF:Version|Device Vendor|Device Product|Device Version|Signature ID|Name|Severity|Extensions
pub fn format_cef(event: &AuditEvent) -> String {
    let severity = match event.event_type {
        gyre_domain::AuditEventType::ProcessExec => 7,
        gyre_domain::AuditEventType::NetworkConnect => 5,
        gyre_domain::AuditEventType::FileAccess => 3,
        gyre_domain::AuditEventType::Syscall => 5,
        gyre_domain::AuditEventType::ContainerStarted => 3,
        gyre_domain::AuditEventType::ContainerStopped => 3,
        gyre_domain::AuditEventType::ContainerCrashed => 7,
        gyre_domain::AuditEventType::ContainerOom => 8,
        gyre_domain::AuditEventType::ContainerNetworkBlocked => 6,
        gyre_domain::AuditEventType::Custom(_) => 3,
    };
    let event_type = event.event_type.as_str();
    let mut extensions = format!("agentId={} ts={}", event.agent_id.as_str(), event.timestamp);
    if let Some(ref path) = event.path {
        extensions.push_str(&format!(" filePath={}", cef_escape(path)));
    }
    if let Some(pid) = event.pid {
        extensions.push_str(&format!(" pid={}", pid));
    }
    format!(
        "CEF:0|Gyre|gyre-server|0.1.0|{}|{}|{}|{}\n",
        cef_escape(&event_type),
        cef_escape(&event_type),
        severity,
        extensions
    )
}

fn cef_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('=', "\\=")
        .replace('\n', "\\n")
}

/// Produce a simple ISO 8601-like timestamp from unix epoch seconds.
fn chrono_like_iso8601(secs: u64) -> String {
    // Without chrono: format as "1970-01-01T00:00:00Z" approximation
    // We just use the raw seconds for simplicity; real deployments would use chrono.
    format!("{}", secs)
}

// ── Forwarding ────────────────────────────────────────────────────────────────

/// Forward a batch of events to a single SIEM target.
pub async fn forward_to_target(
    target: &SiemTarget,
    events: &[AuditEvent],
    http_client: &reqwest::Client,
) -> Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    match target.target_type {
        TargetType::Syslog => forward_syslog(target, events).await,
        TargetType::Webhook => forward_webhook(target, events, http_client).await,
    }
}

async fn forward_syslog(target: &SiemTarget, events: &[AuditEvent]) -> Result<()> {
    let host = target
        .config
        .get("host")
        .and_then(|v| v.as_str())
        .unwrap_or("127.0.0.1");
    let port = target
        .config
        .get("port")
        .and_then(|v| v.as_u64())
        .unwrap_or(514);
    let format = target
        .config
        .get("format")
        .and_then(|v| v.as_str())
        .map(OutputFormat::from_str)
        .unwrap_or(OutputFormat::Json);

    let addr = format!("{}:{}", host, port);
    let mut stream = tokio::net::TcpStream::connect(&addr).await?;

    for event in events {
        let msg = match format {
            OutputFormat::Cef => format_cef(event),
            OutputFormat::Json => format_syslog(event),
        };
        stream.write_all(msg.as_bytes()).await?;
    }
    stream.flush().await?;
    Ok(())
}

async fn forward_webhook(
    target: &SiemTarget,
    events: &[AuditEvent],
    http_client: &reqwest::Client,
) -> Result<()> {
    let url = target
        .config
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("webhook target missing 'url'"))?;
    let format = target
        .config
        .get("format")
        .and_then(|v| v.as_str())
        .map(OutputFormat::from_str)
        .unwrap_or(OutputFormat::Json);

    let body = match format {
        OutputFormat::Json => serde_json::to_string(events)?,
        OutputFormat::Cef => events.iter().map(format_cef).collect::<Vec<_>>().join(""),
    };

    let mut req = http_client
        .post(url)
        .header("Content-Type", "application/json")
        .body(body);

    if let Some(auth) = target.config.get("auth_header").and_then(|v| v.as_str()) {
        req = req.header("Authorization", auth);
    }

    let resp = req.send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("webhook {} returned HTTP {}", url, resp.status());
    }
    Ok(())
}

// ── Background job ────────────────────────────────────────────────────────────

/// Spawn the SIEM forwarder background task that runs every 10 seconds.
pub fn spawn_siem_forwarder(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
        loop {
            interval.tick().await;
            if let Err(e) = run_forward_cycle(&state).await {
                error!("SIEM forward cycle error: {:#}", e);
            }
        }
    });
}

async fn run_forward_cycle(state: &AppState) -> Result<()> {
    let targets = state.siem_store.list().await;
    let enabled: Vec<_> = targets.into_iter().filter(|t| t.enabled).collect();
    if enabled.is_empty() {
        return Ok(());
    }

    let last_ts = state.siem_store.last_forwarded_ts().await;
    let events = state.audit.since_timestamp(last_ts, 1000).await?;
    if events.is_empty() {
        return Ok(());
    }

    let max_ts = events.iter().map(|e| e.timestamp).max().unwrap_or(last_ts);

    for target in &enabled {
        match forward_to_target(target, &events, &state.http_client).await {
            Ok(()) => info!(
                target = %target.name,
                count = events.len(),
                "SIEM events forwarded"
            ),
            Err(e) => warn!(
                target = %target.name,
                error = %e,
                "SIEM forwarding failed"
            ),
        }
    }

    state.siem_store.set_last_forwarded_ts(max_ts).await;
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::Id;
    use gyre_domain::{AuditEvent, AuditEventType};

    fn make_event(id: &str, et: AuditEventType) -> AuditEvent {
        AuditEvent::new(
            Id::new(id),
            Id::new("agent-1"),
            et,
            Some("/tmp/test".to_string()),
            serde_json::json!({ "mode": "read" }),
            Some(4321),
            1704067200,
        )
    }

    #[test]
    fn syslog_format_contains_event_type() {
        let event = make_event("e1", AuditEventType::FileAccess);
        let msg = format_syslog(&event);
        assert!(msg.starts_with("<134>1 "));
        assert!(msg.contains("file_access"));
        assert!(msg.contains("agent-1"));
    }

    #[test]
    fn cef_format_structure() {
        let event = make_event("e1", AuditEventType::NetworkConnect);
        let msg = format_cef(&event);
        assert!(msg.starts_with("CEF:0|Gyre|gyre-server|"));
        assert!(msg.contains("network_connect"));
        assert!(msg.contains("agentId=agent-1"));
        assert!(msg.contains("filePath="));
        assert!(msg.contains("pid=4321"));
    }

    #[test]
    fn cef_format_process_exec_severity() {
        let event = make_event("e1", AuditEventType::ProcessExec);
        let msg = format_cef(&event);
        assert!(msg.contains("|7|"));
    }

    #[test]
    fn cef_escape_pipes() {
        assert_eq!(cef_escape("foo|bar"), "foo\\|bar");
        assert_eq!(cef_escape("a=b"), "a\\=b");
    }

    #[tokio::test]
    async fn siem_store_crud() {
        let store = SiemStore::new();
        let t = SiemTarget {
            id: "t1".to_string(),
            name: "test-target".to_string(),
            target_type: TargetType::Webhook,
            config: serde_json::json!({ "url": "http://example.com/siem" }),
            enabled: true,
        };
        store.add(t.clone()).await;

        let list = store.list().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "test-target");

        let got = store.get("t1").await.unwrap();
        assert_eq!(got.id, "t1");

        let mut updated = t.clone();
        updated.enabled = false;
        assert!(store.update(updated).await);
        assert!(!store.get("t1").await.unwrap().enabled);

        assert!(store.remove("t1").await);
        assert!(store.list().await.is_empty());
    }

    #[tokio::test]
    async fn siem_store_last_ts() {
        let store = SiemStore::new();
        assert_eq!(store.last_forwarded_ts().await, 0);
        store.set_last_forwarded_ts(12345).await;
        assert_eq!(store.last_forwarded_ts().await, 12345);
    }

    #[test]
    fn webhook_target_type_roundtrip() {
        assert_eq!(TargetType::from_str("syslog"), Some(TargetType::Syslog));
        assert_eq!(TargetType::from_str("webhook"), Some(TargetType::Webhook));
        assert_eq!(TargetType::from_str("unknown"), None);
    }
}
