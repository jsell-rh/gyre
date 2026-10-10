//! Data retention policies: define max age per data type and run cleanup.
//!
//! Implements business-continuity.md §5: seven data types with spec defaults,
//! policies persisted via the KV store (survive restart), and a nightly
//! cleanup job that hard-deletes data past policy age.
//!
//! Per-type enforcement:
//! - `activity_events` — lives only in `TelemetryBuffer`, a bounded ring
//!   buffer (per-workspace + per-server caps, gyre-common/src/message.rs).
//!   Boundedness IS the retention enforcement; no separate purge path exists
//!   (the `ActivityRepository` port is not wired into `AppState`). Documented
//!   here rather than faked.
//! - `agent_logs` — in-memory `HashMap<agent, Vec<String>>` with canonical
//!   `[ts] message` lines (api/agent_logs.rs). Entries past the cutoff are
//!   drained. "Compress after 7 days" (spec) is a size optimization, not a
//!   retention requirement — not implemented; the 30-day purge is.
//! - `audit_events` / `analytics_events` — hard DELETE via the repository
//!   ports (`delete_older_than`), SQLite + Postgres adapters.
//! - `snapshots` — tiered file policy `24h×24 + 7d×7 + 4w×4` on
//!   `snapshot_dir()` files, oldest-first deletion beyond tier caps.
//! - `attestations` — never purged (`max_age_days: u64::MAX`); the
//!   attestation store has no delete path (non-repudiation).
//! - `notifications` — read (resolved or dismissed) older than
//!   `max_age_days_read` (default 90d), unread older than `max_age_days`
//!   (default 365d).

use anyhow::Result;
use gyre_ports::KvJsonStore;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

use crate::AppState;

/// KV namespace + key the policy list is persisted under.
const KV_NAMESPACE: &str = "retention";
const KV_KEY: &str = "policies";

/// Snapshot tier policy (business-continuity.md §5): keep at most 24
/// snapshots with mtime within the last 24h, 7 within the last 7 days, and 4
/// within the last 4 weeks; delete everything else.
const SNAPSHOT_TIER_24H_KEEP: usize = 24;
const SNAPSHOT_TIER_7D_KEEP: usize = 7;
const SNAPSHOT_TIER_4W_KEEP: usize = 4;

#[derive(Clone, Serialize, Deserialize)]
pub struct RetentionPolicy {
    pub data_type: String,
    pub max_age_days: u64,
    /// Read-notification cutoff in days. Only meaningful for `notifications`:
    /// read (resolved or dismissed) notifications are purged after this many
    /// days, unread ones after `max_age_days`. `None` elsewhere.
    #[serde(default)]
    pub max_age_days_read: Option<u64>,
}

/// Default retention policies — the seven data types from
/// business-continuity.md §5.
pub fn default_policies() -> Vec<RetentionPolicy> {
    vec![
        RetentionPolicy {
            data_type: "activity_events".to_string(),
            max_age_days: 90,
            max_age_days_read: None,
        },
        RetentionPolicy {
            data_type: "agent_logs".to_string(),
            max_age_days: 30,
            max_age_days_read: None,
        },
        RetentionPolicy {
            data_type: "audit_events".to_string(),
            max_age_days: 365,
            max_age_days_read: None,
        },
        // Enforced structurally by the tiered snapshot policy, not by age.
        RetentionPolicy {
            data_type: "snapshots".to_string(),
            max_age_days: u64::MAX,
            max_age_days_read: None,
        },
        // Never purged (non-repudiation).
        RetentionPolicy {
            data_type: "attestations".to_string(),
            max_age_days: u64::MAX,
            max_age_days_read: None,
        },
        RetentionPolicy {
            data_type: "notifications".to_string(),
            max_age_days: 365,
            max_age_days_read: Some(90),
        },
        RetentionPolicy {
            data_type: "analytics_events".to_string(),
            max_age_days: 365,
            max_age_days_read: None,
        },
    ]
}

/// Retention policy store, optionally backed by a KV store.
///
/// `new()` seeds the spec defaults in memory; `init()` loads previously
/// persisted policies from the KV store (or persists defaults on first boot).
/// `update`/`set_policy` persist best-effort so admin edits survive restart.
#[derive(Clone)]
pub struct RetentionStore {
    policies: Arc<RwLock<Vec<RetentionPolicy>>>,
    /// Placeholder (`MemKvStore`) until `init` installs the real store.
    kv: Arc<RwLock<Arc<dyn KvJsonStore>>>,
}

impl RetentionStore {
    pub fn new() -> Self {
        Self {
            policies: Arc::new(RwLock::new(default_policies())),
            kv: Arc::new(RwLock::new(Arc::new(crate::mem::MemKvStore::default()) as Arc<dyn KvJsonStore>)),
        }
    }

    /// Load policies from the KV store, or persist the spec defaults on
    /// first boot. Called once from main after `build_state`.
    pub async fn init(&self, kv: Arc<dyn KvJsonStore>) {
        match kv.kv_get(KV_NAMESPACE, KV_KEY).await {
            Ok(Some(json)) => {
                if let Ok(policies) = serde_json::from_str::<Vec<RetentionPolicy>>(&json) {
                    if !policies.is_empty() {
                        *self.policies.write() = policies;
                    }
                }
            }
            _ => {}
        }
        *self.kv.write() = kv;
        // First boot or empty/corrupt blob: persist whatever we now hold
        // (loaded policies or defaults).
        self.persist().await;
    }

    async fn persist(&self) {
        let json = match serde_json::to_string(&*self.policies.read()) {
            Ok(json) => json,
            Err(_) => return, // RetentionPolicy is plain data; cannot fail
        };
        let kv = Arc::clone(&self.kv.read());
        if let Err(e) = kv.kv_set(KV_NAMESPACE, KV_KEY, json).await {
            tracing::warn!(error = %e, "failed to persist retention policies");
        }
    }

    pub fn list(&self) -> Vec<RetentionPolicy> {
        self.policies.read().clone()
    }

    /// Replace all policies. Caller (admin PUT) validates shape.
    pub fn update(&self, new_policies: Vec<RetentionPolicy>) {
        *self.policies.write() = new_policies;
        self.persist_best_effort();
    }

    pub fn set_policy(&self, data_type: &str, max_age_days: u64) {
        let mut policies = self.policies.write();
        if let Some(p) = policies.iter_mut().find(|p| p.data_type == data_type) {
            p.max_age_days = max_age_days;
        } else {
            policies.push(RetentionPolicy {
                data_type: data_type.to_string(),
                max_age_days,
                max_age_days_read: None,
            });
        }
        drop(policies);
        self.persist_best_effort();
    }

    /// Fire-and-forget persistence for the sync mutation paths.
    /// `update`/`set_policy` keep their sync signatures (admin handlers);
    /// the KV write is spawned onto the current runtime, if any. Outside a
    /// runtime (plain unit tests) the write is skipped — the in-memory
    /// policies stay authoritative for the process lifetime anyway.
    fn persist_best_effort(&self) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        let policies = self.policies.read().clone();
        let kv = Arc::clone(&self.kv.read());
        tokio::spawn(async move {
            if let Ok(json) = serde_json::to_string(&policies) {
                if let Err(e) = kv.kv_set(KV_NAMESPACE, KV_KEY, json).await {
                    tracing::warn!(error = %e, "failed to persist retention policies");
                }
            }
        });
    }
}

impl RetentionStore {
    /// Run retention cleanup against the live stores in `state`.
    ///
    /// Idempotent: every purge is a "delete rows/files older than cutoff"
    /// operation, so a second consecutive run deletes nothing new.
    pub async fn run_cleanup(&self, state: &AppState) -> Result<()> {
        let now = now_secs();
        for policy in self.list() {
            // u64::MAX means "never purge" (attestations; snapshots are
            // tier-enforced instead of age-enforced below).
            let max_age_secs = policy.max_age_days.saturating_mul(86_400);
            let cutoff = now.saturating_sub(max_age_secs);

            let purged = match policy.data_type.as_str() {
                "audit_events" => state.audit.delete_older_than(cutoff).await?,
                "analytics_events" => state.analytics.delete_older_than(cutoff).await?,
                "notifications" => {
                    let read_cutoff = match policy.max_age_days_read {
                        Some(days) => {
                            let s = days.saturating_mul(86_400);
                            now.saturating_sub(s)
                        }
                        None => cutoff,
                    };
                    state
                        .notifications
                        .delete_older_than(read_cutoff, cutoff)
                        .await?
                }
                "agent_logs" => {
                    let purged = purge_agent_logs(state, cutoff).await;
                    purged
                }
                "snapshots" => {
                    let purged = purge_snapshots().await;
                    purged
                }
                "attestations" => {
                    // Never purged (non-repudiation, business-continuity §5).
                    // The AttestationRepository port has no delete path.
                    info!(data_type = "attestations", "retention: never purge");
                    0
                }
                "activity_events" => {
                    // Activity events live only in the bounded TelemetryBuffer
                    // ring (per-workspace + per-server caps) — boundedness is
                    // the enforcement; there is no unbounded store to purge.
                    info!(
                        data_type = "activity_events",
                        "retention: bounded by TelemetryBuffer ring capacity"
                    );
                    0
                }
                _ => 0,
            };
            if purged > 0 || policy.max_age_days != u64::MAX {
                info!(
                    data_type = %policy.data_type,
                    purged,
                    "retention cleanup applied"
                );
            }
        }
        Ok(())
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Drain agent-log lines whose `[ts]` prefix is older than `cutoff`.
/// Lines without a parseable prefix are kept (conservative — cannot prove
/// age, do not delete). Returns lines purged.
async fn purge_agent_logs(state: &AppState, cutoff: u64) -> u64 {
    let mut logs = state.agent_logs.lock().await;
    let mut purged = 0u64;
    for buf in logs.values_mut() {
        let before = buf.len();
        buf.retain(|line| !agent_log_line_is_old(line, cutoff));
        purged += (before - buf.len()) as u64;
    }
    purged
}

/// A line is old when it starts with `[ts]` and ts < cutoff.
fn agent_log_line_is_old(line: &str, cutoff: u64) -> bool {
    let Some(ts) = line
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .and_then(|(ts, _)| ts.trim().parse::<u64>().ok())
    else {
        return false;
    };
    ts < cutoff
}

/// Apply the tiered snapshot policy `24h×24 + 7d×7 + 4w×4` to the snapshot
/// directory. Non-snapshot files are left alone. Returns files deleted.
async fn purge_snapshots() -> u64 {
    let dir = crate::snapshot::snapshot_dir();
    let mut files: Vec<(String, u64)> = Vec::new();
    let mut entries = match tokio::fs::read_dir(&dir).await {
        Ok(entries) => entries,
        Err(_) => return 0, // no snapshot dir yet — nothing to purge
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(meta) = entry.metadata().await else {
            continue;
        };
        let Ok(mtime) = meta.modified() else {
            continue;
        };
        let mtime_secs = mtime
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        files.push((name.to_string(), mtime_secs));
    }

    let now = now_secs();
    let victims = snapshot_tier_deletions(now, &files);
    for name in &victims {
        if let Err(e) = tokio::fs::remove_file(dir.join(name)).await {
            tracing::warn!(file = %name, error = %e, "failed to delete snapshot");
        }
    }
    victims.len() as u64
}

/// Pure tiered-policy decision: given `now` and snapshot files `(name, mtime)`,
/// return the file names to delete. Keep at most 24 snapshots with mtime in
/// the last 24h, at most 7 within the last 7 days, at most 4 within the last
/// 4 weeks; delete the rest, oldest first.
fn snapshot_tier_deletions(now_secs: u64, files: &[(String, u64)]) -> Vec<String> {
    // Sort newest-first; within a tier, the newest entries are kept.
    let mut sorted: Vec<&(String, u64)> = files.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1)); // newest first

    let mut deletions = Vec::new();
    let mut kept_24h = 0usize;
    let mut kept_7d = 0usize;
    let mut kept_4w = 0usize;
    let day = 86_400u64;

    for (name, mtime) in sorted {
        let age = now_secs.saturating_sub(*mtime);
        if age <= day && kept_24h < SNAPSHOT_TIER_24H_KEEP {
            kept_24h += 1;
        } else if age <= 7 * day && kept_7d < SNAPSHOT_TIER_7D_KEEP {
            kept_7d += 1;
        } else if age <= 28 * day && kept_4w < SNAPSHOT_TIER_4W_KEEP {
            kept_4w += 1;
        } else {
            deletions.push(name.clone());
        }
    }
    deletions
}

#[cfg(test)]
mod tests {
    use crate::jobs::next_daily_run_secs;
    use super::*;
    use crate::mem::test_state;
    use gyre_common::{Id, Notification, NotificationType};
    use gyre_domain::{AnalyticsEvent, AttestationBundle, AuditEvent, AuditEventType, MergeAttestation};
    use std::sync::Arc;

    const DAY: u64 = 86_400;

    fn now() -> u64 {
        now_secs()
    }

    #[test]
    fn default_policies_present() {
        let policies = default_policies();
        assert_eq!(policies.len(), 7);
        let types: Vec<&str> = policies.iter().map(|p| p.data_type.as_str()).collect();
        for expected in [
            "activity_events",
            "agent_logs",
            "audit_events",
            "snapshots",
            "attestations",
            "notifications",
            "analytics_events",
        ] {
            assert!(types.contains(&expected), "missing policy {expected}");
        }
        // No cost_entries policy — spec lists exactly these 7 types.
        assert!(!types.contains(&"cost_entries"));
        // Notifications: 365d unread / 90d read split (§5).
        let notif = policies.iter().find(|p| p.data_type == "notifications").unwrap();
        assert_eq!(notif.max_age_days, 365);
        assert_eq!(notif.max_age_days_read, Some(90));
        // Attestations and snapshots are never age-purged.
        assert_eq!(
            policies.iter().find(|p| p.data_type == "attestations").unwrap().max_age_days,
            u64::MAX
        );
    }

    #[test]
    fn update_policies() {
        let store = RetentionStore::new();
        let mut policies = default_policies();
        policies[0].max_age_days = 10;
        store.update(policies);
        assert_eq!(store.list()[0].max_age_days, 10);
    }

    #[test]
    fn set_existing_policy() {
        let store = RetentionStore::new();
        store.set_policy("audit_events", 30);
        let p = store.list().into_iter().find(|p| p.data_type == "audit_events").unwrap();
        assert_eq!(p.max_age_days, 30);
    }

    #[test]
    fn set_new_policy() {
        let store = RetentionStore::new();
        store.set_policy("custom_type", 7);
        let p = store.list().into_iter().find(|p| p.data_type == "custom_type").unwrap();
        assert_eq!(p.max_age_days, 7);
    }

    #[tokio::test]
    async fn policies_persist_across_store_reinit() {
        let kv: Arc<dyn KvJsonStore> = Arc::new(crate::mem::MemKvStore::default());
        let store = RetentionStore::new();
        store.init(Arc::clone(&kv)).await;
        store.set_policy("audit_events", 42);
        // Give the spawned persist task a chance to run.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // A second store booting against the same KV must load 42, not the default.
        let store2 = RetentionStore::new();
        store2.init(kv).await;
        let p = store2
            .list()
            .into_iter()
            .find(|p| p.data_type == "audit_events")
            .unwrap();
        assert_eq!(p.max_age_days, 42);
    }

    fn audit_event(id: &str, ts: u64) -> AuditEvent {
        AuditEvent::new(
            Id::new(id),
            AuditEventType::FileAccess,
            Some(Id::new("agent-1")),
            None,
            None,
            None,
            None,
            "agent".to_string(),
            Some("agent-1".to_string()),
            gyre_domain::AuditOutcome::Success,
            serde_json::json!({ "path": "/tmp/x", "pid": 123 }),
            None,
            None,
            ts,
        )
    }

    fn analytics_event(id: &str, ts: u64) -> AnalyticsEvent {
        AnalyticsEvent::new(
            Id::new(id),
            "test_event",
            Some("agent-1".to_string()),
            serde_json::json!({}),
            ts,
        )
    }

    fn notification(id: &str, created_at: i64, read: bool) -> Notification {
        let mut n = Notification::new(
            Id::new(id),
            Id::new("ws-1"),
            Id::new("user-1"),
            NotificationType::GateFailure,
            "title",
            "tenant-1",
            created_at,
        );
        if read {
            n.resolved_at = Some(created_at + 10);
        }
        n
    }

    /// Seed every in-memory store with old (beyond policy) and new (within
    /// policy) data plus an ancient attestation that must survive.
    async fn seed_state(state: &AppState) {
        let t = now();
        // audit: 365d policy — purge older than 365d.
        gyre_ports::AuditRepository::record(&*state.audit, &audit_event("a-old", t - 400 * DAY))
            .await
            .unwrap();
        gyre_ports::AuditRepository::record(&*state.audit, &audit_event("a-new", t - 100 * DAY))
            .await
            .unwrap();
        // analytics: 365d policy.
        gyre_ports::AnalyticsRepository::record(
            &*state.analytics,
            &analytics_event("an-old", t - 400 * DAY),
        )
        .await
        .unwrap();
        gyre_ports::AnalyticsRepository::record(
            &*state.analytics,
            &analytics_event("an-new", t - 100 * DAY),
        )
        .await
        .unwrap();
        // notifications: read older than 90d purged; unread older than 365d
        // purged; read-but-recent and unread-recent kept.
        gyre_ports::NotificationRepository::create(
            &*state.notifications,
            &notification("n-old-read", (t - 100 * DAY) as i64, true),
        )
        .await
        .unwrap();
        gyre_ports::NotificationRepository::create(
            &*state.notifications,
            &notification("n-old-unread", (t - 400 * DAY) as i64, false),
        )
        .await
        .unwrap();
        gyre_ports::NotificationRepository::create(
            &*state.notifications,
            &notification("n-new-read", (t - 10 * DAY) as i64, true),
        )
        .await
        .unwrap();
        gyre_ports::NotificationRepository::create(
            &*state.notifications,
            &notification("n-new-unread", (t - 10 * DAY) as i64, false),
        )
        .await
        .unwrap();
        // agent_logs: 30d policy — one old line, one new line, one unprefixed.
        state.agent_logs.lock().await.insert(
            "agent-1".to_string(),
            vec![
                format!("[{}] old line", t - 40 * DAY),
                format!("[{}] new line", t - DAY),
                "no prefix line".to_string(),
            ],
        );
        // attestation: ancient, must never be purged.
        let bundle = AttestationBundle {
            attestation: MergeAttestation {
                attestation_version: 1,
                mr_id: "mr-ancient".to_string(),
                merge_commit_sha: "deadbeef".to_string(),
                merged_at: t - 10_000 * DAY,
                gate_results: vec![],
                spec_ref: None,
                spec_fully_approved: false,
                author_agent_id: None,
                conversation_sha: None,
                completion_summary: None,
                meta_specs_used: vec![],
            },
            signature: "sig".to_string(),
            signing_key_id: "kid".to_string(),
            deprecation_notice: None,
        };
        state.attestation_store.save("mr-ancient", &bundle).await.unwrap();
    }

    #[tokio::test]
    async fn run_cleanup_purges_old_and_keeps_new() {
        let state = test_state();
        seed_state(&state).await;

        state.retention_store.run_cleanup(&state).await.unwrap();

        // audit: old gone, new kept.
        let audit = gyre_ports::AuditRepository::query(&*state.audit, &gyre_ports::AuditQueryFilter { limit: 100, ..Default::default() })
            .await
            .unwrap();
        let ids: Vec<&str> = audit.iter().map(|e| e.id.as_str()).collect();
        assert!(!ids.contains(&"a-old"), "old audit event must be purged");
        assert!(ids.contains(&"a-new"), "new audit event must be kept");

        // analytics: old gone, new kept.
        let analytics =
            gyre_ports::AnalyticsRepository::query(&*state.analytics, None, None, 100)
                .await
                .unwrap();
        let ids: Vec<&str> = analytics.iter().map(|e| e.id.as_str()).collect();
        assert!(!ids.contains(&"an-old"), "old analytics event must be purged");
        assert!(ids.contains(&"an-new"), "new analytics event must be kept");

        // notifications: old-read and old-unread purged; both new kept.
        let notifs = gyre_ports::NotificationRepository::list_recent(&*state.notifications, 100)
            .await
            .unwrap();
        let ids: Vec<&str> = notifs.iter().map(|n| n.id.as_str()).collect();
        assert!(!ids.contains(&"n-old-read"), "old read notification must be purged");
        assert!(!ids.contains(&"n-old-unread"), "old unread notification must be purged");
        assert!(ids.contains(&"n-new-read"), "recent read notification must be kept");
        assert!(ids.contains(&"n-new-unread"), "recent unread notification must be kept");

        // agent_logs: old line drained; new line and unprefixed line kept.
        let logs = state.agent_logs.lock().await;
        let lines = logs.get("agent-1").unwrap();
        assert_eq!(lines.len(), 2, "old log line must be purged, others kept: {lines:?}");
        assert!(lines.iter().any(|l| l.contains("new line")));
        assert!(lines.iter().any(|l| l.contains("no prefix line")));
        drop(logs);

        // attestation: ancient bundle survives (never purged).
        assert!(
            state.attestation_store.find_by_mr_id("mr-ancient").await.unwrap().is_some(),
            "attestations must never be purged"
        );
    }

    #[tokio::test]
    async fn run_cleanup_is_idempotent() {
        let state = test_state();
        seed_state(&state).await;

        state.retention_store.run_cleanup(&state).await.unwrap();
        let after_first = gyre_ports::NotificationRepository::list_recent(&*state.notifications, 100)
            .await
            .unwrap()
            .len();
        let audit_first = gyre_ports::AuditRepository::query(&*state.audit, &gyre_ports::AuditQueryFilter { limit: 100, ..Default::default() })
            .await
            .unwrap()
            .len();

        // Second consecutive run must not delete anything new.
        state.retention_store.run_cleanup(&state).await.unwrap();
        let after_second = gyre_ports::NotificationRepository::list_recent(&*state.notifications, 100)
            .await
            .unwrap()
            .len();
        let audit_second = gyre_ports::AuditRepository::query(&*state.audit, &gyre_ports::AuditQueryFilter { limit: 100, ..Default::default() })
            .await
            .unwrap()
            .len();
        assert_eq!(after_first, after_second);
        assert_eq!(audit_first, audit_second);
    }

    #[test]
    fn agent_log_line_is_old_boundaries() {
        let cutoff = 1000;
        assert!(agent_log_line_is_old("[999] x", cutoff));
        assert!(!agent_log_line_is_old("[1000] x", cutoff), "cutoff is exclusive");
        assert!(!agent_log_line_is_old("[1001] x", cutoff));
        assert!(!agent_log_line_is_old("no prefix", cutoff), "unparseable prefix kept");
        assert!(!agent_log_line_is_old("[abc] x", cutoff), "non-numeric ts kept");
        assert!(!agent_log_line_is_old("", cutoff));
    }

    #[test]
    fn snapshot_tiering_keeps_24h_24_7d_7_4w_4() {
        let now = 1_000 * DAY;
        let mut files = Vec::new();
        // 30 files aged 0..29 hours. The newest 24 fill the 24h tier; the 6
        // oldest (24h–29h) spill toward the 7d tier.
        for i in 0..30 {
            files.push((format!("recent-{i}"), now - i * 3_600));
        }
        // 4 files aged 3–6 days.
        for i in 3..7 {
            files.push((format!("week-{i}"), now - i * DAY));
        }
        // 6 files aged 15,17,19,21,23,25 days (all within the 4w tier).
        for i in 0..6 {
            files.push((format!("month-{i}"), now - (15 + 2 * i) * DAY));
        }
        // 3 files older than 4 weeks — always deleted.
        for i in 0..3 {
            files.push((format!("ancient-{i}"), now - (40 + i) * DAY));
        }

        // Expected survivors (35 = 24 + 7 + 4):
        // - 24h tier (24): recent-0..recent-23
        // - 7d tier (7): recent-24..recent-29 (the hour-aged spillover) + week-3
        // - 4w tier (4): week-4, week-5, week-6, month-0 (newest first)
        // Deleted (8): month-1..month-5, ancient-0..2
        let deletions = snapshot_tier_deletions(now, &files);
        assert_eq!(deletions.len(), 8, "deleted: {deletions:?}");
        for name in ["month-1", "month-2", "month-3", "month-4", "month-5"] {
            assert!(deletions.contains(&name.to_string()), "{name} should be deleted");
        }
        for i in 0..3 {
            assert!(deletions.contains(&format!("ancient-{i}")));
        }
        for i in 0..24 {
            assert!(!deletions.contains(&format!("recent-{i}")), "recent-{i} must survive");
        }
        for name in ["week-3", "week-4", "week-5", "week-6", "month-0"] {
            assert!(!deletions.contains(&name.to_string()), "{name} must survive");
        }
    }

    #[test]
    fn snapshot_tiering_empty_and_small_sets() {
        assert!(snapshot_tier_deletions(0, &[]).is_empty());
        // A handful of recent files — all kept.
        let files: Vec<(String, u64)> = (0..5).map(|i| (format!("f{i}"), 100 - i)).collect();
        assert!(snapshot_tier_deletions(100, &files).is_empty());
    }

    #[test]
    fn next_daily_run_secs_matches_wall_clock() {
        let day = 86_400u64;
        // 01:00 UTC, job at 02:00 → 1h away.
        assert_eq!(next_daily_run_secs(day + 3_600, 2), 3_600);
        // 03:00 UTC, job at 02:00 → next day's 02:00, 23h away.
        assert_eq!(next_daily_run_secs(day + 3 * 3_600, 2), 23 * 3_600);
        // Exactly 02:00 → next run is a full day away.
        assert_eq!(next_daily_run_secs(day + 2 * 3_600, 2), day);
        // 23:59 UTC, job at 00:00 → 1 minute away.
        assert_eq!(next_daily_run_secs(2 * day - 60, 0), 60);
    }
}
