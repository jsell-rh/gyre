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
        "event_type": event.event_type.as_str(),
        "agent_id": event.agent_id.as_ref().map(|id| id.as_str()),
        "user_id": event.user_id.as_ref().map(|id| id.as_str()),
        "workspace_id": event.workspace_id.as_ref().map(|id| id.as_str()),
        "resource_type": event.resource_type,
        "resource_id": event.resource_id,
        "outcome": event.outcome.as_str(),
        "detail": event.detail,
        "source_ip": event.source_ip,
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
        // Source control events (observability.md §Audit Event Types).
        gyre_domain::AuditEventType::GitPushAccepted => 3,
        gyre_domain::AuditEventType::GitPushRejected => 5,
        gyre_domain::AuditEventType::GitClone => 3,
        gyre_domain::AuditEventType::SpecChanged => 4,
        // Access control events (observability.md §Audit Event Types).
        gyre_domain::AuditEventType::AuthSuccess => 3,
        gyre_domain::AuditEventType::AuthFailure => 6,
        gyre_domain::AuditEventType::AbacDenied => 5,
        gyre_domain::AuditEventType::RbacDenied => 5,
        gyre_domain::AuditEventType::ImpersonationStarted => 7,
        gyre_domain::AuditEventType::ImpersonationEnded => 7,
        // Agent lifecycle events (observability.md §Audit Event Types):
        // admin interventions rank highest, detector-marked deaths next,
        // routine lifecycle signals stay informational.
        gyre_domain::AuditEventType::AgentKilled => 7,
        gyre_domain::AuditEventType::AgentDead => 6,
        gyre_domain::AuditEventType::AgentTokenRevoked => 5,
        gyre_domain::AuditEventType::AgentReassigned => 4,
        gyre_domain::AuditEventType::AgentSpawned => 3,
        gyre_domain::AuditEventType::AgentCompleted => 3,
        gyre_domain::AuditEventType::AgentHeartbeat => 3,
        gyre_domain::AuditEventType::Custom(_) => 3,
    };
    let event_type = event.event_type.as_str();
    let mut extensions = format!(
        "ts={} resourceType={} outcome={}",
        event.timestamp,
        cef_escape(&event.resource_type),
        event.outcome.as_str()
    );
    if let Some(agent_id) = &event.agent_id {
        extensions.push_str(&format!(" agentId={}", agent_id.as_str()));
    }
    if let Some(user_id) = &event.user_id {
        extensions.push_str(&format!(" userId={}", user_id.as_str()));
    }
    if let Some(workspace_id) = &event.workspace_id {
        extensions.push_str(&format!(" workspaceId={}", workspace_id.as_str()));
    }
    if let Some(resource_id) = &event.resource_id {
        extensions.push_str(&format!(" resourceId={}", cef_escape(resource_id)));
    }
    if let Some(source_ip) = &event.source_ip {
        extensions.push_str(&format!(" src={}", cef_escape(source_ip)));
    }
    if let Some(path) = event.detail.get("path").and_then(|v| v.as_str()) {
        extensions.push_str(&format!(" filePath={}", cef_escape(path)));
    }
    if let Some(pid) = event.detail.get("pid").and_then(|v| v.as_u64()) {
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
            et,
            Some(Id::new("agent-1")),
            None,
            None,
            Some(Id::new("ws-1")),
            None,
            "container".to_string(),
            Some("abc123".to_string()),
            gyre_domain::AuditOutcome::Success,
            serde_json::json!({ "mode": "read", "path": "/tmp/test", "pid": 4321 }),
            Some("10.0.0.1".to_string()),
            None,
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
        assert!(msg.contains("\"outcome\":\"success\""));
        assert!(msg.contains("\"resource_type\":\"container\""));
        assert!(msg.contains("\"workspace_id\":\"ws-1\""));
    }

    #[test]
    fn cef_format_structure() {
        let event = make_event("e1", AuditEventType::NetworkConnect);
        let msg = format_cef(&event);
        assert!(msg.starts_with("CEF:0|Gyre|gyre-server|"));
        assert!(msg.contains("network_connect"));
        assert!(msg.contains("agentId=agent-1"));
        assert!(msg.contains("resourceType=container"));
        assert!(msg.contains("outcome=success"));
        assert!(msg.contains("workspaceId=ws-1"));
        assert!(msg.contains("filePath="));
        assert!(msg.contains("pid=4321"));
        assert!(msg.contains("src=10.0.0.1"));
    }

    #[test]
    fn cef_format_process_exec_severity() {
        let event = make_event("e1", AuditEventType::ProcessExec);
        let msg = format_cef(&event);
        assert!(msg.contains("|7|"));
    }

    #[test]
    fn cef_severity_source_control_and_access_control_events() {
        // observability.md §Audit Event Types severity mapping (task-104):
        // AuthFailure 6, AbacDenied/RbacDenied/GitPushRejected 5,
        // AuthSuccess/GitPushAccepted/GitClone 3, ImpersonationStarted/
        // ImpersonationEnded 7, SpecChanged 4.
        let cases = [
            (AuditEventType::AuthFailure, 6),
            (AuditEventType::AbacDenied, 5),
            (AuditEventType::RbacDenied, 5),
            (AuditEventType::GitPushRejected, 5),
            (AuditEventType::AuthSuccess, 3),
            (AuditEventType::GitPushAccepted, 3),
            (AuditEventType::GitClone, 3),
            (AuditEventType::ImpersonationStarted, 7),
            (AuditEventType::ImpersonationEnded, 7),
            (AuditEventType::SpecChanged, 4),
        ];
        for (event_type, expected) in &cases {
            let event = make_event("e1", event_type.clone());
            let msg = format_cef(&event);
            // Severity is the 7th CEF header field (index 6):
            // CEF:0|Vendor|Product|Version|Signature|Name|<severity>|ext
            let header = msg.split('|').collect::<Vec<_>>();
            assert!(header.len() >= 8, "CEF header must have 8 fields");
            assert_eq!(
                header[6],
                expected.to_string(),
                "wrong CEF severity for {}: expected {}, got {}",
                event_type.as_str(),
                expected,
                header[7]
            );
        }
    }

    #[test]
    fn cef_escape_pipes() {
        assert_eq!(cef_escape("foo|bar"), "foo\\|bar");
        assert_eq!(cef_escape("a=b"), "a\\=b");
    }

    #[test]
    fn cef_agent_lifecycle_severities() {
        // observability.md §Audit Event Types severity mapping: admin kill 7,
        // detector-marked death 6, token revocation 5, reassignment 4,
        // routine lifecycle signals 3.
        let cases = [
            (AuditEventType::AgentKilled, 7),
            (AuditEventType::AgentDead, 6),
            (AuditEventType::AgentTokenRevoked, 5),
            (AuditEventType::AgentReassigned, 4),
            (AuditEventType::AgentSpawned, 3),
            (AuditEventType::AgentCompleted, 3),
            (AuditEventType::AgentHeartbeat, 3),
        ];
        for (et, want) in cases {
            let ev = make_event("e1", et.clone());
            let cef = format_cef(&ev);
            let severity: u8 = cef
                .trim_end()
                .split('|')
                .nth(6)
                .unwrap_or_else(|| panic!("malformed CEF: {cef}"))
                .parse()
                .unwrap_or_else(|_| panic!("non-numeric severity in CEF: {cef}"));
            assert_eq!(severity, want, "wrong CEF severity for {et:?}: {cef}");
        }
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

    #[tokio::test]
    async fn forward_cycle_sends_decompressed_detail_for_compressed_events() {
        // observability.md §Log Retention & Compression: SIEM forwarding
        // must receive the original detail payload even after the
        // log-compression job has zstd-compressed the stored row. The
        // forwarder reads through `since_timestamp`, which decompresses
        // in the adapter — this test proves the wire payload.
        use crate::mem::test_state;
        use tokio::io::AsyncReadExt;

        let state = test_state();

        // Local syslog receiver.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let receiver = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                match tokio::time::timeout(std::time::Duration::from_secs(5), sock.read(&mut chunk))
                    .await
                {
                    Ok(Ok(0)) | Err(_) => break,
                    Ok(Ok(n)) => buf.extend_from_slice(&chunk[..n]),
                    Ok(Err(e)) => panic!("read error: {e}"),
                }
            }
            buf
        });

        state
            .siem_store
            .add(SiemTarget {
                id: "t1".to_string(),
                name: "local-syslog".to_string(),
                target_type: TargetType::Syslog,
                config: serde_json::json!({
                    "host": "127.0.0.1",
                    "port": port,
                    "format": "json"
                }),
                enabled: true,
            })
            .await;

        // One event, aged past the 7-day compression threshold, then
        // compressed by the same port call the job uses.
        let detail = serde_json::json!({
            "path": "/etc/shadow",
            "mode": "read",
            "pid": 4321
        });
        let event = make_event("e-compressed", AuditEventType::FileAccess);
        let mut event = event;
        event.detail = detail.clone();
        event.timestamp = 100;
        gyre_ports::AuditRepository::record(&*state.audit, &event)
            .await
            .unwrap();
        let stats = gyre_ports::AuditRepository::compress_details_before(&*state.audit, 200, 10)
            .await
            .unwrap();
        assert_eq!(
            stats.compressed, 1,
            "event must be compressed before forwarding"
        );

        run_forward_cycle(&state).await.unwrap();

        let received = receiver.await.unwrap();
        let text = String::from_utf8(received).unwrap();
        assert!(text.contains("file_access"), "got: {text}");
        assert!(
            text.contains("\"path\":\"/etc/shadow\""),
            "forwarded payload must carry the decompressed detail: {text}"
        );
        assert!(text.contains("\"pid\":4321"), "got: {text}");

        // The cursor advanced past the forwarded event.
        assert!(state.siem_store.last_forwarded_ts().await >= event.timestamp);
    }
}
